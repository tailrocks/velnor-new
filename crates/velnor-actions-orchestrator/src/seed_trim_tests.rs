//! Seed trim on a fixture tree: only clean registry units survive.

use std::fs;
use std::path::{Path, PathBuf};

use velnor_actions_contract::workflow::cache_family::{CacheFamily, SeedLane};

use super::units::unit_hash;
use super::{TrimInputs, check_no_compiler, trim};

const REG: &str = "registry+https://github.com/rust-lang/crates.io-index#reg-a@1.0.0";
const TAINTED: &str = "registry+https://github.com/rust-lang/crates.io-index#reg-b@1.0.0";
const PATCHED: &str = "path+file:///ws/vendor/p#1.0.0";
const MEMBER: &str = "path+file:///ws/app#0.1.0";
const KEEP: &str = "aaaaaaaaaaaaaaaa";
const KEEP_BUILD: &str = "a1a1a1a1a1a1a1a1";
const KEEP_RUN: &str = "a2a2a2a2a2a2a2a2";
const TAINT: &str = "bbbbbbbbbbbbbbbb";
const OWN: &str = "cccccccccccccccc";
const STALE: &str = "dddddddddddddddd";

fn touch(path: &Path, bytes: usize) {
    fs::create_dir_all(path.parent().expect("parent")).expect("dir");
    fs::write(path, vec![b'x'; bytes]).expect("file");
}

struct Fixture {
    _dir: tempfile::TempDir,
    workspace: PathBuf,
    seed: PathBuf,
}

fn write_profile(profile: &Path) {
    for hash in [KEEP, TAINT, OWN, STALE] {
        touch(&profile.join(format!("deps/libx-{hash}.rlib")), 10);
        touch(&profile.join(format!("deps/x-{hash}.d")), 1);
        touch(&profile.join(format!(".fingerprint/x-{hash}/lib-x")), 1);
    }
    touch(
        &profile.join(format!("deps/x-{KEEP}.x.0123-cgu.0.rcgu.o")),
        5,
    );
    touch(
        &profile.join(format!("build/reg-a-{KEEP_BUILD}/build-script-build")),
        7,
    );
    touch(
        &profile.join(format!("build/reg-a-{KEEP_RUN}/out/gen.rs")),
        3,
    );
    touch(
        &profile.join(format!(".fingerprint/reg-a-{KEEP_BUILD}/build")),
        1,
    );
    touch(&profile.join(format!("build/app-{OWN}/out/gen.rs")), 3);
    touch(&profile.join("incremental/app-x/s"), 100);
    touch(&profile.join("examples/demo"), 100);
    touch(&profile.join(".cargo-lock"), 0);
    touch(&profile.join("app"), 100);
}

fn write_lane(seed: &Path, profile: &Path) {
    let artifact = |id: &str, files: Vec<String>, fresh: bool| {
        serde_json::json!({"reason": "compiler-artifact", "package_id": id, "filenames": files, "fresh": fresh})
            .to_string()
    };
    let path = |rel: String| profile.join(rel).display().to_string();
    let lines = [
        "listing human output".to_owned(),
        artifact(REG, vec![path(format!("deps/libx-{KEEP}.rlib"))], true),
        artifact(
            REG,
            vec![path(format!("build/reg-a-{KEEP_BUILD}/build-script-build"))],
            false,
        ),
        serde_json::json!({"reason": "build-script-executed", "package_id": REG,
            "out_dir": path(format!("build/reg-a-{KEEP_RUN}/out"))})
        .to_string(),
        artifact(
            TAINTED,
            vec![path(format!("deps/libx-{TAINT}.rlib"))],
            false,
        ),
        artifact(
            MEMBER,
            vec![
                path(format!("deps/libx-{OWN}.rlib")),
                path("app".to_owned()),
            ],
            false,
        ),
        serde_json::json!({"reason": "build-finished", "success": true}).to_string(),
    ];
    touch(&seed.join("build/b3-x.jsonl"), 0);
    fs::write(seed.join("build/b3-x.jsonl"), lines.join("\n")).expect("jsonl");
    fs::write(seed.join("build/timing"), "1000 4500\n").expect("timing");
}

fn write_metadata(seed: &Path) {
    let metadata = serde_json::json!({
        "packages": [
            {"id": REG, "source": "registry+https://github.com/rust-lang/crates.io-index", "targets": []},
            {"id": TAINTED, "source": "registry+https://github.com/rust-lang/crates.io-index", "targets": []},
            {"id": PATCHED, "source": null, "targets": []},
            {"id": MEMBER, "source": null, "targets": []}
        ],
        "workspace_members": [MEMBER],
        "resolve": {"nodes": [
            {"id": MEMBER, "deps": [{"pkg": REG}, {"pkg": TAINTED}]},
            {"id": TAINTED, "deps": [{"pkg": PATCHED}]},
            {"id": REG, "deps": []},
            {"id": PATCHED, "deps": []}
        ]}
    });
    fs::write(seed.join("metadata.json"), metadata.to_string()).expect("metadata");
}

