//! Reading from and writing to a mirror. What one is, and its layout, are in
//! [`caco_core::mirrors`].
//!
//! A mirror is a directory or an HTTP file store, and [`Store`] is the only thing
//! here that knows which. Everything an HTTP store does is plain `GET`, `PUT`,
//! `HEAD` and `DELETE`, with a bearer token or a username and password, so a static
//! file server (read only), a WebDAV server and Gitea's generic package registry all
//! work, and they differ in only two ways, both handled in [`Store`]:
//!
//! - A `409` from a `PUT` means the file is already there on a registry, and that
//!   its parent directory is not on WebDAV. Which one is found out by asking.
//! - A registry refuses some file names outright. Gitea does so for an apostrophe,
//!   a comma or anything outside ASCII, so the file is stored under a safe name and
//!   the index records both.
//!
//! The index is the only listing a mirror has. No store here can be asked what it
//! holds, so a lost index cannot be rebuilt from the files behind it, and that shapes
//! how it is written. A store that will not overwrite a file gets a spare copy first
//! ([`Store::replace_file`]), and nothing writes back an index it could not read.
//!
//! A mirror with `layout = "idgames"` is a copy of the idgames archive, made by something
//! else: it has no index, it is never written to, and a file on it goes by its path in the
//! archive. [`crate::idgames::sync`] reads the public mirrors the same way, through
//! [`Store`], since they are that with a different owner.

use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

use caco_core::config::{MirrorConfig, MirrorLayout, MirrorLocation};
use caco_core::db::mirrors::{self, MirrorFile};
use caco_core::db::{self, WadRecord};
use caco_core::identity;
use caco_core::mirrors::{
    INDEX_PATH, MirrorEntry, MirrorIndex, entry_for_wad, safe_stored_name, stored_path,
};
use caco_core::placement::{self, Destination, partial_beside};
use caco_core::presence::StorePaths;
use reqwest::Method;
use reqwest::StatusCode;
use reqwest::blocking::{Body, Client, RequestBuilder, Response};
use reqwest::header::{
    CONTENT_LENGTH, CONTENT_TYPE, ETAG, IF_MODIFIED_SINCE, IF_NONE_MATCH, LAST_MODIFIED,
};
use rusqlite::Connection;

use crate::error::{Result, SourceError};
use crate::http::{TIMEOUT, USER_AGENT, waf_challenge};
use crate::idgames::listing::{self, LISTING_PATH};

/// What a failure to talk to a store amounts to before it is known which mirror it
/// was: the words for the user.
pub(crate) type StoreResult<T> = std::result::Result<T, String>;

/// How long a mirror gets to accept a connection. Shorter than the timeout on the
/// rest of the request, because a mirror that has gone is the likeliest failure
/// there is, on a laptop away from home, and Play is waiting on the answer.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// More than this is not an index, whatever a server says.
const MAX_INDEX_BYTES: u64 = 64 << 20;

/// How long a file gets to come down from a mirror. The largest uploads on idgames are
/// hundreds of megabytes, and this can be somebody's home server.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

/// Where a store that refuses to overwrite keeps a whole copy of the index while it
/// replaces the real one.
const SPARE_INDEX_PATH: &str = "index/index.json.new";

/// What a check that caco can write to a store writes, and takes away again. In the
/// index's directory, which exists on every mirror caco has set up and which a
/// registry takes for a version, and under a name that is nothing else.
const WRITE_TEST_PATH: &str = "index/caco-write-test";
const WRITE_TEST_SPARE_PATH: &str = "index/caco-write-test.new";

/// Held around reading an index and acting on it, so two uploads in this process
/// cannot both read the index and have the second write away the first's entry, and
/// a sync cannot write back rows an upload has just replaced. Another machine can
/// still race this one, and the loser's entry comes back when its WAD is uploaded
/// again. There is nothing in a plain file store to do better with.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

fn lock_index() -> std::sync::MutexGuard<'static, ()> {
    INDEX_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A failure to do something with one mirror, in the words the status bar and the
/// link dialog show.
fn mirror_error(mirror: &str, message: impl Into<String>) -> SourceError {
    SourceError::Mirror {
        mirror: mirror.to_string(),
        message: message.into(),
    }
}

// ---------------------------------------------------------------------------
// Stores
// ---------------------------------------------------------------------------

pub(crate) enum Store {
    Local {
        root: PathBuf,
    },
    Http {
        root: String,
        client: Client,
        auth: Auth,
        /// The variable the token should have come from, to say so when it did not.
        token_env: Option<String>,
    },
}

/// How to log in to an HTTP store. A bearer token where a store takes one, as Gitea
/// does, and a username and password where it does not: the WebDAV servers in common
/// use answer a bearer token with a 401.
pub(crate) enum Auth {
    None,
    Bearer(String),
    Basic {
        username: String,
        password: Option<String>,
    },
}

impl Auth {
    fn of(config: &MirrorConfig) -> Self {
        match (config.login(), config.token()) {
            (Some(username), password) => Auth::Basic {
                username: username.to_string(),
                password,
            },
            (None, Some(token)) => Auth::Bearer(token),
            (None, None) => Auth::None,
        }
    }
}

/// What a `PUT` found out.
#[derive(Debug, PartialEq, Eq)]
enum Put {
    Stored,
    /// `409`. See the module docs for the two things it can mean.
    Conflict,
    /// The store will not hold a file of this name.
    NameRefused,
}

