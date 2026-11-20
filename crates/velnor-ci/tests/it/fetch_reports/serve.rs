// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;

#[derive(Debug, Clone)]
pub(crate) struct Reply {
    status: u16,
    location: Option<String>,
    body: Vec<u8>,
}

impl Reply {
    pub(crate) fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            location: None,
            body: body.into(),
        }
    }

    pub(crate) fn status(status: u16) -> Self {
        Self {
            status,
            location: None,
            body: Vec::new(),
        }
    }

    pub(crate) fn redirect(location: &str) -> Self {
        Self {
            status: 302,
            location: Some(location.to_owned()),
            body: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hit {
    pub(crate) path: String,
    pub(crate) authorized: bool,
}

#[derive(Debug)]
pub(crate) struct Served {
    pub(crate) api: String,
    hits: Arc<Mutex<Vec<Hit>>>,
}

impl Served {
    pub(crate) fn hits(&self) -> Vec<Hit> {
        self.hits
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    pub(crate) fn count(&self, prefix: &str) -> usize {
        self.hits()
            .iter()
            .filter(|hit| hit.path.starts_with(prefix))
            .count()
    }
}

type Routes = Arc<Mutex<BTreeMap<String, Vec<Reply>>>>;

pub(crate) fn serve(routes: BTreeMap<String, Vec<Reply>>) -> Served {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| unreachable!("{e}"));
    let address = listener
        .local_addr()
        .unwrap_or_else(|e| unreachable!("{e}"));
    let api = format!("http://{address}");
    let routes: Routes = Arc::new(Mutex::new(routes));
    let hits = Arc::new(Mutex::new(Vec::new()));
    let (base, shared, seen) = (api.clone(), routes, Arc::clone(&hits));
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let (base, shared, seen) = (base.clone(), Arc::clone(&shared), Arc::clone(&seen));
            thread::spawn(move || answer(stream, &base, &shared, &seen));
        }
    });
    Served { api, hits }
}

fn answer(mut stream: TcpStream, base: &str, routes: &Routes, hits: &Mutex<Vec<Hit>>) {
    let mut request = Vec::new();
    let mut byte = [0_u8; 1];
    while !request.ends_with(b"\r\n\r\n") && stream.read(&mut byte).is_ok_and(|n| n == 1) {
        request.push(byte[0]);
    }
    let text = String::from_utf8_lossy(&request).to_lowercase();
    let original = String::from_utf8_lossy(&request).into_owned();
    let path = original
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    hits.lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(Hit {
            path: path.clone(),
            authorized: text.contains("\r\nauthorization:"),
        });
    let reply = {
        let mut routes = routes.lock().unwrap_or_else(PoisonError::into_inner);
        match routes.get_mut(&path) {
            Some(replies) if replies.len() > 1 => replies.remove(0),
            Some(replies) => replies.first().cloned().unwrap_or(Reply::status(404)),
            None => Reply::status(404),
        }
    };
    let location = reply
        .location
        .as_ref()
        .map_or_default(|to| format!("location: {}\r\n", to.replace("{base}", base)));
    let head = format!(
        "HTTP/1.1 {} Reply\r\ncontent-type: application/octet-stream\r\n{location}content-length: {}\r\nconnection: close\r\n\r\n",
        reply.status,
        reply.body.len()
    );
    stream.write_all(head.as_bytes()).ok();
    stream.write_all(&reply.body).ok();
}
