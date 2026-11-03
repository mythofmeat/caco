//! An HTTP file store for the mirror tests, on a loopback socket, so that they run
//! with no network and no containers. It behaves the way the real ones were seen to:
//! Gitea 28.3's generic registry, a WebDAV server, and a static file server.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

/// What kind of store to behave as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flavor {
    /// `PUT` over a file is a 409. Paths are `<version>/<file>` and nothing shallower.
    /// Some file names are a 400. No `MKCOL`.
    Registry,
    /// `PUT` overwrites, and a missing parent directory is a 409 until `MKCOL`.
    Webdav,
    /// `GET` and `HEAD` and nothing else.
    Static,
}

#[derive(Default)]
struct State {
    files: HashMap<String, Vec<u8>>,
    dirs: Vec<String>,
    log: Vec<String>,
    /// `(method, path)` of a request to answer with this status instead, once.
    fail_once: Vec<(String, String, u16)>,
    /// Extra headers on every response.
    headers: Vec<(String, String)>,
    /// Whether a WebDAV store has the folder its address names.
    root_exists: bool,
}

/// One running server. It lives until the test process does.
#[derive(Clone)]
pub(crate) struct TestServer {
    pub(crate) url: String,
    flavor: Flavor,
    /// The whole `Authorization` header it wants, if it wants one.
    login: Option<String>,
    state: Arc<Mutex<State>>,
}

impl TestServer {
    pub(crate) fn start(flavor: Flavor, login: Option<&str>) -> Self {
        Self::start_with(flavor, login, false)
    }

    /// A WebDAV store whose address names a folder that is not there yet.
    pub(crate) fn start_without_root(login: Option<&str>) -> Self {
        Self::start_with(Flavor::Webdav, login, true)
    }

