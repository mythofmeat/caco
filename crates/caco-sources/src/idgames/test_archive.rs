//! A mirror of idgames and its API for the tests, on a loopback socket, so that they run
//! with no network and ask nothing of any volunteer. It behaves the way the real ones were
//! seen to: conditional `GET`s answered with a 304, `ls-laR.gz` as a file, and the API's
//! `warning` and `error` documents.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use caco_core::config::{MirrorConfig, MirrorLayout};
use serde_json::{Value, json};

/// The headers Cloudflare answers a challenge with, as `http::waf_challenge` knows them.
const CHALLENGE: (u16, &str) = (403, "cf-mitigated: challenge\r\nserver: cloudflare\r\n");

/// One file on the mirror.
#[derive(Clone)]
struct Served {
    bytes: Vec<u8>,
    etag: String,
    last_modified: String,
}

#[derive(Default)]
struct State {
    files: HashMap<String, Served>,
    /// What the API knows, as the entries it would answer with.
    entries: Vec<Value>,
    /// Every request, as `mirror <path>` or `api <action> <what>`.
    log: Vec<String>,
    /// Everything on the mirror answers with a challenge.
    mirror_blocked: bool,
    /// Everything on the API answers with a challenge.
    api_blocked: bool,
    /// How many more API requests to answer before it is blocked.
    api_answers_left: Option<usize>,
}

/// One running archive. It lives until the test process does.
#[derive(Clone)]
pub(crate) struct TestArchive {
    addr: String,
    state: Arc<Mutex<State>>,
}

