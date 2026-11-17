// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::GitHubApi;
use crate::homebrew::qualification_evidence;
use std::{
    ffi::OsString,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};

fn write_evidence(root: &Path, accept: &str, route: &str, bytes: &[u8]) -> std::path::PathBuf {
    let key = crate::generator::sha256_hex(format!("{accept}\n{route}").as_bytes());
    let path = root.join(key);
    std::fs::write(&path, bytes).expect("write raw response");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).expect("private file");
    path
}

#[test]
fn homebrew_offline_reads_bind_route_accept_and_fail_closed_without_network() {
    if std::env::var_os("VELNOR_HOME_EVIDENCE_TEST_CHILD").is_none() {
        let tools = tempfile::tempdir().expect("external tool fixture");
        let marker = tools.path().join("gh-was-executed");
        let mise = tools.path().join("mise");
        let quoted = marker.display().to_string().replace('\'', "'\\''");
        let script = format!("#!/bin/sh\nprintf invoked > '{quoted}'\nprintf '{{}}'\n");
        std::fs::write(&mise, script).expect("external gh marker shim");
        std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o700))
            .expect("executable shim");
        let test_name = format!(
            "{}::homebrew_offline_reads_bind_route_accept_and_fail_closed_without_network",
            module_path!()
        );
        let (_, test_name) = test_name.split_once("::").expect("crate-qualified test");
        let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", test_name, "--nocapture"])
            .env("VELNOR_HOME_EVIDENCE_TEST_CHILD", "1")
            .env("PATH", tools.path())
            .output()
            .expect("run isolated evidence reader");
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !marker.exists(),
            "offline reads must never invoke external gh transport"
        );
        return;
    }
    let root = tempfile::tempdir().expect("evidence directory");
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))
        .expect("private directory");
    let accept = "application/vnd.github+json";
    let route = "repos/jackin-project/jackin/branches/main";
    let bytes = br#"{"name":"main","protected":true}"#;
    let path = write_evidence(root.path(), accept, route, bytes);
    let api = GitHubApi::new("jackin-project/jackin".to_owned());
    assert!(!qualification_evidence::active());
    qualification_evidence::scope(root.path(), || {
        assert_eq!(api.get_json_bytes(route)?, bytes);
        let missing = "repos/jackin-project/jackin/branches/missing";
        let failure = api
            .get_json_bytes(missing)
            .expect_err("missing raw evidence fails closed");
        assert!(failure.to_string().contains("homebrew_evidence_file"));
        let failure = api
            .request_accept(
                vec![OsString::from(route)],
                1024,
                "application/octet-stream",
            )
            .expect_err("wrong Accept cannot alias JSON evidence");
        assert!(failure.to_string().contains("homebrew_evidence_file"));
        assert!(
            api.request_accept(vec![OsString::from(route)], bytes.len() - 1, accept)
                .is_err()
        );
        assert!(
            api.get_json_bytes("repos/foreign/project/branches/main")
                .is_err()
        );
        std::fs::remove_file(&path).expect("remove regular response");
        symlink(root.path().join("foreign"), &path).expect("symlink raw response");
        assert!(api.get_json_bytes(route).is_err());
        std::fs::remove_file(&path).expect("remove symlink");
        let _ = write_evidence(root.path(), accept, route, bytes);
        assert_eq!(api.get_json_bytes(route)?, bytes);
        Ok(())
    })
    .expect("offline qualification scope");
    assert!(!qualification_evidence::active());
}