/// How to ask a store whether a file has changed since it was last read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Validators {
    pub(crate) etag: Option<String>,
    pub(crate) last_modified: Option<String>,
}

/// What asking for a file that may not have changed found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Conditional {
    NotModified,
    NotThere,
    Changed {
        bytes: Vec<u8>,
        validators: Validators,
    },
}

impl Store {
    pub(crate) fn open(config: &MirrorConfig) -> StoreResult<Self> {
        Ok(match config.location()? {
            MirrorLocation::Path(root) => Store::Local { root },
            MirrorLocation::Url(root) => Store::Http {
                root,
                client: Client::builder()
                    .timeout(TIMEOUT)
                    .connect_timeout(CONNECT_TIMEOUT)
                    .user_agent(USER_AGENT)
                    .build()
                    .map_err(|e| e.to_string())?,
                auth: Auth::of(config),
                token_env: config.token_env_name().map(str::to_string),
            },
        })
    }

    /// The root of a local store, or an error if it is not there.
    ///
    /// A missing root is a drive that is not mounted, which says nothing about what
    /// the mirror holds. It must not read as an empty mirror, and a write must not
    /// quietly create one in its place.
    fn local_root(root: &Path) -> StoreResult<&Path> {
        if root.is_dir() {
            Ok(root)
        } else {
            Err(format!("{} is not there", root.display()))
        }
    }

    /// The index, or `None` if the store definitely has none: a new mirror, or one
    /// that has been emptied.
    fn read_index(&self) -> StoreResult<Option<MirrorIndex>> {
        // The spare is only there if a replacement was cut short, and then it is
        // the complete copy.
        let text = match self.read_text(INDEX_PATH)? {
            Some(text) => text,
            None => match self.read_text(SPARE_INDEX_PATH)? {
                Some(text) => text,
                None => return Ok(None),
            },
        };
        MirrorIndex::parse(&text)
            .map(Some)
            .map_err(|e| format!("could not read its index: {e}"))
    }

    fn read_text(&self, path: &str) -> StoreResult<Option<String>> {
        let Some(bytes) = self.read_bytes(path, MAX_INDEX_BYTES)? else {
            return Ok(None);
        };
        String::from_utf8(bytes)
            .map(Some)
            .map_err(|e| format!("could not read {path}: {e}"))
    }

    /// Whether reading from this store is free, as opposed to someone's bandwidth.
    pub(crate) fn is_local(&self) -> bool {
        matches!(self, Store::Local { .. })
    }

    /// The file at `path`, up to `max_bytes` of it, or `None` if there is none.
    pub(crate) fn read_bytes(&self, path: &str, max_bytes: u64) -> StoreResult<Option<Vec<u8>>> {
        match self {
            Store::Local { root } => {
                let root = Self::local_root(root)?;
                read_at_most(root.join(path), max_bytes, path)
            }
            Store::Http { .. } => {
                let response = self.send(self.http_request(Method::GET, path)?)?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                let response = self.require_success(response)?;
                read_response(response, max_bytes, path).map(Some)
            }
        }
    }

