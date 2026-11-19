// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::io;
use std::path::Path;

use tokio::fs;

use super::probe::request;
use super::proxy::Proxy;

/// Copies each job's step log from the API stub into `<logs>/jobs/<run>-<job>.log` and returns
/// the jobs the stub had no log for.
pub async fn save(
    stub: &str,
    token: &str,
    repo: &str,
    run: u64,
    jobs: &[u64],
    logs: &Path,
) -> io::Result<Vec<u64>> {
    let proxy = Proxy::authorized(stub, token);
    let folder = logs.join("jobs");
    fs::create_dir_all(&folder).await?;
    let mut missing = Vec::new();
    for &job in jobs {
        let path = format!("/repos/{repo}/actions/jobs/{job}/logs");
        match request(&proxy, "GET", &path, "").await {
            Some(bytes) => fs::write(folder.join(format!("{run}-{job}.log")), bytes).await?,
            None => missing.push(job),
        }
    }
    Ok(missing)
}
