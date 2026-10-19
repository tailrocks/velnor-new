//! Dependency seed keys: executed in `sh` over a fixture repository.

use std::collections::BTreeMap;
use std::path::Path;

use velnor_actions_contract::StepKind;
use velnor_actions_contract::workflow::TASK_EXECUTION_MANIFEST_PATH;
use velnor_actions_contract::workflow::cache_family::{CacheFamily, trusted_root_prefix};
use velnor_actions_mise::ToolCatalog;

use super::super::{CacheKeyInputs, cache_keys_step};
use super::{SeedKeyInputs, nested_roots};

const MANIFEST: &str = r#"{"generator_version":"0.0.0","schema":1,"tasks":{"stack/rust/app/build/default":{"argv":["mise","--","cargo","nextest","list","--manifest-path","app/Cargo.toml"],"env":{"A":"1"},"execution_digest":"b3-1","task_digest":"b3-2"},"stack/rust/app/fmt/default":{"argv":["mise","--","cargo","fmt","--manifest-path","app/Cargo.toml"],"env":{},"execution_digest":"b3-3"},"stack/rust/nested/lib/clippy/default":{"argv":["mise","--","cargo","clippy","--manifest-path","nested/lib/Cargo.toml"],"env":{},"execution_digest":"b3-4"}}}"#;

fn inputs(helper: bool) -> CacheKeyInputs {
    let roots = vec![String::new(), "nested".to_owned()];
    CacheKeyInputs {
        fetch_roots: roots.clone(),
        tofu_roots: Vec::new(),
        seed: SeedKeyInputs {
            roots: roots.clone(),
            workspace_roots: roots,
            helper_build: helper.then(|| {
                [
                    "mise",
                    "exec",
                    "rust@1.99.1",
                    "--",
                    "mbx",
                    "build",
                    "--release",
                ]
                .map(str::to_owned)
                .to_vec()
            }),
        },
    }
}

/// A fixed scratch path removed on drop.
struct Fixed(std::path::PathBuf);