    /// The file at `path`, unless it is what `known` says was read last time.
    ///
    /// A file on disk goes by its size and modification time, which is what a web server
    /// makes an `ETag` from anyway.
    pub(crate) fn read_if_changed(
        &self,
        path: &str,
        known: Option<&Validators>,
        max_bytes: u64,
    ) -> StoreResult<Conditional> {
        match self {
            Store::Local { root } => {
                let full = Self::local_root(root)?.join(path);
                let metadata = match fs::metadata(&full) {
                    Ok(metadata) => metadata,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        return Ok(Conditional::NotThere);
                    }
                    Err(e) => return Err(format!("could not read {path}: {e}")),
                };
                let validators = local_validators(&metadata);
                if known == Some(&validators) {
                    return Ok(Conditional::NotModified);
                }
                match read_at_most(full, max_bytes, path)? {
                    Some(bytes) => Ok(Conditional::Changed { bytes, validators }),
                    None => Ok(Conditional::NotThere),
                }
            }
            Store::Http { .. } => {
                let mut request = self.http_request(Method::GET, path)?;
                if let Some(known) = known {
                    if let Some(etag) = &known.etag {
                        request = request.header(IF_NONE_MATCH, etag);
                    }
                    if let Some(when) = &known.last_modified {
                        request = request.header(IF_MODIFIED_SINCE, when);
                    }
                }
                let response = self.send(request)?;
                match response.status() {
                    StatusCode::NOT_MODIFIED => return Ok(Conditional::NotModified),
                    StatusCode::NOT_FOUND => return Ok(Conditional::NotThere),
                    _ => {}
                }
                let response = self.require_success(response)?;
                let header = |name| {
                    response
                        .headers()
                        .get(name)
                        .and_then(|value: &reqwest::header::HeaderValue| value.to_str().ok())
                        .map(str::to_string)
                };
                let validators = Validators {
                    etag: header(ETAG),
                    last_modified: header(LAST_MODIFIED),
                };
                let bytes = read_response(response, max_bytes, path)?;
                Ok(Conditional::Changed { bytes, validators })
            }
        }
    }

    /// Copy `file` to `dest`, refusing anything but exactly its size in bytes.
    fn download(&self, file: &MirrorFile, dest: &Path) -> StoreResult<()> {
        let name = &file.filename;
        match self.copy_file(&file.stored_path(), file.size, dest) {
            Ok(Some(copied)) if copied == file.size => Ok(()),
            Ok(Some(_)) => Err(format!(
                "its copy of {name} is not the {} bytes its index lists",
                file.size
            )),
            Ok(None) => Err(format!("its index lists {name}, but the file is not there")),
            Err(why) => Err(format!("could not download {name}: {why}")),
        }
    }

    /// Copy the file at `path` to `dest`, stopping one byte past `size` so that a file that
    /// is too long is noticed without filling the disk first. How many bytes were copied,
    /// or `None` if there is no such file.
    fn copy_file(&self, path: &str, size: u64, dest: &Path) -> StoreResult<Option<u64>> {
        let copied = match self {
            Store::Local { root } => {
                let root = Self::local_root(root)?;
                match File::open(root.join(path)) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
                    Err(e) => return Err(e.to_string()),
                    Ok(source) => copy_at_most(source, size, dest),
                }
            }
            Store::Http { .. } => {
                // Instead of the client's, which on a blocking client covers receiving the
                // whole body, and would fail anything that takes over 30 seconds.
                let request = self
                    .http_request(Method::GET, path)?
                    .timeout(DOWNLOAD_TIMEOUT);
                let response = self.send(request)?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(None);
                }
                let response = self.require_success(response)?;
                copy_at_most(response, size, dest)
            }
        };
        copied.map(Some).map_err(|e| e.to_string())
    }

    /// Whether the store has a file at `path` of this size.
    fn has_file(&self, path: &str, size: u64) -> StoreResult<bool> {
        match self {
            Store::Local { root } => {
                let root = Self::local_root(root)?;
                Ok(fs::metadata(root.join(path)).is_ok_and(|meta| meta.len() == size))
            }
            Store::Http { .. } => {
                let response = self.send(self.http_request(Method::HEAD, path)?)?;
                if response.status() == StatusCode::NOT_FOUND {
                    return Ok(false);
                }
                let response = self.require_success(response)?;
                // The header, not `Response::content_length`: that reads the size of
                // the body this response has, which for a HEAD is none.
                let length = response
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<u64>().ok());
                Ok(length.is_none_or(|length| length == size))
            }
        }
    }

    /// Store the file at `src` under `path`.
    fn put_file(&self, path: &str, src: &Path, size: u64) -> StoreResult<Put> {
        match self {
            Store::Local { root } => {
                let dest = Self::local_root(root)?.join(path);
                let tmp = partial_beside(&dest);
                let written = dest
                    .parent()
                    .map_or(Ok(()), fs::create_dir_all)
                    .and_then(|()| fs::copy(src, &tmp))
                    .and_then(|_| fs::rename(&tmp, &dest));
                match written {
                    Ok(()) => Ok(Put::Stored),
                    Err(e) => {
                        let _ = fs::remove_file(&tmp);
                        match e.kind() {
                            // A filesystem that will not have this name, such as
                            // exFAT on a NAS with a `?` in it.
                            io::ErrorKind::InvalidFilename | io::ErrorKind::InvalidInput => {
                                Ok(Put::NameRefused)
                            }
                            _ => Err(format!("could not write {path}: {e}")),
                        }
                    }
                }
            }
            Store::Http { .. } => {
                let file = File::open(src)
                    .map_err(|e| format!("could not read {}: {e}", src.display()))?;
                self.put_body(path, Body::from(file), upload_timeout(size))
            }
        }
    }

    fn put_body(&self, path: &str, body: Body, timeout: Duration) -> StoreResult<Put> {
        let request = self
            .http_request(Method::PUT, path)?
            // Said out loud, because Gitea takes a form's Content-Type to mean a
            // multipart upload and fails with a 500.
            .header(CONTENT_TYPE, "application/octet-stream")
            // Instead of the client's, which on a blocking client covers sending the
            // whole body, and would fail anything that takes over 30 seconds.
            .timeout(timeout)
            .body(body);
        let response = self.send(request)?;
        match response.status() {
            StatusCode::CONFLICT => Ok(Put::Conflict),
            StatusCode::BAD_REQUEST => Ok(Put::NameRefused),
            _ => self.require_success(response).map(|_| Put::Stored),
        }
    }

    /// Write a small file, replacing whatever is there.
    ///
    /// A local store renames it into place. An HTTP store that takes a `PUT` over an
    /// existing file is done in one request. Gitea's registry will not: the file has
    /// to be deleted first, and a failure between the two would lose it. So a whole
    /// copy goes up beside it first, and [`read_index`](Self::read_index) falls back
    /// to that, leaving something complete to read at every moment.
    fn replace_file(&self, path: &str, spare: &str, text: &str) -> StoreResult<()> {
        if let Store::Local { root } = self {
            let dest = Self::local_root(root)?.join(path);
            let tmp = partial_beside(&dest);
            let written = dest
                .parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| fs::write(&tmp, text))
                .and_then(|()| fs::rename(&tmp, &dest));
            return written.map_err(|e| {
                let _ = fs::remove_file(&tmp);
                format!("could not write {path}: {e}")
            });
        }

        match self.put_text(path, text)? {
            Put::Stored => return Ok(()),
            Put::NameRefused => return Err(format!("it refused {path}")),
            Put::Conflict => {}
        }
        if !self.has_any_file(path)? {
            // WebDAV's 409: there is no directory for it to go in yet.
            self.make_parent_dirs(path);
            return match self.put_text(path, text)? {
                Put::Stored => Ok(()),
                _ => Err(format!("it refused {path}")),
            };
        }

        // The store has the file, and will not overwrite it.
        self.delete(spare)?;
        if self.put_text(spare, text)? != Put::Stored {
            return Err(format!("it refused {spare}"));
        }
        self.delete(path)?;
        if self.put_text(path, text)? != Put::Stored {
            return Err(format!(
                "it refused {path}, so caco is reading {spare} until the next upload"
            ));
        }
        // Harmless if this fails: the spare is only read when `path` is missing, and
        // the next replacement writes a new one first.
        let _ = self.delete(spare);
        Ok(())
    }

    fn put_text(&self, path: &str, text: &str) -> StoreResult<Put> {
        self.put_body(path, Body::from(text.to_string()), TIMEOUT)
    }

    /// Whether anything is at `path`, whatever its size.
    fn has_any_file(&self, path: &str) -> StoreResult<bool> {
        let response = self.send(self.http_request(Method::HEAD, path)?)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(false);
        }
        self.require_success(response).map(|_| true)
    }

    /// Remove the file at `path`, if there is one.
    fn delete(&self, path: &str) -> StoreResult<()> {
        if let Store::Local { root } = self {
            return match fs::remove_file(root.join(path)) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => {
                    Err(format!("could not remove {path}: {e}"))
                }
                _ => Ok(()),
            };
        }
        let response = self.send(self.http_request(Method::DELETE, path)?)?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        self.require_success(response).map(|_| ())
    }

    /// Ask a WebDAV server for the directory `path` goes in. A store that is not one
    /// says no, which the `PUT` this comes before will report if it matters. A 409 from
    /// this means the mirror's own folder is not there yet either, as on a new mirror
    /// whose address names one.
    fn make_parent_dirs(&self, path: &str) {
        let Some((parent, _)) = path.rsplit_once('/') else {
            return;
        };
        if self.make_dir(parent) == Some(StatusCode::CONFLICT) {
            self.make_dir("");
            self.make_dir(parent);
        }
    }

    /// `MKCOL`, for a directory under the mirror's root, or the root for `""`. What the
    /// store answered, if it answered.
    fn make_dir(&self, path: &str) -> Option<StatusCode> {
        let method = Method::from_bytes(b"MKCOL").ok()?;
        let request = self.http_request(method, &format!("{path}/")).ok()?;
        self.send(request).ok().map(|response| response.status())
    }

    /// Make the place a new mirror goes, if the store can: the folder on a local store,
    /// and on a WebDAV one too if its address names one. A registry makes its package
    /// on the first upload and has no such step.
    fn create_root(&self) -> StoreResult<()> {
        match self {
            Store::Local { root } => fs::create_dir_all(root)
                .map_err(|e| format!("could not create {}: {e}", root.display())),
            Store::Http { .. } => {
                // A store that has no such thing says so, and one that already has it
                // says that. Either way the next step finds out if it matters.
                self.make_dir("");
                Ok(())
            }
        }
    }

    /// Whether anything can be written here, found out by writing something.
    fn check_writable(&self) -> StoreResult<()> {
        self.replace_file(WRITE_TEST_PATH, WRITE_TEST_SPARE_PATH, "caco was here\n")?;
        self.delete(WRITE_TEST_PATH)
    }

    /// Store a WAD's file, returning the name it was stored under when that is not
    /// its own.
    fn store_file(
        &self,
        sha256: &str,
        filename: &str,
        src: &Path,
        size: u64,
    ) -> StoreResult<Option<String>> {
        if self.store_under(&stored_path(sha256, filename), src, size)? {
            return Ok(None);
        }
        let safe = safe_stored_name(filename);
        if safe != filename && self.store_under(&stored_path(sha256, &safe), src, size)? {
            return Ok(Some(safe));
        }
        Err(format!("it refused the file name {filename}"))
    }

    /// Store a file at `path`, working out what a conflict meant. `false` if the
    /// store will not have that name.
    fn store_under(&self, path: &str, src: &Path, size: u64) -> StoreResult<bool> {
        match self.put_file(path, src, size)? {
            Put::Stored => Ok(true),
            Put::NameRefused => Ok(false),
            Put::Conflict => {
                // Content-addressed, so if it is there at this size it is this file.
                if self.has_file(path, size)? {
                    return Ok(true);
                }
                self.make_parent_dirs(path);
                match self.put_file(path, src, size)? {
                    Put::Stored => Ok(true),
                    Put::NameRefused => Ok(false),
                    Put::Conflict => Err(format!("it refused {path} (HTTP 409)")),
                }
            }
        }
    }

    fn http_request(&self, method: Method, path: &str) -> StoreResult<RequestBuilder> {
        let Store::Http {
            root, client, auth, ..
        } = self
        else {
            return Err("not an HTTP mirror".to_string());
        };
        let segments: Vec<String> = path
            .split('/')
            .map(|segment| urlencoding::encode(segment).into_owned())
            .collect();
        let request = client.request(method, format!("{root}/{}", segments.join("/")));
        Ok(match auth {
            Auth::None => request,
            Auth::Bearer(token) => request.bearer_auth(token),
            Auth::Basic { username, password } => request.basic_auth(username, password.as_ref()),
        })
    }

    fn send(&self, request: RequestBuilder) -> StoreResult<Response> {
        let response = request.send().map_err(|e| describe_request_error(&e))?;
        match waf_challenge(&response) {
            Some(kind) => Err(format!("{kind} answered with a challenge")),
            None => Ok(response),
        }
    }

    fn require_success(&self, response: Response) -> StoreResult<Response> {
        let status = response.status();
        if status.is_success() {
            return Ok(response);
        }
        let Store::Http {
            auth, token_env, ..
        } = self
        else {
            return Ok(response);
        };
        // Most of what goes wrong setting up a mirror for the first time, and on the
        // second machine a data dir reaches: the config travels, and a token does not.
        let refused = status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN;
        let missing = !matches!(
            auth,
            Auth::Bearer(_)
                | Auth::Basic {
                    password: Some(_),
                    ..
                }
        );
        Err(match (refused, missing, token_env) {
            (true, true, Some(name)) => format!("HTTP {status}: {name} is not set"),
            (true, true, None) => {
                format!("HTTP {status}: it needs a token or password, which Mirrors can save")
            }
            (true, false, _) => format!("HTTP {status}: it refused the token or password"),
            _ => format!("HTTP {status}"),
        })
    }
}

