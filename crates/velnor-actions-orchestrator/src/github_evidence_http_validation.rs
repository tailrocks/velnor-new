// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) fn is_sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::super::{
        ArtifactArchiveRequest, GitHubEvidenceRead, GitHubRepository, NativeGitHubEvidenceRead,
    };
    use std::time::{Duration, Instant};

    #[test]
    fn archive_admits_sha256_and_rejects_other_digest_encodings()
    -> Result<(), Box<dyn std::error::Error>> {
        let repo = GitHubRepository::parse("owner/repo")?;
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("artifact.zip");
        let run = |digest: &str| {
            NativeGitHubEvidenceRead
                .archive(ArtifactArchiveRequest {
                    repo: &repo,
                    artifact_id: 1,
                    token: "",
                    expected_size: 1,
                    expected_sha256: digest,
                    dest: &path,
                    deadline: Instant::now() + Duration::from_secs(5),
                })
                .err()
                .map(|error| error.to_string())
                .unwrap_or_default()
        };
        assert!(run(&"0".repeat(64)).contains("github_evidence_token_missing"));
        assert!(run(&format!("b3-{}", "0".repeat(64))).contains("github_evidence_archive_bound"));
        assert!(run(&"A".repeat(64)).contains("github_evidence_archive_bound"));
        Ok(())
    }
    #[test]
    fn streaming_requires_exact_sha256_and_size() -> Result<(), Box<dyn std::error::Error>> {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        use std::thread;
        let temp = tempfile::tempdir()?;
        for (index, (size, digest, accepted)) in [
            (3, crate::generator::sha256_hex(b"abc"), true),
            (3, "0".repeat(64), false),
            (4, crate::generator::sha256_hex(b"abc"), false),
        ]
        .into_iter()
        .enumerate()
        {
            let listener = TcpListener::bind("127.0.0.1:0")?;
            let address = listener.local_addr()?;
            let worker = thread::spawn(move || -> std::io::Result<()> {
                let (mut stream, _) = listener.accept()?;
                let mut request = [0; 2048];
                let _ = stream.read(&mut request)?;
                stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 3\r\nConnection: close\r\n\r\nabc",
                )
            });
            let mut response = ureq::get(format!("http://{address}")).call()?;
            let outcome = super::super::stream_archive(
                &mut response,
                &temp.path().join(format!("{index}.zip")),
                size,
                &digest,
                Instant::now() + Duration::from_secs(5),
            );
            assert_eq!(outcome.is_ok(), accepted);
            worker.join().map_err(|_| "server panicked")??;
        }
        Ok(())
    }
}
