// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GitHubRepository, REDIRECT_LIMIT, agent, api_origin, api_request, checked_storage_uri,
    storage_request_headers,
};
use crate::{OrchestratorError, internal::internal};
use std::{io::Read, time::Instant};

const LOG_LIMIT: u64 = 8 * 1024 * 1024;
const ARTIFACT_LIMIT: u64 = velnor_actions_contract::MAX_GITHUB_EVIDENCE_ARCHIVE_BYTES;

pub(crate) fn qualification_job_log(
    repository: &str,
    job_id: u64,
    token: &str,
    deadline: Instant,
) -> Result<Vec<u8>, OrchestratorError> {
    let repo = GitHubRepository::parse(repository)?;
    if job_id == 0 {
        return Err(internal("qualification_log_job_id"));
    }
    let url = format!(
        "{}/repos/{}/{}/actions/jobs/{job_id}/logs",
        api_origin(),
        repo.owner,
        repo.name
    );
    fetch_job_log(&url, token, deadline, checked_storage_uri)
}

fn fetch_job_log(
    url: &str,
    token: &str,
    deadline: Instant,
    validate: impl Fn(&str) -> Result<String, OrchestratorError>,
) -> Result<Vec<u8>, OrchestratorError> {
    let response = api_request(url, token, deadline)?;
    if response.status().as_u16() != 302 {
        return Err(internal("qualification_log_redirect"));
    }
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| internal("qualification_log_location"))?;
    let mut current = validate(location)?;
    for redirects in 0..=REDIRECT_LIMIT {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(internal("qualification_log_deadline"));
        }
        let mut request = agent()?.get(&current);
        for (name, value) in storage_request_headers() {
            request = request.header(name, &value);
        }
        let response = request
            .config()
            .timeout_global(Some(remaining))
            .max_redirects(0)
            .http_status_as_error(false)
            .build()
            .call()
            .map_err(|_| internal("qualification_log_transport"))?;
        if response.status().is_redirection() {
            let location = response
                .headers()
                .get("location")
                .and_then(|value| value.to_str().ok())
                .ok_or_else(|| internal("qualification_log_location"))?;
            if redirects >= REDIRECT_LIMIT {
                return Err(internal("qualification_log_redirect_limit"));
            }
            current = validate(location)?;
            continue;
        }
        if response.status().as_u16() != 200 || !valid_headers(response.headers()) {
            return Err(internal("qualification_log_response"));
        }
        let mut bytes = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(LOG_LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| internal("qualification_log_read"))?;
        if bytes.is_empty() || bytes.len() as u64 > LOG_LIMIT || Instant::now() >= deadline {
            return Err(internal("qualification_log_size"));
        }
        return Ok(bytes);
    }
    Err(internal("qualification_log_redirect_limit"))
}

pub(crate) fn qualification_artifact_bytes(
    repository: &str,
    artifact_id: u64,
    expected_size: u64,
    expected_sha256: &str,
    token: &str,
    deadline: Instant,
) -> Result<Vec<u8>, OrchestratorError> {
    use super::{ArtifactArchiveRequest, GitHubEvidenceRead, NativeGitHubEvidenceRead};
    if artifact_id == 0 || expected_size == 0 || expected_size > ARTIFACT_LIMIT {
        return Err(internal("qualification_artifact_bound"));
    }
    let repo = GitHubRepository::parse(repository)?;
    let dir = tempfile::tempdir().map_err(|_| internal("qualification_artifact_temp"))?;
    let path = dir.path().join("artifact.zip");
    NativeGitHubEvidenceRead.archive(ArtifactArchiveRequest {
        repo: &repo,
        artifact_id,
        token,
        expected_size,
        expected_sha256,
        dest: &path,
        deadline,
    })?;
    let bytes = std::fs::read(path).map_err(|_| internal("qualification_artifact_read"))?;
    if bytes.len() as u64 != expected_size {
        return Err(internal("qualification_artifact_size"));
    }
    Ok(bytes)
}

