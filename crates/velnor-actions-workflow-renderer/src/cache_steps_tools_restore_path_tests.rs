//! The restore composite runs hermetically: the restored Mise `bin/` and
//! `shims/` that Setup Mise put on `PATH` never supply a helper or the shell
//! to the steps that check them, and `bin/` holds only what Setup Mise writes.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::{INSTALLS_VERIFY_SCRIPT, TOOLS_CACHE_ADMISSION_SCRIPT};
use crate::{VERIFY_PATH, VERIFY_SHELL};

const SETUP_MISE: &str = "verified-mise-binary";

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "velnor-tools-restore-path-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path
}

fn sha256(bytes: &str) -> String {
    let dir = scratch("digest");
    let file = dir.join("bytes");
    fs::write(&file, bytes).expect("digest input");
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(format!(
            "PATH={VERIFY_PATH}; if command -v sha256sum >/dev/null; then sha256sum \"$1\"; else shasum -a 256 \"$1\"; fi"
        ))
        .arg("sh")
        .arg(&file)
        .output()
        .expect("sha256");
    fs::remove_dir_all(dir).expect("cleanup digest");
    String::from_utf8_lossy(&output.stdout)[..64].to_owned()
}

/// A Mise root as an exact cache hit restores it: the genuine binary, with
/// `bin/` and `shims/` on `PATH` ahead of the system directories.
fn restored(root: &Path) -> (PathBuf, PathBuf) {
    let home = root.join("home");
    let bin = home.join(".local/share/mise/bin");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(home.join(".local/share/mise/shims")).expect("shims");
    fs::create_dir_all(home.join(".local/share/mise/installs")).expect("installs");
    fs::write(bin.join("mise"), SETUP_MISE).expect("mise");
    fs::set_permissions(bin.join("mise"), fs::Permissions::from_mode(0o755)).expect("mode");
    (home, bin)
}

fn plant(dir: &Path, names: &[&str], marker: &Path) {
    for name in names {
        let path = dir.join(name);
        let body = format!(
            "#!/bin/sh\nprintf '%s\\n' {name} >> '{}'\nprintf '%s  %s\\n' \"$TOOLS_MISE_SHA256\" \"$1\"\n",
            marker.display()
        );
        fs::write(&path, body).expect("planted helper");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).expect("mode");
    }
}

fn admit(home: &Path, runner_temp: &Path, hit: bool) -> Output {
    let key = format!("mise-tools-v2-{}", "a".repeat(64));
    let mise = home.join(".local/share/mise");
    let inherited = std::env::var_os("PATH").expect("PATH");
    let mut paths = vec![mise.join("shims"), mise.join("bin")];
    paths.extend(std::env::split_paths(&inherited));
    let mut command = Command::new("/bin/bash");
    command
        .arg("-c")
        .arg(TOOLS_CACHE_ADMISSION_SCRIPT)
        .env("PATH", std::env::join_paths(paths).expect("PATH"))
        .env("HOME", home)
        .env("RUNNER_TEMP", runner_temp)
        .env("TOOLS_MISE_SHA256", sha256(SETUP_MISE))
        .env("TOOLS_EXPECTED_KEY", &key)
        .env("TOOLS_SEED_ADMITTED", "false");
    if hit {
        command
            .env("TOOLS_CACHE_HIT", "true")
            .env("TOOLS_MATCHED_KEY", &key);
    }
    command.output().expect("run cache admission")
}

fn log(output: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    )
}

#[test]
fn a_planted_sha256sum_in_the_restored_mise_bin_never_runs_and_is_rejected() {
    // The review's reproduction: with `PATH` as Setup Mise leaves it, the
    // keep step's `verify_mise` ran a cached `bin/sha256sum`, which could
    // print the pinned digest; only `bin/mise` itself was ever checked.
    for (name, hit) in [("hit", true), ("miss", false)] {
        let root = scratch(&format!("planted-{name}"));
        let (home, bin) = restored(&root);
        let marker = root.join("planted.log");
        plant(&bin, &["sha256sum"], &marker);
        plant(
            &home.join(".local/share/mise/shims"),
            &["sha256sum", "shasum", "find", "rm"],
            &marker,
        );
        let output = admit(&home, &root.join("runner-temp"), hit);
        let ran = fs::read_to_string(&marker).unwrap_or_default();
        assert!(ran.is_empty(), "{name}: a planted helper ran: {ran}");
        assert!(!output.status.success(), "{name}: {}", log(&output));
        assert!(
            log(&output).contains("sha256sum is not part of the Setup Mise install"),
            "{name}: {}",
            log(&output)
        );
        fs::remove_dir_all(root).expect("cleanup");
    }
}

#[test]
fn setup_mise_bin_holds_only_the_binary_and_its_version_record() {
    let root = scratch("bookkeeping");
    let (home, bin) = restored(&root);
    fs::write(bin.join("mise-version"), "2026.10.17").expect("version record");
    let output = admit(&home, &root.join("runner-temp"), true);
    assert!(output.status.success(), "{}", log(&output));

    fs::set_permissions(bin.join("mise-version"), fs::Permissions::from_mode(0o755)).expect("mode");
    let output = admit(&home, &root.join("runner-temp"), true);
    assert!(
        !output.status.success(),
        "executable record: {}",
        log(&output)
    );

    fs::remove_file(bin.join("mise-version")).expect("remove record");
    symlink("/bin/sh", bin.join(".hidden")).expect("link");
    let output = admit(&home, &root.join("runner-temp"), true);
    assert!(!output.status.success(), "hidden link: {}", log(&output));
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn every_restore_check_resets_path_and_runs_an_absolute_shell() {
    assert!(
        TOOLS_CACHE_ADMISSION_SCRIPT.starts_with(&format!("set -eu;\nPATH={VERIFY_PATH};")),
        "the admission resets PATH before any command"
    );
    assert!(
        INSTALLS_VERIFY_SCRIPT.lines().find(|line| !line.is_empty()
            && !line.starts_with('#')
            && *line != "set -euo pipefail")
            == Some(format!("PATH={VERIFY_PATH}").as_str()),
        "the re-verify resets PATH before any command"
    );
    let file = super::action_file("0.1.0", &[]).expect("restore action");
    let shell = format!("shell: {VERIFY_SHELL}");
    assert_eq!(file.bytes.matches(&shell).count(), 2, "{}", file.bytes);
    assert!(!file.bytes.contains("shell: bash"), "{}", file.bytes);
}