/// A failed request, for a person setting up a mirror. reqwest's own message stops at
/// "error sending request for url", and the reason is at the bottom of the chain: a
/// refused connection, or the self-signed certificate a home server is likely to have.
fn describe_request_error(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        return "timed out".to_string();
    }
    let mut root: &dyn std::error::Error = error;
    while let Some(source) = root.source() {
        root = source;
    }
    if error.is_connect() {
        format!("could not connect: {root}")
    } else {
        root.to_string()
    }
}

/// Copy at most one byte more than `size` from `reader` to `dest`, so a file that is
/// too long is noticed without filling the disk first. How many bytes were copied.
fn copy_at_most(reader: impl Read, size: u64, dest: &Path) -> io::Result<u64> {
    let mut file = File::create(dest)?;
    io::copy(&mut reader.take(size.saturating_add(1)), &mut file)
}

/// A local file, up to `max_bytes` of it. `name` is for the error.
fn read_at_most(path: PathBuf, max_bytes: u64, name: &str) -> StoreResult<Option<Vec<u8>>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("could not read {name}: {e}")),
    };
    let mut bytes = Vec::new();
    file.take(max_bytes)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("could not read {name}: {e}"))?;
    Ok(Some(bytes))
}

/// A response's body, up to `max_bytes` of it.
fn read_response(response: Response, max_bytes: u64, name: &str) -> StoreResult<Vec<u8>> {
    let mut bytes = Vec::new();
    response
        .take(max_bytes)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("could not read {name}: {e}"))?;
    Ok(bytes)
}

