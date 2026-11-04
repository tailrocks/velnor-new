// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{
    cell::{Cell, RefCell},
    io::Cursor,
    time::{Duration, Instant},
};
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_tool_download::{
        ToolDownloadRequest, ToolDownloadResponse, ToolDownloadTransport, download_tool_artifact,
    },
};

struct Fake {
    location: RefCell<String>,
    bytes: RefCell<Vec<u8>>,
    storage_calls: Cell<u32>,
    status: Cell<u16>,
    redirects: Cell<u8>,
    encoding: RefCell<Option<String>>,
}
impl ToolDownloadTransport for Fake {
    fn github(
        &self,
        _: &str,
        _: u64,
        token: &str,
        _: Instant,
    ) -> Result<ToolDownloadResponse, OrchestratorError> {
        assert_eq!(token, "fixture-token");
        Ok(ToolDownloadResponse {
            status: 302,
            location: Some(self.location.borrow().clone()),
            content_length: None,
            content_encoding: None,
            body: Box::new(Cursor::new(Vec::new())),
        })
    }
    fn storage(&self, _: &str, _: Instant) -> Result<ToolDownloadResponse, OrchestratorError> {
        self.storage_calls.set(self.storage_calls.get() + 1);
        let bytes = self.bytes.borrow().clone();
        if self.redirects.get() > 0 {
            self.redirects.set(self.redirects.get() - 1);
            return Ok(ToolDownloadResponse {
                status: 302,
                location: Some("https://next.blob.core.windows.net/a".into()),
                content_length: None,
                content_encoding: None,
                body: Box::new(Cursor::new(Vec::new())),
            });
        }
        Ok(ToolDownloadResponse {
            status: self.status.get(),
            location: None,
            content_length: Some(bytes.len() as u64),
            content_encoding: self.encoding.borrow().clone(),
            body: Box::new(Cursor::new(bytes)),
        })
    }
}
#[test]
fn tool_download_checks_redirect_digest_length_and_cleans_failure()
-> Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let destination = temp.path().canonicalize()?.join("archive.zip");
    let fake = Fake {
        location: RefCell::new("https://example.blob.core.windows.net/artifact".into()),
        bytes: RefCell::new(b"abc".to_vec()),
        storage_calls: Cell::new(0),
        status: Cell::new(200),
        redirects: Cell::new(0),
        encoding: RefCell::new(None),
    };
    let request = ToolDownloadRequest {
        repository: "example/demo",
        id: 10,
        token: "fixture-token",
        expected_size: 3,
        expected_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        destination: &destination,
        deadline: Instant::now() + Duration::from_secs(5),
    };
    let mut verified = download_tool_artifact(&fake, &request)?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut verified, &mut bytes)?;
    assert_eq!(bytes, b"abc");
    assert!(!destination.exists());
    *fake.bytes.borrow_mut() = b"bad".to_vec();
    assert!(download_tool_artifact(&fake, &request).is_err());
    assert!(!destination.exists());
    let before = fake.storage_calls.get();
    for location in [
        "https://evil.example/artifact",
        "http://example.blob.core.windows.net/artifact",
        "https://example.blob.core.windows.net.evil.example/artifact",
        "https://example.blob.core.windows.net@evil.example/artifact",
        "https://example.blob.core.windows.net:444/artifact",
        "https://example.blob.core.windows.net:abc/artifact",
        "https://example.blob.core.windows.net/artifact#fragment",
    ] {
        *fake.location.borrow_mut() = location.into();
        assert!(download_tool_artifact(&fake, &request).is_err());
        assert_eq!(fake.storage_calls.get(), before, "accepted {location}");
        assert!(!destination.exists());
    }
    *fake.location.borrow_mut() = "https://example.blob.core.windows.net/artifact".into();
    *fake.bytes.borrow_mut() = b"abcd".to_vec();
    assert!(download_tool_artifact(&fake, &request).is_err());
    assert!(!destination.exists());
    *fake.bytes.borrow_mut() = b"abc".to_vec();
    fake.status.set(403);
    assert!(download_tool_artifact(&fake, &request).is_err());
    assert!(!destination.exists());
    fake.status.set(200);
    *fake.encoding.borrow_mut() = Some("gzip".into());
    assert!(download_tool_artifact(&fake, &request).is_err());
    assert!(!destination.exists());
    *fake.encoding.borrow_mut() = None;
    fake.redirects.set(2);
    let mut verified = download_tool_artifact(&fake, &request)?;
    let mut redirected = Vec::new();
    std::io::Read::read_to_end(&mut verified, &mut redirected)?;
    assert_eq!(redirected, b"abc");
    fake.redirects.set(4);
    assert!(download_tool_artifact(&fake, &request).is_err());
    assert!(!destination.exists());
    std::fs::write(&destination, b"existing")?;
    assert!(download_tool_artifact(&fake, &request).is_err());
    assert_eq!(std::fs::read(&destination)?, b"existing");
    let invalid_token = ToolDownloadRequest {
        token: "not-a-secret\0invalid",
        ..request
    };
    assert!(download_tool_artifact(&fake, &invalid_token).is_err());
    Ok(())
}

