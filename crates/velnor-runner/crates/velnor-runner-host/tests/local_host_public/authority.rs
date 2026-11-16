// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use bollard::Docker;
use bollard::query_parameters::ListContainersOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use super::TestResult;

pub(crate) const TOKEN: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
pub(crate) const JIT: &str = "ZmFrZS1qaXQtY29uZmlnLWZvci10aGUtdGVzdA==";

#[derive(Clone, Default)]
pub(crate) struct Run {
    pub(crate) job: String,
    pub(crate) ledger: Option<String>,
    pub(crate) refusal: Option<String>,
    pub(crate) event: Option<String>,
    pub(crate) git_ref: Option<String>,
    pub(crate) sha: Option<String>,
}

pub(crate) struct Fake {
    environment: String,
    docker: Docker,
    pub(crate) runs: Mutex<HashMap<(String, u64), Run>>,
    pub(crate) events: Mutex<Vec<String>>,
    pub(crate) reachable: AtomicBool,
    unauthorized: AtomicUsize,
    minted: AtomicUsize,
    pub(crate) delete_failures: AtomicUsize,
}

pub(crate) struct Served {
    pub(crate) fake: Arc<Fake>,
    pub(crate) address: SocketAddr,
}

impl Fake {
    pub(crate) fn unauthorized(&self) -> usize {
        self.unauthorized.load(Ordering::SeqCst)
    }

    pub(crate) fn events(&self) -> Vec<String> {
        self.events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    pub(crate) fn dispatch(&self, side: &str, id: u64, run: Run) {
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert((side.to_owned(), id), run);
        }
    }

    fn log(&self, event: String) {
        if let Ok(mut events) = self.events.lock() {
            events.push(event);
        }
    }

    async fn live(&self, side: &str) -> usize {
        let filters = HashMap::from([(
            "label".to_owned(),
            vec![
                format!("velnor-local={}", self.environment),
                format!("velnor-local.side={side}"),
            ],
        )]);
        self.docker
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters: Some(filters),
                ..Default::default()
            }))
            .await
            .map_or(usize::MAX, |containers| containers.len())
    }

    async fn answer(&self, method: &str, path: &str) -> (u16, String) {
        let parts: Vec<&str> = path.trim_start_matches('/').split('/').collect();
        match (method, parts.as_slice()) {
            ("POST", ["jit"]) => (200, self.jit().await),
            ("GET", ["runs", side, id, attempt]) => self.run(side, id, attempt),
            ("DELETE", ["agents", side, name]) => {
                let failing = self
                    .delete_failures
                    .try_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                        Some(if left == usize::MAX {
                            left
                        } else {
                            left.saturating_sub(1)
                        })
                    })
                    .is_ok_and(|left| left > 0);
                if failing {
                    self.log(format!("delete_failed:{side}:{name}"));
                    return (500, "{}".to_owned());
                }
                self.log(format!("delete:{side}:{name}"));
                (200, format!("{{\"name\":\"{name}\",\"deleted\":true}}"))
            }
            _ => (404, "{}".to_owned()),
        }
    }

    async fn jit(&self) -> String {
        let number = self.minted.fetch_add(1, Ordering::SeqCst) + 1;
        let name = format!("agent-{number}");
        for side in ["trusted", "untrusted"] {
            let live = self.live(side).await;
            self.log(format!("mint:{name}:live_{side}={live}"));
        }
        format!("{{\"name\":\"{name}\",\"jit\":\"{JIT}\",\"seconds\":0.1}}")
    }

    fn run(&self, side: &str, id: &str, attempt: &str) -> (u16, String) {
        let Ok(id) = id.parse::<u64>() else {
            return (404, "{}".to_owned());
        };
        let found = self
            .runs
            .lock()
            .ok()
            .and_then(|runs| runs.get(&(side.to_owned(), id)).cloned());
        let Some(run) = found else {
            return (404, "{}".to_owned());
        };
        let ledger = run
            .ledger
            .map_or("null".to_owned(), |ledger| format!("\"{ledger}\""));
        let refusal = run
            .refusal
            .map_or("null".to_owned(), |refusal| format!("\"{refusal}\""));
        let sha = run.sha.unwrap_or_else(|| "a".repeat(40));
        let event = run.event.unwrap_or_else(|| "push".to_owned());
        let git_ref = run.git_ref.unwrap_or_else(|| "refs/heads/main".to_owned());
        (
            200,
            format!(
                "{{\"side\":\"{side}\",\"id\":{id},\"attempt\":{attempt},\"event\":\"{event}\",\"ref\":\"{git_ref}\",\"sha\":\"{sha}\",\"workflow\":\".github/workflows/ci.yml\",\"jobs\":[\"{}\"],\"ledger\":{ledger},\"refusal\":{refusal}}}",
                run.job
            ),
        )
    }
}

async fn read_request(stream: &mut TcpStream) -> TestResult<(String, String, bool)> {
    let mut buffer = Vec::new();
    let mut chunk = [0_u8; 1024];
    let (head_end, length) = loop {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            return Err("closed before the request".into());
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&buffer[..position]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (position + 4, length);
        }
    };
    while buffer.len() < head_end + length {
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
    let mut first = head.lines().next().unwrap_or_default().split(' ');
    let method = first.next().unwrap_or_default().to_owned();
    let path = first.next().unwrap_or_default().to_owned();
    let bearer = format!("Bearer {TOKEN}");
    let authorized = head.lines().any(|line| {
        line.split_once(": ").is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == bearer
        })
    });
    Ok((method, path, authorized))
}

async fn serve_one(fake: &Fake, mut stream: TcpStream) -> TestResult<()> {
    if !fake.reachable.load(Ordering::SeqCst) {
        return Ok(());
    }
    let (method, path, authorized) = read_request(&mut stream).await?;
    let (status, body) = if authorized {
        fake.answer(&method, &path).await
    } else {
        fake.unauthorized.fetch_add(1, Ordering::SeqCst);
        (401, "{}".to_owned())
    };
    let response = format!(
        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await?;
    Ok(())
}

pub(crate) async fn serve(docker: Docker, environment: &str) -> TestResult<Served> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let fake = Arc::new(Fake {
        environment: environment.to_owned(),
        docker,
        runs: Mutex::new(HashMap::new()),
        events: Mutex::new(Vec::new()),
        reachable: AtomicBool::new(true),
        unauthorized: AtomicUsize::new(0),
        minted: AtomicUsize::new(0),
        delete_failures: AtomicUsize::new(0),
    });
    let handler = Arc::clone(&fake);
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let fake = Arc::clone(&handler);
            tokio::spawn(async move {
                let _served = serve_one(&fake, stream).await;
            });
        }
    });
    Ok(Served { fake, address })
}