/// What stands in for an `ETag` and a `Last-Modified` for a file on disk.
fn local_validators(metadata: &fs::Metadata) -> Validators {
    let modified = metadata
        .modified()
        .ok()
        .map(chrono::DateTime::<chrono::Utc>::from);
    Validators {
        etag: Some(format!(
            "{}-{}",
            metadata.len(),
            modified.map_or(0, |when| when.timestamp())
        )),
        last_modified: modified.map(|when| when.to_rfc2822()),
    }
}

/// A generous time for sending `size` bytes: a minute, and a slow uplink's rate.
fn upload_timeout(size: u64) -> Duration {
    const SLOW_BYTES_PER_SECOND: u64 = 128 << 10;
    Duration::from_secs(60 + size / SLOW_BYTES_PER_SECOND)
}

// ---------------------------------------------------------------------------
// Syncing
// ---------------------------------------------------------------------------

/// What reading every mirror's index found.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncReport {
    /// Whether anything a view of the library shows has changed.
    pub changed: bool,
    /// Mirrors whose index could not be read, as `(name, why)`. What they were last
    /// known to hold still counts: one that cannot be reached today says nothing
    /// about what is on it, and a laptop away from home should not see half its
    /// library go manual.
    pub failed: Vec<(String, String)>,
    /// Mirrors whose index was read, as `(name, how many files it lists)`. A mirror
    /// holding nothing is here with a zero, which is how to tell one that is empty from
    /// one that has not been read.
    pub read: Vec<(String, usize)>,
    /// Index entries that could not be used, and were left out.
    pub skipped: usize,
}