fn fixture() -> Fixture {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let workspace = dir.path().join("ws");
    let seed = dir.path().join("seed");
    let profile = workspace.join("target/debug");
    write_profile(&profile);
    write_lane(&seed, &profile);
    write_metadata(&seed);
    Fixture {
        _dir: dir,
        workspace,
        seed,
    }
}

fn names(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

#[test]
fn only_clean_registry_units_seen_this_run_survive() {
    let fixture = fixture();
    let inputs = TrimInputs {
        workspace: &fixture.workspace,
        seed_dir: &fixture.seed,
        root: "",
        lanes: &[SeedLane::Build, SeedLane::Clippy, SeedLane::Doc],
        ceiling: 1_000,
    };
    let (lanes, families) = trim(&inputs).expect("trim");
    let profile = fixture.workspace.join("target/debug");
    assert_eq!(names(&profile), [".fingerprint", "build", "deps"]);
    assert_eq!(
        names(&profile.join("deps")),
        [
            format!("libx-{KEEP}.rlib"),
            format!("x-{KEEP}.d"),
            format!("x-{KEEP}.x.0123-cgu.0.rcgu.o"),
        ]
    );
    assert_eq!(
        names(&profile.join("build")),
        [format!("reg-a-{KEEP_BUILD}"), format!("reg-a-{KEEP_RUN}")]
    );
    assert_eq!(
        names(&profile.join(".fingerprint")),
        [format!("reg-a-{KEEP_BUILD}"), format!("x-{KEEP}")]
    );
    let build = &lanes[&SeedLane::Build];
    assert!(build.ran);
    assert_eq!((build.kept_units, build.dropped_units), (3, 2));
    assert_eq!((build.fresh, build.compiled), (1, 1));
    assert_eq!(build.millis, Some(3500));
    assert!(!lanes[&SeedLane::Clippy].ran);
    let rsdeps = &families[&CacheFamily::Rsdeps];
    assert!(
        !rsdeps.complete && !rsdeps.within_ceiling,
        "two lanes never ran"
    );
    assert_eq!(rsdeps.raw_bytes, 10 + 1 + 1 + 5 + 7 + 3 + 1);
}

#[test]
fn a_complete_family_is_within_the_ceiling_only_at_or_below_it() {
    let fixture = fixture();
    for lane in ["clippy", "doc"] {
        fs::create_dir_all(fixture.seed.join(lane)).expect("lane dir");
    }
    for (ceiling, within) in [(28, true), (27, false)] {
        let inputs = TrimInputs {
            workspace: &fixture.workspace,
            seed_dir: &fixture.seed,
            root: "",
            lanes: &[SeedLane::Build, SeedLane::Clippy, SeedLane::Doc],
            ceiling,
        };
        let (_, families) = trim(&inputs).expect("trim");
        let rsdeps = &families[&CacheFamily::Rsdeps];
        assert!(rsdeps.complete);
        assert_eq!(rsdeps.within_ceiling, within, "ceiling {ceiling}");
        assert!(fixture.workspace.join("target/velnor-doc/debug").is_dir());
    }
}

#[test]
fn unit_hashes_come_from_the_last_hex_segment() {
    assert_eq!(
        unit_hash("libserde-2ed16da61a4898e6.rlib").as_deref(),
        Some("2ed16da61a4898e6")
    );
    assert_eq!(
        unit_hash("proc-macro2-48c25d3e2615c431").as_deref(),
        Some("48c25d3e2615c431")
    );
    assert_eq!(
        unit_hash("serde-2ed16da61a4898e6.serde.391ae21ee3dea154-cgu.0.rcgu.o").as_deref(),
        Some("2ed16da61a4898e6")
    );
    assert_eq!(unit_hash("libfp.rlib"), None);
    assert_eq!(unit_hash("x-2ED16DA61A4898E6.rlib"), None);
}

#[test]
fn a_live_compiler_blocks_the_trim() {
    let proc_root = tempfile::TempDir::new().expect("proc");
    touch(&proc_root.path().join("12/comm"), 0);
    fs::write(proc_root.path().join("12/comm"), "bash\n").expect("comm");
    touch(&proc_root.path().join("self/comm"), 0);
    check_no_compiler(proc_root.path()).expect("no compiler");
    fs::create_dir_all(proc_root.path().join("13")).expect("pid");
    fs::write(proc_root.path().join("13/comm"), "rustc\n").expect("comm");
    let err = check_no_compiler(proc_root.path()).expect_err("rustc alive");
    assert!(
        err.to_string()
            .contains("seed_compiler_still_running:rustc"),
        "{err}"
    );
    assert!(check_no_compiler(&proc_root.path().join("missing")).is_err());
}
