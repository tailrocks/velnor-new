use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::*;
use crate::tool_seed_test_support::mock_trust_commands;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "velnor-tools-restore-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path.canonicalize().expect("canonical scratch")
}

fn expected_key() -> String {
    format!("mise-tools-v2-{}", "a".repeat(64))
}

fn ready_seed(seed: &Path) {
    fs::create_dir_all(seed.join("mise/tree/installs")).expect("mise seed tree");
    fs::create_dir_all(seed.join("rustup/tree/toolchains")).expect("rustup seed tree");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("seed provenance");
    fs::write(seed.join("mise/KEY"), expected_key()).expect("seed key");
    fs::write(seed.join("mise/tree/installs/marker"), "seed-mise").expect("Mise seed");
    fs::write(seed.join("rustup/tree/toolchains/marker"), "seed-rustup").expect("Rustup seed");
}

fn cached_paths(home: &Path, runner_temp: &Path, contents: &str) {
    let mise = home.join(".local/share/mise/installs");
    let rustup = runner_temp.join("velnor/rustup/toolchains");
    let cargo = runner_temp.join("velnor/cargo");
    fs::create_dir_all(&mise).expect("Mise cache path");
    fs::create_dir_all(&rustup).expect("Rustup cache path");
    fs::create_dir_all(cargo.join("bin")).expect("Cargo bin path");
    fs::write(mise.join("marker"), contents).expect("Mise cache marker");
    fs::write(mise.join("remote-only"), contents).expect("Mise remote-only marker");
    fs::write(rustup.join("marker"), contents).expect("Rustup cache marker");
    fs::write(rustup.join("remote-only"), contents).expect("Rustup remote-only marker");
    fs::write(cargo.join("bin/cargo"), contents).expect("Cargo cache binary");
    fs::write(cargo.join(".crates.toml"), contents).expect("Cargo metadata");
    fs::write(cargo.join(".crates2.json"), contents).expect("Cargo metadata v2");
}

fn run_admission(home: &Path, runner_temp: &Path, cache_hit: &str, matched_key: &str) -> Output {
    Command::new("bash")
        .arg("-c")
        .arg(TOOLS_CACHE_ADMISSION_SCRIPT)
        .env("HOME", home)
        .env("RUNNER_TEMP", runner_temp)
        .env("TOOLS_CACHE_HIT", cache_hit)
        .env("TOOLS_EXPECTED_KEY", expected_key())
        .env("TOOLS_MATCHED_KEY", matched_key)
        .output()
        .expect("run restore admission")
}