/// Read every configured mirror's index into the library, and forget mirrors that
/// are no longer configured.
///
/// One request per mirror, so run it off the UI thread. An index that is definitely
/// not there empties that mirror's rows, because the safe mistake is to understate:
/// a WAD wrongly reading as manual costs a warning, and one wrongly reading as
/// automatic can have its only copy cleaned up.
pub fn sync_mirrors(
    conn: &Connection,
    configured: &[MirrorConfig],
) -> caco_core::Result<SyncReport> {
    let mut report = SyncReport::default();
    let names: Vec<&str> = configured
        .iter()
        .map(|mirror| mirror.name.as_str())
        .collect();
    report.changed |= mirrors::remove_other_mirrors(conn, &names)?;

    let mut seen = Vec::new();
    for mirror in configured {
        // Settings refuses both of these, and a file edited by hand gets here.
        if mirror.name.trim().is_empty() {
            let why = "it needs a name".to_string();
            report.failed.push((mirror.name.clone(), why));
            continue;
        }
        if seen.contains(&mirror.name.as_str()) {
            let why = "another mirror has the same name".to_string();
            report.failed.push((mirror.name.clone(), why));
            continue;
        }
        seen.push(mirror.name.as_str());

        match mirror.layout() {
            Ok(MirrorLayout::Caco) => {}
            Ok(MirrorLayout::Idgames) => {
                // Nothing to read: it has no index, and rows would only mean that it
                // once had one. It holds what idgames holds.
                report.changed |= mirrors::sync_files(conn, &mirror.name, &[])?;
                match Store::open(mirror).and_then(|store| check_idgames_copy(&store)) {
                    Ok(_) => report.read.push((mirror.name.clone(), 0)),
                    Err(why) => report.failed.push((mirror.name.clone(), why)),
                }
                continue;
            }
            Err(why) => {
                report.failed.push((mirror.name.clone(), why));
                continue;
            }
        }

        let _lock = lock_index();
        match Store::open(mirror).and_then(|store| store.read_index()) {
            Ok(index) => {
                let (entries, skipped) = index.unwrap_or_default().entries();
                report.skipped += skipped;
                report.changed |= mirrors::sync_files(conn, &mirror.name, &entries)?;
                report.read.push((mirror.name.clone(), entries.len()));
            }
            Err(why) => report.failed.push((mirror.name.clone(), why)),
        }
    }
    Ok(report)
}

// ---------------------------------------------------------------------------
// Setting up
// ---------------------------------------------------------------------------

/// What trying a mirror found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorProbe {
    /// How many files its index lists, or `None` if it has no index: a new mirror.
    pub files: Option<usize>,
    /// It is a local folder that is not there. Setting it up creates it.
    pub folder_missing: bool,
    /// Whether caco could write to it, if it was asked to and could tell.
    pub can_write: Option<std::result::Result<(), String>>,
}

impl MirrorProbe {
    /// Whether nothing stands in the way of using this mirror as configured.
    pub fn is_ok(&self) -> bool {
        !matches!(self.can_write, Some(Err(_)))
    }
}

/// Try a mirror before relying on it, leaving nothing behind: read its index, and if
/// caco is meant to upload, write a small file and remove it again. So a passing probe
/// means this configuration will work, and not just that a server answered.
///
/// Takes the config rather than a name, because the mirror being tried is usually not
/// saved yet. Run it off the UI thread, since it is a few requests, and however long a
/// server that has gone takes to fail. It takes no lock, because it writes nothing the
/// index depends on, and a bulk upload to another mirror should not make it wait.
pub fn probe_mirror(config: &MirrorConfig) -> std::result::Result<MirrorProbe, String> {
    let store = Store::open(config)?;
    if config.layout()? == MirrorLayout::Idgames {
        return Ok(MirrorProbe {
            files: check_idgames_copy(&store)?,
            folder_missing: false,
            can_write: None,
        });
    }
    if let Store::Local { root } = &store
        && !root.exists()
    {
        return Ok(MirrorProbe {
            files: None,
            folder_missing: true,
            can_write: None,
        });
    }
    if let Store::Local { root } = &store
        && !root.is_dir()
    {
        return Err(format!("{} is a file, not a folder", root.display()));
    }

    let files = store.read_index()?.map(|index| index.entries().0.len());
    let can_write = config.write.then(|| store.check_writable());
    Ok(MirrorProbe {
        files,
        folder_missing: false,
        can_write,
    })
}