    fn start_with(flavor: Flavor, login: Option<&str>, missing_root: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/root", listener.local_addr().unwrap());
        let server = Self {
            url,
            flavor,
            login: login.map(str::to_string),
            state: Arc::new(Mutex::new(State {
                root_exists: flavor != Flavor::Webdav || !missing_root,
                ..Default::default()
            })),
        };
        let handle = server.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let handle = handle.clone();
                std::thread::spawn(move || handle.serve(stream));
            }
        });
        server
    }

    /// The file at `path` under the root, as it would be fetched.
    pub(crate) fn file(&self, path: &str) -> Option<Vec<u8>> {
        self.state.lock().unwrap().files.get(path).cloned()
    }

    pub(crate) fn put(&self, path: &str, bytes: &[u8]) {
        let mut state = self.state.lock().unwrap();
        state.files.insert(path.to_string(), bytes.to_vec());
        if let Some((dir, _)) = path.rsplit_once('/') {
            state.dirs.push(dir.to_string());
        }
    }

    /// Requests so far, as `METHOD path`.
    pub(crate) fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    pub(crate) fn clear_log(&self) {
        self.state.lock().unwrap().log.clear();
    }

    /// Answer the next `method` of `path` with `status`, then carry on as normal.
    pub(crate) fn fail_once(&self, method: &str, path: &str, status: u16) {
        let mut state = self.state.lock().unwrap();
        state
            .fail_once
            .push((method.to_string(), path.to_string(), status));
    }

    pub(crate) fn add_header(&self, name: &str, value: &str) {
        let mut state = self.state.lock().unwrap();
        state.headers.push((name.to_string(), value.to_string()));
    }

    fn serve(&self, stream: TcpStream) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let (Some(method), Some(target)) = (parts.next(), parts.next()) else {
            return;
        };

        let mut content_length = 0usize;
        let mut authorization = None;
        let mut content_type = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
            let Some((name, value)) = line.trim().split_once(':') else {
                continue;
            };
            match name.to_ascii_lowercase().as_str() {
                "content-length" => content_length = value.trim().parse().unwrap_or(0),
                "authorization" => authorization = Some(value.trim().to_string()),
                "content-type" => content_type = Some(value.trim().to_string()),
                _ => {}
            }
        }
        let mut body = vec![0; content_length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }

        let path = urlencoding::decode(target.strip_prefix("/root/").unwrap_or(target))
            .map(|path| path.trim_end_matches('/').to_string())
            .unwrap_or_default();
        let (status, payload, length) = self.answer(
            method,
            &path,
            &body,
            authorization.as_deref(),
            content_type.as_deref(),
        );

        let extra: String = self
            .state
            .lock()
            .unwrap()
            .headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}\r\n"))
            .collect();
        let mut stream = stream;
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {length}\r\nConnection: close\r\n{extra}\r\n"
        );
        let _ = stream.write_all(head.as_bytes());
        if method != "HEAD" {
            let _ = stream.write_all(&payload);
        }
        let _ = stream.flush();
    }

    /// What to answer: the status, the body, and the length to declare.
    fn answer(
        &self,
        method: &str,
        path: &str,
        body: &[u8],
        authorization: Option<&str>,
        content_type: Option<&str>,
    ) -> (u16, Vec<u8>, usize) {
        let mut state = self.state.lock().unwrap();
        state.log.push(format!("{method} {path}"));

        if let Some(position) = state
            .fail_once
            .iter()
            .position(|(m, p, _)| m == method && p == path)
        {
            let (_, _, status) = state.fail_once.remove(position);
            return (status, Vec::new(), 0);
        }

        let authorized = self.login.is_none() || authorization == self.login.as_deref();
        // A registry can have public reads, and a WebDAV server with a password has none.
        let is_write = !matches!(method, "GET" | "HEAD");
        if (is_write || self.flavor == Flavor::Webdav) && !authorized {
            return (401, Vec::new(), 0);
        }

        let segments = path.split('/').count();
        match (self.flavor, method) {
            (Flavor::Static, "GET" | "HEAD") | (Flavor::Webdav, "GET" | "HEAD") => {
                file_answer(&state, path)
            }
            (Flavor::Static, _) => (405, Vec::new(), 0),
            // The registry has no path shallower than `<version>/<file>`.
            (Flavor::Registry, _) if segments != 2 => (405, Vec::new(), 0),
            (Flavor::Registry, "GET" | "HEAD") => file_answer(&state, path),
            (Flavor::Registry, "PUT") => {
                if content_type.is_some_and(|t| t.starts_with("application/x-www-form-urlencoded"))
                {
                    return (500, Vec::new(), 0);
                }
                let name = path.rsplit('/').next().unwrap_or_default();
                if !registry_accepts(name) {
                    return (400, Vec::new(), 0);
                }
                if state.files.contains_key(path) {
                    return (409, Vec::new(), 0);
                }
                state.files.insert(path.to_string(), body.to_vec());
                (201, Vec::new(), 0)
            }
            (Flavor::Registry, "DELETE") => {
                if state.files.remove(path).is_some() {
                    (204, Vec::new(), 0)
                } else {
                    (404, Vec::new(), 0)
                }
            }
            (Flavor::Registry, _) => (405, Vec::new(), 0),
            (Flavor::Webdav, "PUT") => {
                if let Some((dir, _)) = path.rsplit_once('/')
                    && !state.dirs.iter().any(|known| known == dir)
                {
                    return (409, Vec::new(), 0);
                }
                if !state.root_exists {
                    return (409, Vec::new(), 0);
                }
                let existed = state
                    .files
                    .insert(path.to_string(), body.to_vec())
                    .is_some();
                (if existed { 204 } else { 201 }, Vec::new(), 0)
            }
            (Flavor::Webdav, "MKCOL") => {
                if path.is_empty() {
                    state.root_exists = true;
                    return (201, Vec::new(), 0);
                }
                if !state.root_exists {
                    return (409, Vec::new(), 0);
                }
                state.dirs.push(path.to_string());
                (201, Vec::new(), 0)
            }
            (Flavor::Webdav, "DELETE") => {
                if state.files.remove(path).is_some() {
                    (204, Vec::new(), 0)
                } else {
                    (404, Vec::new(), 0)
                }
            }
            (Flavor::Webdav, _) => (405, Vec::new(), 0),
        }
    }
}

fn file_answer(state: &State, path: &str) -> (u16, Vec<u8>, usize) {
    match state.files.get(path) {
        Some(bytes) => (200, bytes.clone(), bytes.len()),
        None => (404, Vec::new(), 0),
    }
}

/// What Gitea 28.3 was seen to take: ASCII word characters and some punctuation, with
/// no apostrophe or comma, and no leading or trailing space.
fn registry_accepts(name: &str) -> bool {
    name.trim() == name
        && !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_+=:;.()[]{}~!@#$%^& ".contains(c))
}