impl TestArchive {
    pub(crate) fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let archive = Self {
            addr: listener.local_addr().unwrap().to_string(),
            state: Arc::default(),
        };
        let handle = archive.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let handle = handle.clone();
                std::thread::spawn(move || handle.serve(stream));
            }
        });
        archive
    }

    pub(crate) fn mirror_url(&self) -> String {
        format!("http://{}/pub/idgames/", self.addr)
    }

    pub(crate) fn api_url(&self) -> String {
        format!("http://{}/api.php", self.addr)
    }

    /// This as a source for a sync, named by its address like a public mirror is.
    pub(crate) fn as_mirror(&self) -> MirrorConfig {
        MirrorConfig {
            name: self.mirror_url(),
            url: Some(self.mirror_url()),
            layout: Some(MirrorLayout::IDGAMES.to_string()),
            ..Default::default()
        }
    }

    pub(crate) fn put_file(&self, path: &str, bytes: &[u8], last_modified: &str) {
        let mut state = self.state.lock().unwrap();
        let etag = format!("\"{}-{}\"", bytes.len(), state.files.len());
        state.files.insert(
            path.to_string(),
            Served {
                bytes: bytes.to_vec(),
                etag,
                last_modified: last_modified.to_string(),
            },
        );
    }

    pub(crate) fn remove_file(&self, path: &str) {
        self.state.lock().unwrap().files.remove(path);
    }

    pub(crate) fn put_entry(&self, entry: Value) {
        self.state.lock().unwrap().entries.push(entry);
    }

    pub(crate) fn block_mirror(&self) {
        self.state.lock().unwrap().mirror_blocked = true;
    }

    pub(crate) fn block_api(&self, blocked: bool) {
        self.state.lock().unwrap().api_blocked = blocked;
    }

    /// Answer this many more API requests, then start answering with challenges.
    pub(crate) fn block_api_after(&self, answers: usize) {
        self.state.lock().unwrap().api_answers_left = Some(answers);
    }

    /// Requests so far.
    pub(crate) fn log(&self) -> Vec<String> {
        self.state.lock().unwrap().log.clone()
    }

    pub(crate) fn clear_log(&self) {
        self.state.lock().unwrap().log.clear();
    }

    fn serve(&self, stream: TcpStream) {
        let mut reader = BufReader::new(stream.try_clone().unwrap());
        let mut request_line = String::new();
        if reader.read_line(&mut request_line).is_err() {
            return;
        }
        let mut parts = request_line.split_whitespace();
        let (Some(_method), Some(target)) = (parts.next(), parts.next()) else {
            return;
        };

        let mut if_none_match = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
                break;
            }
            if let Some((name, value)) = line.trim().split_once(':')
                && name.eq_ignore_ascii_case("if-none-match")
            {
                if_none_match = Some(value.trim().to_string());
            }
        }

        let (status, headers, body) = match target.split_once('?') {
            Some(("/api.php", query)) => self.answer_api(query),
            _ => self.answer_mirror(target, if_none_match.as_deref()),
        };
        let mut stream = stream;
        let head = format!(
            "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
            body.len()
        );
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(&body);
        let _ = stream.flush();
    }

    fn answer_mirror(&self, target: &str, if_none_match: Option<&str>) -> (u16, String, Vec<u8>) {
        let mut state = self.state.lock().unwrap();
        let path = target.strip_prefix("/pub/idgames/").unwrap_or(target);
        let path = urlencoding::decode(path).unwrap().into_owned();
        state.log.push(format!("mirror {path}"));
        if state.mirror_blocked {
            return (CHALLENGE.0, CHALLENGE.1.to_string(), Vec::new());
        }
        let Some(file) = state.files.get(&path) else {
            return (404, String::new(), b"<html>Not Found</html>".to_vec());
        };
        let headers = format!(
            "ETag: {}\r\nLast-Modified: {}\r\n",
            file.etag, file.last_modified
        );
        if if_none_match == Some(file.etag.as_str()) {
            return (304, headers, Vec::new());
        }
        (200, headers, file.bytes.clone())
    }

    fn answer_api(&self, query: &str) -> (u16, String, Vec<u8>) {
        let params: HashMap<String, String> = query
            .split('&')
            .filter_map(|pair| {
                let (name, value) = pair.split_once('=')?;
                Some((
                    name.to_string(),
                    urlencoding::decode(&value.replace('+', " "))
                        .ok()?
                        .into_owned(),
                ))
            })
            .collect();
        let param = |name: &str| params.get(name).map_or("", String::as_str);
        let action = param("action");

        let mut state = self.state.lock().unwrap();
        let what = [param("name"), param("file"), param("id"), param("query")]
            .into_iter()
            .find(|value| !value.is_empty())
            .unwrap_or("");
        state.log.push(format!("api {action} {what}"));
        if let Some(left) = &mut state.api_answers_left {
            if *left == 0 {
                state.api_blocked = true;
            } else {
                *left -= 1;
            }
        }
        if state.api_blocked {
            return (CHALLENGE.0, CHALLENGE.1.to_string(), Vec::new());
        }

        let path_of = |entry: &Value| {
            format!(
                "{}{}",
                entry["dir"].as_str().unwrap_or(""),
                entry["filename"].as_str().unwrap_or("")
            )
        };
        let answer = match action {
            "getfiles" => {
                let files: Vec<Value> = state
                    .entries
                    .iter()
                    .filter(|entry| entry["dir"] == param("name"))
                    .cloned()
                    .collect();
                if files.is_empty() {
                    json!({
                        "warning": {"type": "No Files", "message": "No files are under this directory."},
                        "meta": {"version": 3},
                    })
                } else {
                    json!({"content": {"file": files}, "meta": {"version": 3}})
                }
            }
            "get" => {
                let found = state.entries.iter().find(|entry| {
                    path_of(entry) == param("file")
                        || param("id").parse::<i64>().is_ok_and(|id| entry["id"] == id)
                });
                match found {
                    Some(entry) => json!({"content": entry, "meta": {"version": 3}}),
                    None => json!({
                        "error": {"type": "Bad File", "message": "No file associated with that path."},
                        "meta": {"version": 3},
                    }),
                }
            }
            "search" => {
                let field = param("type");
                let wanted = param("query").to_lowercase();
                let files: Vec<&Value> = state
                    .entries
                    .iter()
                    .filter(|entry| {
                        entry[field]
                            .as_str()
                            .is_some_and(|value| value.to_lowercase().contains(&wanted))
                    })
                    .collect();
                json!({"content": {"file": files}, "meta": {"version": 3}})
            }
            _ => json!({"error": {"type": "Bad Action", "message": "No such action."}}),
        };
        (200, String::new(), answer.to_string().into_bytes())
    }
}

/// One line of a listing: a path, a size and how `ls` writes its date.
pub(crate) fn listing_of(files: &[(&str, u64, &str)]) -> Vec<u8> {
    let mut by_dir: Vec<(String, Vec<String>)> = Vec::new();
    for (path, size, when) in files {
        let (dir, name) = path.rsplit_once('/').unwrap_or((".", path));
        let dir = if dir == "." {
            ".".to_string()
        } else {
            format!("./{dir}")
        };
        let line = format!("-rw-rw-r-- 1 idgmaint idgames {size:>9} {when} {name}");
        match by_dir.iter_mut().find(|(known, _)| *known == dir) {
            Some((_, lines)) => lines.push(line),
            None => by_dir.push((dir, vec![line])),
        }
    }
    let mut text = String::new();
    for (dir, lines) in by_dir {
        text.push_str(&format!("{dir}:\ntotal 1\n{}\n\n", lines.join("\n")));
    }
    gzip(text.as_bytes())
}

pub(crate) fn gzip(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