/// Make a mirror ready to use: create its folder if it is a local one that is not
/// there, and if caco is meant to write to it and it has no index, give it an empty
/// one. Leaves a mirror that already has an index as it is, and a copy of idgames, which
/// is nothing of caco's, as it found it.
///
/// Only the setup wizard calls this, when someone has said they want a mirror there.
/// An upload must not create a folder, because to an upload a missing one means a drive
/// that is not mounted.
pub fn set_up_mirror(config: &MirrorConfig) -> Result<()> {
    let fail = |message: String| mirror_error(&config.name, message);
    if config.layout().map_err(fail)? == MirrorLayout::Idgames {
        return Ok(());
    }
    let store = Store::open(config).map_err(fail)?;
    store.create_root().map_err(fail)?;
    if config.write {
        let _lock = lock_index();
        if store.read_index().map_err(fail)?.is_none() {
            store
                .replace_file(
                    INDEX_PATH,
                    SPARE_INDEX_PATH,
                    &MirrorIndex::default().to_json(),
                )
                .map_err(fail)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Fetching
// ---------------------------------------------------------------------------

/// Download a WAD's file from whichever mirror has it, into the directory it belongs
/// in. Doesn't write to the row, which the caller does for every route a file can
/// arrive by.
///
/// `Ok(None)` when no configured mirror lists it. An error when some did and none
/// could give it up: the first failure, since it is about the most likely source.
pub fn fetch_from_mirrors(
    conn: &Connection,
    wad: &WadRecord,
    configured: &[MirrorConfig],
    paths: &StorePaths,
) -> Result<Option<PathBuf>> {
    let files = mirrors::files_for_wad(conn, wad.id)?;
    let dir = paths.store_dir(wad.retrievability());

    let mut first_failure = None;
    let mut tried: Vec<&str> = Vec::new();
    // In the order the match gave: the strongest evidence, then the newest release.
    // Each file then from its mirrors in the order the user listed them.
    for file in &files {
        if tried.contains(&file.sha256.as_str()) {
            continue;
        }
        tried.push(&file.sha256);

        for mirror in configured {
            let Some(copy) = files
                .iter()
                .find(|copy| copy.sha256 == file.sha256 && copy.mirror == mirror.name)
            else {
                continue;
            };
            match Store::open(mirror).and_then(|store| fetch_one(&store, copy, &dir)) {
                Ok(path) => return Ok(Some(path)),
                Err(why) => {
                    first_failure.get_or_insert_with(|| mirror_error(&mirror.name, why));
                }
            }
        }
    }
    first_failure.map_or(Ok(None), Err)
}

/// Download one file into `dir`: to a temporary name, then checked, then renamed. A
/// partial or corrupt download therefore never sits under a name the presence check
/// or a launch could pick up.
fn fetch_one(store: &Store, file: &MirrorFile, dir: &Path) -> StoreResult<PathBuf> {
    fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let dest = match placement::destination(dir, &file.filename, &file.sha256)
        .map_err(|e| e.to_string())?
    {
        Destination::AlreadyHere(path) => return Ok(path),
        Destination::Write(path) => path,
    };
    let partial = partial_beside(&dest);
    let fetched = store
        .download(file, &partial)
        .and_then(|()| check_download(&partial, file))
        .and_then(|()| {
            fs::rename(&partial, &dest)
                .map_err(|e| format!("could not write {}: {e}", dest.display()))
        });
    if let Err(why) = fetched {
        let _ = fs::remove_file(&partial);
        return Err(why);
    }
    Ok(dest)
}

/// Copy a file off the first copy of idgames that has it, into `dir`. `archive_path` is
/// where it is in the archive.
///
/// `Ok(None)` if none of them has it, which is ordinary: a copy can be of part of the
/// archive, or behind it. An error if one that should could not give it up: the first
/// failure, as in [`fetch_from_mirrors`].
///
/// There is no hash to check this against, since nothing says what a file in an archive
/// with one name per path has to be. The launch this comes before hashes it, and asks if
/// it is not what the WAD was last played with.
pub fn fetch_from_idgames_copies(
    configured: &[MirrorConfig],
    archive_path: &str,
    dir: &Path,
) -> Result<Option<PathBuf>> {
    let Some(filename) = safe_archive_filename(archive_path) else {
        return Ok(None);
    };
    let mut first_failure = None;
    for mirror in configured.iter().filter(|mirror| mirror.is_idgames_copy()) {
        match Store::open(mirror)
            .and_then(|store| fetch_by_path(&store, archive_path, filename, dir))
        {
            Ok(Some(path)) => return Ok(Some(path)),
            Ok(None) => {}
            Err(why) => {
                first_failure.get_or_insert_with(|| mirror_error(&mirror.name, why));
            }
        }
    }
    first_failure.map_or(Ok(None), Err)
}

/// The last part of a path in the archive, if that path stays inside whatever root it
/// gets joined to: it may have come from a WAD row, which came from a page on a website.
fn safe_archive_filename(archive_path: &str) -> Option<&str> {
    if archive_path
        .split('/')
        .any(|part| part.is_empty() || !caco_core::mirrors::is_bare_file_name(part))
    {
        return None;
    }
    archive_path.rsplit('/').next()
}

/// Copy one file from a store into `dir` under its own name, through a temporary one.
/// Its hash only decides what it is called if that name is taken, as in
/// [`placement::destination`], since `map01.zip` is on the archive more than once.
fn fetch_by_path(
    store: &Store,
    archive_path: &str,
    filename: &str,
    dir: &Path,
) -> StoreResult<Option<PathBuf>> {
    fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let partial = partial_beside(&dir.join(filename));
    let fetched = store
        .copy_file(archive_path, u64::MAX - 1, &partial)
        .and_then(|found| {
            if found.is_none() {
                return Ok(None);
            }
            let hashes = identity::hash_file(&partial)
                .map_err(|e| format!("could not check {filename}: {e}"))?;
            let destination =
                placement::destination(dir, filename, &hashes.sha256).map_err(|e| e.to_string())?;
            let dest = match destination {
                Destination::AlreadyHere(path) => {
                    let _ = fs::remove_file(&partial);
                    return Ok(Some(path));
                }
                Destination::Write(path) => path,
            };
            fs::rename(&partial, &dest)
                .map_err(|e| format!("could not write {}: {e}", dest.display()))?;
            Ok(Some(dest))
        });
    if !matches!(fetched, Ok(Some(_))) {
        let _ = fs::remove_file(&partial);
    }
    fetched
}

/// What a copy of idgames has at its root, to tell one from the folder above it: how many
/// uploads its listing names, or `None` for a copy with no listing, which has to at least
/// have the directory the WADs are in. Either way, whether it is there at all.
fn check_idgames_copy(store: &Store) -> StoreResult<Option<usize>> {
    let bytes = store.read_bytes(LISTING_PATH, MAX_INDEX_BYTES)?;
    if let Some(bytes) = bytes {
        let text =
            listing::decompress(&bytes).map_err(|e| format!("could not read its listing: {e}"))?;
        let today = chrono::Utc::now().date_naive();
        return Ok(Some(listing::parse_listing(&text, today).len()));
    }
    match store {
        Store::Local { root } if root.join("levels").is_dir() => Ok(None),
        Store::Local { root } => Err(format!(
            "there is no {LISTING_PATH} or levels folder directly under {}",
            root.display()
        )),
        Store::Http { .. } => Err(format!("there is no {LISTING_PATH} at that address")),
    }
}

/// Whether the file at `path` is the one the index promised. The only thing that
/// makes a file on someone's server safe to play in place of the one a WAD lost.
fn check_download(path: &Path, file: &MirrorFile) -> StoreResult<()> {
    let hashes =
        identity::hash_file(path).map_err(|e| format!("could not check {}: {e}", file.filename))?;
    if hashes.sha256 == file.sha256 {
        Ok(())
    } else {
        Err(format!(
            "its copy of {} is not the file its index lists",
            file.filename
        ))
    }
}

// ---------------------------------------------------------------------------
// Uploading
// ---------------------------------------------------------------------------

/// What [`upload_wad`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uploaded {
    pub mirror: String,
    pub filename: String,
    /// The mirror had the file already, so only what is known about it was added.
    pub already_there: bool,
}

/// Put a WAD's file on a mirror and add it to the index, then read the new index
/// into the library, which is what makes the WAD fetchable from here on.
///
/// The index is read before anything is written, so an index that cannot be read
/// (one from a newer caco, or a server's error page) costs nothing but an error
/// message. Reads the whole file to hash it unless it is known, so run it off the UI
/// thread. `added` is the time to record, as RFC 3339.
pub fn upload_wad(
    conn: &Connection,
    wad_id: i64,
    mirror: &MirrorConfig,
    added: &str,
) -> Result<Uploaded> {
    let fail = |message: String| mirror_error(&mirror.name, message);
    if mirror.is_idgames_copy() {
        return Err(fail(
            "it is a copy of idgames, which caco does not write to".to_string(),
        ));
    }

    let wad = db::get_wad(conn, wad_id, false)?
        .ok_or_else(|| SourceError::Core(caco_core::Error::WadNotFound(wad_id)))?;
    let path = wad
        .cached_path
        .as_deref()
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| fail(format!("{} has no file on this machine", wad.title)))?;
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| caco_core::mirrors::is_bare_file_name(name))
        .ok_or_else(|| fail(format!("{} cannot be used as a file name", path.display())))?
        .to_string();
    let identity = identity::verify(conn, &wad, &path)?
        .ok_or_else(|| fail(format!("could not read {}", path.display())))?
        .into_identity();

    let store = Store::open(mirror).map_err(fail)?;
    let _lock = lock_index();
    let mut index = store.read_index().map_err(fail)?.unwrap_or_default();

    let mut entry = entry_for_wad(conn, &wad, &identity, &filename, added)?;
    let existing = index.entry(&entry.sha256);
    let already_there = match &existing {
        Some(existing) => store
            .has_file(&existing.stored_path(), existing.size)
            .map_err(fail)?,
        None => false,
    };
    if !already_there {
        // A file the index lists and the store has lost goes back where the index says
        // it is, whatever this copy happens to be called.
        let name = existing
            .as_ref()
            .map_or(filename.as_str(), MirrorEntry::stored_name);
        entry.stored_as = store
            .store_file(&entry.sha256, name, &path, entry.size)
            .map_err(fail)?;
    }
    index.upsert(entry);
    store
        .replace_file(INDEX_PATH, SPARE_INDEX_PATH, &index.to_json())
        .map_err(fail)?;

    let (entries, _) = index.entries();
    mirrors::sync_files(conn, &mirror.name, &entries)?;

    Ok(Uploaded {
        mirror: mirror.name.clone(),
        filename,
        already_there,
    })
}