fn run_seed(root: &Path, seed: &Path, home: &Path, runner_temp: &Path) -> Output {
    let script =
        crate::tool_seed::tool_seed_action_script(seed.to_str().expect("seed path is UTF-8"))
            .expect("tool seed script");
    let script = mock_trust_commands(&script, root);
    let mount = format!("{} ext4 0:77 ro,nosuid,nodev", seed.display());
    Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", home)
        .env("RUNNER_TEMP", runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("SEED_KEY", expected_key())
        .env("SEED_TEST_ROOT", seed)
        .env("SEED_TEST_MOUNTS", mount)
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .output()
        .expect("run trusted seed import")
}

fn run_lifecycle(
    root: &Path,
    seed: &Path,
    home: &Path,
    cache_hit: &str,
    matched_key: &str,
) -> (Output, Option<Output>) {
    let runner_temp = home.join("runner-temp");
    let admission = run_admission(home, &runner_temp, cache_hit, matched_key);
    let import =
        if admission.status.success() && (cache_hit != "true" || matched_key != expected_key()) {
            Some(run_seed(root, seed, home, &runner_temp))
        } else {
            None
        };
    (admission, import)
}

#[test]
fn generated_restore_action_imports_seed_after_admission_on_nonexact_keys() {
    let file = action_file("0.1.0").expect("restore action");
    let admission = file
        .bytes
        .find("Discard tools bytes unless the exact restore key matched")
        .expect("admission step");
    let condition = file
        .bytes
        .find(SEED_IMPORT_CONDITION)
        .expect("seed admission condition");
    let seed = file
        .bytes
        .find(&format!("uses: {}", crate::tool_seed::TOOL_SEED_USES))
        .expect("seed action");
    assert!(admission < condition && condition < seed);
    assert!(file.bytes.contains("cache_key: ${{ inputs.key }}"));
}

fn assert_nonexact_restore_imports_seed(name: &str, matched_key: &str) {
    let root = scratch(name);
    let seed = root.join("seed");
    let home = root.join("home");
    let runner_temp = home.join("runner-temp");
    ready_seed(&seed);
    cached_paths(&home, &runner_temp, "remote-cache");

    let (admission, import) = run_lifecycle(&root, &seed, &home, "false", matched_key);
    assert!(admission.status.success(), "admission: {admission:?}");
    let import = import.expect("nonexact restore imports the matching seed");
    assert!(import.status.success(), "seed import: {import:?}");
    assert_eq!(
        fs::read_to_string(home.join(".local/share/mise/installs/marker"))
            .expect("seeded Mise marker"),
        "seed-mise"
    );
    assert_eq!(
        fs::read_to_string(runner_temp.join("velnor/rustup/toolchains/marker"))
            .expect("seeded Rustup marker"),
        "seed-rustup"
    );
    assert!(!home.join(".local/share/mise/installs/remote-only").exists());
    assert!(
        !runner_temp
            .join("velnor/rustup/toolchains/remote-only")
            .exists()
    );
    assert!(!runner_temp.join("velnor/cargo/bin").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates.toml").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates2.json").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn matching_seed_is_imported_after_an_ordinary_remote_miss() {
    assert_nonexact_restore_imports_seed("miss", "");
}

#[test]
fn matching_seed_replaces_a_partial_restore_without_remote_remnants() {
    let partial_key = format!("mise-tools-v2-{}", "b".repeat(64));
    assert_nonexact_restore_imports_seed("partial", &partial_key);
}

#[test]
fn exact_remote_hit_keeps_cache_bytes_and_skips_seed_import() {
    let root = scratch("exact-hit");
    let seed = root.join("seed");
    let home = root.join("home");
    let runner_temp = home.join("runner-temp");
    ready_seed(&seed);
    cached_paths(&home, &runner_temp, "exact-cache");

    let (admission, import) = run_lifecycle(&root, &seed, &home, "true", &expected_key());
    assert!(admission.status.success(), "admission: {admission:?}");
    assert!(import.is_none(), "exact cache hit skips seed import");
    assert_eq!(
        fs::read_to_string(home.join(".local/share/mise/installs/marker"))
            .expect("exact Mise cache marker"),
        "exact-cache"
    );
    assert_eq!(
        fs::read_to_string(runner_temp.join("velnor/rustup/toolchains/marker"))
            .expect("exact Rustup cache marker"),
        "exact-cache"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn missing_seed_leaves_nonexact_cache_paths_cold() {
    let root = scratch("missing-seed");
    let seed = root.join("seed");
    let home = root.join("home");
    let runner_temp = home.join("runner-temp");
    cached_paths(&home, &runner_temp, "remote-cache");

    let (admission, import) = run_lifecycle(&root, &seed, &home, "false", "");
    assert!(admission.status.success(), "admission: {admission:?}");
    let import = import.expect("miss runs the seed step");
    assert!(
        import.status.success(),
        "missing seed is a cold success: {import:?}"
    );
    assert!(String::from_utf8_lossy(&import.stdout).contains("tool seed absent"));
    assert!(!home.join(".local/share/mise").exists());
    assert!(!runner_temp.join("velnor/rustup").exists());
    assert!(!runner_temp.join("velnor/cargo/bin").exists());
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn failed_admission_does_not_run_the_seed_import() {
    let root = scratch("admission-failure");
    let seed = root.join("seed");
    let home = root.join("home");
    let runner_temp = home.join("runner-temp");
    let mise_parent = home.join(".local/share");
    let outside = root.join("outside");
    ready_seed(&seed);
    fs::create_dir_all(&mise_parent).expect("Mise parent");
    fs::create_dir_all(&outside).expect("outside target");
    fs::write(outside.join("sentinel"), "untouched").expect("sentinel");
    symlink(&outside, mise_parent.join("mise")).expect("Mise symlink");

    let (admission, import) = run_lifecycle(&root, &seed, &home, "false", "");
    assert!(!admission.status.success(), "symlink admission must fail");
    assert!(import.is_none(), "failed admission skips the seed step");
    assert_eq!(
        fs::read_to_string(outside.join("sentinel")).expect("sentinel remains"),
        "untouched"
    );
    assert!(!outside.join("installs/marker").exists());
    fs::remove_dir_all(root).expect("cleanup");
}