#[cfg(unix)]
#[test]
fn download_is_anchored_when_parent_is_swapped() -> Result<(), Box<dyn std::error::Error>> {
    use std::{fs, os::unix::fs::symlink, path::PathBuf};
    struct Swap {
        parent: PathBuf,
        moved: PathBuf,
        escape: PathBuf,
    }
    impl ToolDownloadTransport for Swap {
        fn github(
            &self,
            _: &str,
            _: u64,
            _: &str,
            _: Instant,
        ) -> Result<ToolDownloadResponse, OrchestratorError> {
            Ok(ToolDownloadResponse {
                status: 302,
                location: Some("https://example.blob.core.windows.net/artifact".into()),
                content_length: None,
                content_encoding: None,
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
        fn storage(&self, _: &str, _: Instant) -> Result<ToolDownloadResponse, OrchestratorError> {
            fs::rename(&self.parent, &self.moved).map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?;
            symlink(&self.escape, &self.parent).map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?;
            Ok(ToolDownloadResponse {
                status: 200,
                location: None,
                content_length: Some(3),
                content_encoding: None,
                body: Box::new(Cursor::new(b"abc".to_vec())),
            })
        }
    }
    let temp = tempfile::tempdir()?;
    let root = temp.path().canonicalize()?;
    let parent = root.join("owned");
    let moved = root.join("moved");
    let escape = root.join("escape");
    fs::create_dir(&parent)?;
    fs::create_dir(&escape)?;
    let destination = parent.join("archive.zip");
    let request = ToolDownloadRequest {
        repository: "example/demo",
        id: 10,
        token: "fixture-token",
        expected_size: 3,
        expected_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        destination: &destination,
        deadline: Instant::now() + Duration::from_secs(5),
    };
    let mut verified = download_tool_artifact(
        &Swap {
            parent,
            moved: moved.clone(),
            escape: escape.clone(),
        },
        &request,
    )?;
    assert!(!escape.join("archive.zip").exists());
    assert!(!moved.join("archive.zip").exists());
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut verified, &mut bytes)?;
    assert_eq!(bytes, b"abc");
    Ok(())
}

#[cfg(unix)]
#[test]
fn download_preserves_leaf_created_during_request() -> Result<(), Box<dyn std::error::Error>> {
    struct CreateLeaf(std::path::PathBuf);
    impl ToolDownloadTransport for CreateLeaf {
        fn github(
            &self,
            _: &str,
            _: u64,
            _: &str,
            _: Instant,
        ) -> Result<ToolDownloadResponse, OrchestratorError> {
            Ok(ToolDownloadResponse {
                status: 302,
                location: Some("https://example.blob.core.windows.net/artifact".into()),
                content_length: None,
                content_encoding: None,
                body: Box::new(Cursor::new(Vec::new())),
            })
        }
        fn storage(&self, _: &str, _: Instant) -> Result<ToolDownloadResponse, OrchestratorError> {
            std::fs::write(&self.0, b"other").map_err(|error| OrchestratorError::Contract {
                problem: error.to_string(),
            })?;
            Ok(ToolDownloadResponse {
                status: 200,
                location: None,
                content_length: Some(3),
                content_encoding: None,
                body: Box::new(Cursor::new(b"abc".to_vec())),
            })
        }
    }
    let temp = tempfile::tempdir()?;
    let destination = temp.path().canonicalize()?.join("archive.zip");
    let request = ToolDownloadRequest {
        repository: "example/demo",
        id: 10,
        token: "fixture-token",
        expected_size: 3,
        expected_sha256: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        destination: &destination,
        deadline: Instant::now() + Duration::from_secs(5),
    };
    assert!(download_tool_artifact(&CreateLeaf(destination.clone()), &request).is_err());
    assert_eq!(std::fs::read(&destination)?, b"other");
    Ok(())
}