fn valid_headers(headers: &ureq::http::HeaderMap) -> bool {
    let mut encodings = headers.get_all("content-encoding").iter();
    match (encodings.next(), encodings.next()) {
        (None, None) => {}
        (Some(value), None)
            if value
                .to_str()
                .is_ok_and(|s| s.eq_ignore_ascii_case("identity")) => {}
        _ => return false,
    }
    let mut lengths = headers.get_all("content-length").iter();
    match (lengths.next(), lengths.next()) {
        (None, None) => true,
        (Some(value), None) => value
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .is_some_and(|size| size <= LOG_LIMIT),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{fetch_job_log, valid_headers};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
        time::{Duration, Instant},
    };

    fn server(responses: Vec<String>) -> (String, thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("listen");
        let address = listener.local_addr().expect("address");
        let origin = format!("http://{address}");
        let worker = thread::spawn(move || {
            let mut requests = Vec::new();
            for response in responses {
                let (mut stream, _) = listener.accept().expect("accept");
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .expect("timeout");
                let mut request = Vec::new();
                let mut buffer = [0_u8; 4096];
                loop {
                    let read = stream.read(&mut buffer).expect("read");
                    if read == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..read]);
                    if request.windows(4).any(|part| part == b"\r\n\r\n") {
                        break;
                    }
                }
                requests.push(String::from_utf8_lossy(&request).into_owned());
                let response = response.replace("__ORIGIN__", &format!("http://{address}"));
                stream.write_all(response.as_bytes()).expect("write");
            }
            requests
        });
        (origin, worker)
    }

    fn response(status: &str, extra: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn fetch(origin: &str) -> Result<Vec<u8>, crate::OrchestratorError> {
        let base = origin.to_owned();
        fetch_job_log(
            &format!("{origin}/api"),
            "synthetic-test-credential",
            Instant::now() + Duration::from_secs(10),
            |location| {
                if location.starts_with(&format!("{base}/storage")) {
                    Ok(location.to_owned())
                } else {
                    Err(crate::internal::internal("test_redirect_rejected"))
                }
            },
        )
    }

    #[test]
    fn redirect_download_never_forwards_api_credential() -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![
            response("302 Found", "Location: __ORIGIN__/storage1\r\n", ""),
            response("302 Found", "Location: __ORIGIN__/storage2\r\n", ""),
            response("200 OK", "", "safe log"),
        ]);
        assert_eq!(fetch(&origin)?, b"safe log");
        let requests = worker.join().map_err(|_| "server")?;
        assert!(
            requests[0]
                .to_ascii_lowercase()
                .contains("authorization: bearer ")
        );
        assert!(
            requests[1..]
                .iter()
                .all(|request| !request.to_ascii_lowercase().contains("authorization:"))
        );
        Ok(())
    }

    #[test]
    fn rejects_hostile_redirect_and_invalid_storage_headers()
    -> Result<(), Box<dyn std::error::Error>> {
        let (origin, worker) = server(vec![response(
            "302 Found",
            "Location: https://attacker.invalid/log\r\n",
            "",
        )]);
        assert!(fetch(&origin).is_err());
        worker.join().map_err(|_| "server")?;
        let (origin, worker) = server(vec![
            response("302 Found", "Location: __ORIGIN__/storage\r\n", ""),
            response("200 OK", "Content-Encoding: gzip\r\n", "abc"),
        ]);
        assert!(fetch(&origin).is_err());
        worker.join().map_err(|_| "server")?;
        let mut headers = ureq::http::HeaderMap::new();
        headers.insert("content-length", "8388609".parse()?);
        assert!(!valid_headers(&headers));
        headers.clear();
        headers.append("content-encoding", "identity".parse()?);
        headers.append("content-encoding", "gzip".parse()?);
        assert!(!valid_headers(&headers));
        Ok(())
    }
    #[test]
    fn rejects_truncated_and_oversized_storage_bodies() -> Result<(), Box<dyn std::error::Error>> {
        for body in [
            "HTTP/1.1 200 OK\r\nContent-Length: 8388609\r\nConnection: close\r\n\r\n",
            "HTTP/1.1 200 OK\r\nContent-Length: 20\r\nConnection: close\r\n\r\nx",
        ] {
            let (origin, worker) = server(vec![
                response("302 Found", "Location: __ORIGIN__/storage\r\n", ""),
                body.to_owned(),
            ]);
            assert!(fetch(&origin).is_err());
            worker.join().map_err(|_| "server")?;
        }
        Ok(())
    }

    #[test]
    fn artifact_rejects_invalid_bounds_and_digest_before_network() {
        let deadline = Instant::now() + Duration::from_secs(5);
        assert!(
            super::qualification_artifact_bytes(
                "owner/repo",
                0,
                1,
                &"0".repeat(64),
                "fixture",
                deadline
            )
            .is_err()
        );
        assert!(
            super::qualification_artifact_bytes(
                "owner/repo",
                1,
                1,
                "not-sha256",
                "fixture",
                deadline
            )
            .is_err()
        );
        let accepted = super::qualification_artifact_bytes(
            "owner/repo",
            1,
            8_388_609,
            &"0".repeat(64),
            "",
            deadline,
        )
        .expect_err("missing token");
        assert!(format!("{accepted:?}").contains("github_evidence_token_missing"));
        let rejected = super::qualification_artifact_bytes(
            "owner/repo",
            1,
            super::ARTIFACT_LIMIT + 1,
            &"0".repeat(64),
            "",
            deadline,
        )
        .expect_err("oversized archive");
        assert!(format!("{rejected:?}").contains("qualification_artifact_bound"));
    }
    #[test]
    fn production_storage_allowlist_rejects_non_github_redirects() {
        use super::checked_storage_uri;
        assert!(
            checked_storage_uri(
                "https://productionresultssa0.blob.core.windows.net/container/blob?sig=fixture"
            )
            .is_ok()
        );
        for url in [
            "http://productionresultssa0.blob.core.windows.net/blob",
            "https://productionresultssa0.blob.core.windows.net.attacker.invalid/blob",
            "https://user@productionresultssa0.blob.core.windows.net/blob",
            "https://productionresultssa0.blob.core.windows.net:8443/blob",
        ] {
            assert!(checked_storage_uri(url).is_err(), "{url}");
        }
    }
}