/// Where a bulk upload has got to.
#[derive(Debug)]
pub struct UploadProgress<'a> {
    /// How many are finished, so that `done / total` drives a progress bar directly.
    pub done: usize,
    pub total: usize,
    /// The WAD being uploaded now.
    pub title: &'a str,
}

/// What a bulk upload did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UploadReport {
    pub uploaded: usize,
    /// The WADs that did not make it, as `(title, why)`. One failing does not stop the
    /// rest, since each is on the mirror or not by itself.
    pub failed: Vec<(String, String)>,
    pub cancelled: bool,
}

/// Upload each of `wads` to a mirror, one at a time.
///
/// The worker's idea of a transaction, as in `enrich_service`: each WAD is finished
/// and in the index before the next one starts, so cancelling, or closing the app, is
/// just a shorter run. That costs one rewrite of the index per WAD, which is nothing
/// next to sending the files.
pub fn upload_wads(
    conn: &Connection,
    wads: &[WadRecord],
    mirror: &MirrorConfig,
    added: &str,
    progress: &mut dyn FnMut(UploadProgress<'_>),
    cancelled: &dyn Fn() -> bool,
) -> UploadReport {
    let mut report = UploadReport::default();
    for (done, wad) in wads.iter().enumerate() {
        if cancelled() {
            report.cancelled = true;
            break;
        }
        progress(UploadProgress {
            done,
            total: wads.len(),
            title: &wad.title,
        });
        match upload_wad(conn, wad.id, mirror, added) {
            Ok(_) => report.uploaded += 1,
            Err(e) => report.failed.push((wad.title.clone(), e.to_string())),
        }
    }
    report
}

#[cfg(test)]
mod test_server;

#[cfg(test)]
mod tests;
