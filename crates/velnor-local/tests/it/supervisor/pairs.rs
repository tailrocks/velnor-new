// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::future::Future;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use velnor_local::io::authority::Backend;
use velnor_local::io::pair::{MintError, Minted};
use velnor_local::io::supervisor;
use velnor_local::side::Side;

use crate::support::temp_dir;

struct Server {
    asked: Arc<AtomicUsize>,
    online_after: usize,
}

impl Backend for Server {
    fn get(&self, _: Side, path: &str) -> impl Future<Output = io::Result<Option<Vec<u8>>>> + Send {
        let body = if path.contains("isagentonline") {
            let asked = self.asked.fetch_add(1, Ordering::SeqCst);
            format!(r#"{{"online":{}}}"#, asked >= self.online_after)
        } else {
            r#"{"value":[{"name":"trusted-1"}],"count":1}"#.to_owned()
        };
        std::future::ready(Ok(Some(body.into_bytes())))
    }

    fn delete(&self, _: Side, _: &str) -> impl Future<Output = io::Result<()>> + Send {
        std::future::ready(Ok(()))
    }

    fn mint(
        &self,
        _: Side,
        _: Option<&str>,
    ) -> impl Future<Output = Result<Option<Minted>, MintError>> + Send {
        std::future::ready(Ok(None))
    }
}

fn fake_authority(dir: &Path) -> (std::path::PathBuf, supervisor::Pairs) {
    let script = dir.join("fake-authority");
    std::fs::write(
        &script,
        "#!/bin/bash\nmkdir -p \"$3/runner\"\necho 127.0.0.1:9 > \"$3/authority/address\"\necho 'pair trusted-1 listening' > \"$3/runner/velnor-host.log\"\nexec -a \"$0 $1 $2 $3 $4 $5\" /bin/sleep 600\n",
    )
    .unwrap_or_else(|e| unreachable!("{e}"));
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
        .unwrap_or_else(|e| unreachable!("{e}"));
    let pairs = supervisor::Pairs {
        host: script.clone(),
        trusted: 1,
        ..supervisor::Pairs::default()
    };
    (script, pairs)
}

async fn start_listening_pair(
    name: &str,
    online_after: usize,
    timeout: Duration,
) -> (io::Result<supervisor::Endpoint>, usize, String) {
    let dir = temp_dir(name);
    let (script, pairs) = fake_authority(&dir);
    let asked = Arc::new(AtomicUsize::new(0));
    let server = Server {
        asked: Arc::clone(&asked),
        online_after,
    };
    let started =
        supervisor::start_pairs(&dir, &script, &pairs, async |_: &Path| Ok(server), timeout).await;
    let pid = std::fs::read_to_string(dir.join("authority/pid")).unwrap_or_default();
    let command = Command::new("ps")
        .args(["-p", pid.trim(), "-o", "command="])
        .output()
        .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
        .unwrap_or_default();
    Command::new("kill")
        .args(["-KILL", pid.trim()])
        .status()
        .ok();
    (started, asked.load(Ordering::SeqCst), command)
}

#[tokio::test]
async fn start_returns_only_once_the_agent_is_online_at_runner_server() {
    let (started, asked, _) =
        start_listening_pair("supervisor-online", 3, Duration::from_secs(10)).await;
    assert!(started.is_ok(), "{started:?}");
    assert!(asked > 3, "start returned after {asked} online checks");
}

#[tokio::test]
async fn start_fails_at_the_deadline_when_the_agent_never_comes_online() {
    let (started, asked, command) =
        start_listening_pair("supervisor-offline", usize::MAX, Duration::from_millis(500)).await;
    let message = started.err().map(|e| e.to_string()).unwrap_or_default();
    assert!(asked > 0, "the agent was never asked about");
    assert!(
        !command.contains("fake-authority"),
        "the authority was left running: [{command}]"
    );
    assert!(
        message.contains("0 of 1 trusted runners were online"),
        "{message}"
    );
}

#[tokio::test]
async fn start_fails_when_runner_server_cannot_be_opened_and_stops_the_authority() {
    let dir = temp_dir("supervisor-open-fails");
    let (script, pairs) = fake_authority(&dir);
    let started = supervisor::start_pairs(
        &dir,
        &script,
        &pairs,
        async |_: &Path| -> io::Result<Server> { Err(io::Error::other("runner.server is down")) },
        Duration::from_secs(5),
    )
    .await;
    assert!(started.is_err());
    assert!(!dir.join("authority/pid").exists());
}
