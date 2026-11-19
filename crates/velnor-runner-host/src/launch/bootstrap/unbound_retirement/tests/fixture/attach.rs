// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use velnor_resource_probe::storage_init_contract::METADATA_ROOT_MODE;

use super::TestResult;

const HELPER_PREFIX: &str = "velnor-init-";

struct Directory {
    target: &'static str,
    mount: u64,
    parent: u64,
    device: u32,
    inode: u64,
    owner: (u32, u32, u32),
}

impl Directory {
    fn json(&self) -> String {
        format!(
            r#"{{"target":"{}","mount_id":{},"parent_mount_id":{},"device_major":0,"device_minor":{},"inode":{},"uid":{},"gid":{},"mode":{}}}"#,
            self.target,
            self.mount,
            self.parent,
            self.device,
            self.inode,
            self.owner.0,
            self.owner.1,
            self.owner.2
        )
    }
}

const fn directory(
    target: &'static str,
    (mount, parent, device): (u64, u64, u32),
    inode: u64,
    owner: (u32, u32, u32),
) -> Directory {
    Directory {
        target,
        mount,
        parent,
        device,
        inode,
        owner,
    }
}

pub(super) fn ready(container_name: &str) -> Option<String> {
    let operation = container_name.strip_prefix(HELPER_PREFIX)?;
    let meta = (12, 12, 3);
    let records = [
        directory("/velnor/init/home", (10, 1, 1), 1, (1000, 1000, 0o755)),
        directory("/velnor/init/work", (11, 1, 2), 2, (1000, 1000, 0o755)),
        directory(
            "/velnor/init/metadata",
            (12, 1, 3),
            3,
            (0, 0, METADATA_ROOT_MODE),
        ),
        directory(
            "/velnor/init/metadata/runtime-identity",
            meta,
            4,
            (0, 0, 0o555),
        ),
        directory("/velnor/init/metadata/job-facts", meta, 5, (0, 0, 0o555)),
        directory("/velnor/init/metadata/source-proof", meta, 6, (0, 0, 0o755)),
        directory(
            "/velnor/init/metadata/source-proof/control",
            meta,
            7,
            (0, 0, 0o555),
        ),
        directory(
            "/velnor/init/home/source-proof/inbox",
            (10, 10, 1),
            8,
            (1000, 1000, 0o700),
        ),
    ]
    .map(|record| record.json());
    let [home, work, metadata, identity, facts, proof, control, inbox] = records;
    Some(format!(
        r#"{{"schema":2,"operation_id":"{operation}","home":{home},"work":{work},"metadata":{metadata},"runtime_identity":{identity},"job_facts":{facts},"source_proof":{proof},"control":{control},"inbox":{inbox}}}"#
    ))
}

pub(super) async fn serve(mut stream: UnixStream, ready: Option<String>) -> TestResult<()> {
    stream
        .write_all(b"HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.raw-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n")
        .await?;
    let Some(ready) = ready else {
        return Ok(());
    };
    let mut request = String::new();
    BufReader::new(&mut stream).read_line(&mut request).await?;
    let mut frame = vec![1, 0, 0, 0];
    frame.extend_from_slice(&u32::try_from(ready.len() + 1)?.to_be_bytes());
    frame.extend_from_slice(ready.as_bytes());
    frame.push(b'\n');
    stream.write_all(&frame).await?;
    std::future::pending().await
}