impl Fixed {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixed {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn git_init_and_add(dir: &Path) {
    let status = crate::git_fixture::sterile_command()
        .arg("init")
        .arg("-q")
        .arg(dir)
        .status()
        .expect("git init");
    assert!(status.success(), "git init");
    let status = crate::git_fixture::command(dir)
        .expect("git")
        .args(["add", "-A"])
        .status()
        .expect("git add");
    assert!(status.success(), "git add");
}

/// Write the fixture repository, edit it with `edit`, and run the step.
fn run(edit: impl Fn(&Path), env: &[(&str, &str)], helper: bool) -> BTreeMap<String, String> {
    // One fixed path per test process: compat binds the absolute workspace.
    let base = std::env::temp_dir().join(format!("velnor-seed-keys-{}", std::process::id()));
    std::fs::remove_dir_all(&base).ok();
    std::fs::create_dir_all(&base).expect("base");
    let dir = Fixed(base);
    let workspace = dir.path().join("ws");
    let write = |rel: &str, text: &str| {
        let path = workspace.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("file");
    };
    write("Cargo.lock", "root-lock");
    write("Cargo.toml", "[workspace]\n");
    write("app/Cargo.toml", "[package]\nname = \"app\"\n");
    write("nested/Cargo.lock", "nested-lock");
    write("nested/lib/Cargo.toml", "[package]\nname = \"lib\"\n");
    write(
        TASK_EXECUTION_MANIFEST_PATH,
        &format!("# marker\n{MANIFEST}\n"),
    );
    edit(&workspace);
    git_init_and_add(&workspace);
    let bin = dir.path().join("bin");
    std::fs::create_dir_all(&bin).expect("bin");
    let mise = bin.join("mise");
    std::fs::write(
        &mise,
        "#!/bin/sh\nprintf 'rustc %s\\n' \"${FAKE_RUSTC:-1.99.1}\"\n",
    )
    .expect("mise");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&mise, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    let step = cache_keys_step(&ToolCatalog::pinned(), &inputs(helper))
        .expect("builds")
        .expect("emitted");
    let StepKind::Shell { run, .. } = step.kind else {
        panic!("shell");
    };
    std::fs::create_dir_all(dir.path().join("tmp")).expect("tmp");
    let output = dir.path().join("out");
    std::fs::write(&output, "").expect("output");
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let status = std::process::Command::new("sh")
        .arg("-c")
        .arg(&run[2])
        .env("PATH", path)
        .env("RUNNER_TEMP", dir.path().join("tmp"))
        .env("GITHUB_WORKSPACE", &workspace)
        .env("GITHUB_OUTPUT", &output)
        .envs(env.iter().copied())
        .status()
        .expect("sh");
    assert!(status.success());
    std::fs::read_to_string(&output)
        .expect("outputs")
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(name, value)| (name.to_owned(), value.to_owned()))
        .collect()
}

#[test]
fn every_root_gets_a_trusted_root_scoped_key_and_prefix() {
    let out = run(|_| {}, &[("GITHUB_EVENT_NAME", "pull_request")], true);
    for (ordinal, root) in ["", "nested"].iter().enumerate() {
        let key = &out[&format!("rsdeps_key_{ordinal}")];
        let prefix = &out[&format!("rsdeps_restore_keys_{ordinal}")];
        assert!(
            prefix.starts_with(&trusted_root_prefix(CacheFamily::Rsdeps, root)),
            "{prefix}"
        );
        assert!(
            key.starts_with(prefix.as_str()) && key.len() == prefix.len() + 69,
            "{key}"
        );
        assert!(key.len() <= CacheFamily::Rsdeps.max_key_bytes());
    }
    assert_ne!(out["rsdeps_key_0"], out["rsdeps_key_1"]);
    let helper = &out["rsdeps_helper_key"];
    assert!(helper.starts_with(&out["rsdeps_helper_restore_keys"]));
    assert!(helper.starts_with(&trusted_root_prefix(CacheFamily::RsdepsHelper, "")));
}

fn edit_file(rel: &'static str, text: String) -> impl Fn(&Path) {
    move |ws: &Path| {
        std::fs::write(ws.join(rel), &text).expect("edit");
    }
}

fn manifest(marker: &str, body: &str) -> impl Fn(&Path) {
    edit_file(
        TASK_EXECUTION_MANIFEST_PATH,
        format!("# {marker}\n{body}\n"),
    )
}

#[test]
fn depset_follows_root_manifests_but_never_image_version_or_nested_roots() {
    let base = run(|_| {}, &[("ImageVersion", "20261001.1")], false);
    let image = run(|_| {}, &[("ImageVersion", "20261020.9")], false);
    assert_eq!(
        base["rsdeps_key_0"], image["rsdeps_key_0"],
        "ImageVersion never keys"
    );
    let manifest_edit = run(
        edit_file("app/Cargo.toml", "[package]\nname = \"b\"\n".to_owned()),
        &[],
        false,
    );
    assert_ne!(base["rsdeps_key_0"], manifest_edit["rsdeps_key_0"]);
    assert_eq!(
        base["rsdeps_restore_keys_0"], manifest_edit["rsdeps_restore_keys_0"],
        "compat unchanged"
    );
    assert_eq!(
        base["rsdeps_key_1"], manifest_edit["rsdeps_key_1"],
        "another root is unaffected"
    );
    let nested = run(
        edit_file("nested/lib/Cargo.toml", "x".to_owned()),
        &[],
        false,
    );
    assert_eq!(
        base["rsdeps_key_0"], nested["rsdeps_key_0"],
        "nested roots are excluded"
    );
    assert_ne!(base["rsdeps_key_1"], nested["rsdeps_key_1"]);
}

#[test]
fn depset_follows_lane_records_but_never_version_markers_or_other_kinds() {
    let base = run(|_| {}, &[], false);
    let record = MANIFEST.replace("\"A\":\"1\"", "\"A\":\"2\"");
    let lanes = run(manifest("marker", &record), &[], false);
    assert_ne!(
        base["rsdeps_key_0"], lanes["rsdeps_key_0"],
        "lane env keys the seed"
    );
    let marker = MANIFEST
        .replace("b3-1", "b3-9")
        .replace("\"0.0.0\"", "\"9.9.9\"");
    let versions = run(manifest("other", &marker), &[], false);
    assert_eq!(
        base["rsdeps_key_0"], versions["rsdeps_key_0"],
        "version markers never key"
    );
    let fmt_only = MANIFEST.replace(
        "\"fmt\",\"--manifest-path\"",
        "\"fmt\",\"--all\",\"--manifest-path\"",
    );
    let fmt = run(manifest("m", &fmt_only), &[], false);
    assert_eq!(
        base["rsdeps_key_0"], fmt["rsdeps_key_0"],
        "non-lane records never key"
    );
}

#[test]
fn compat_follows_the_toolchain_flags_image_os_and_never_trust() {
    let base = run(|_| {}, &[], false);
    for env in [
        ("FAKE_RUSTC", "1.99.2"),
        ("RUSTFLAGS", "-Cdebuginfo=0"),
        ("RUSTDOCFLAGS", "-Dwarnings"),
        ("ImageOS", "ubuntu26"),
    ] {
        let out = run(|_| {}, &[env], false);
        assert_ne!(
            base["rsdeps_restore_keys_0"], out["rsdeps_restore_keys_0"],
            "{env:?}"
        );
    }
    let push = run(
        |_| {},
        &[
            ("GITHUB_EVENT_NAME", "push"),
            ("GITHUB_REF", "refs/heads/main"),
            ("GITHUB_REF_PROTECTED", "true"),
            ("VELNOR_DEFAULT_REF", "refs/heads/main"),
        ],
        false,
    );
    assert_eq!(push["trust"], "trusted");
    assert_eq!(
        base["rsdeps_key_0"], push["rsdeps_key_0"],
        "seed keys are always trusted"
    );
}

#[test]
fn nested_roots_are_strict_descendants() {
    let roots = vec![
        String::new(),
        "a".to_owned(),
        "a/b".to_owned(),
        "ab".to_owned(),
    ];
    assert_eq!(nested_roots("", &roots), ["a", "a/b", "ab"]);
    assert_eq!(nested_roots("a", &roots), ["a/b"]);
    assert_eq!(nested_roots("ab", &roots), Vec::<String>::new());
}
