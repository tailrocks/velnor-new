use super::*;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use velnor_actions_contract::StepKind;

const SENTINEL: &str = "#!/bin/sh\nprintf executed > \"$VELNOR_SENTINEL\"\n";
static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Sandbox(PathBuf);
impl Sandbox {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "velnor-mise-preflight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("create sandbox");
        Self(root.canonicalize().expect("canonical sandbox"))
    }

    fn binary(&self) -> PathBuf {
        self.0.join("velnor/mise/bin/mise")
    }

    fn install(&self) {
        let binary = self.binary();
        fs::create_dir_all(binary.parent().expect("parent")).expect("binary directory");
        fs::write(&binary, SENTINEL).expect("sentinel");
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).expect("executable");
    }

    fn run(&self, sha: &str) -> Output {
        Command::new("sh")
            .args(["-c", SCRIPT])
            .env(TEMP_ENV, &self.0)
            .env("VELNOR_MISE_CACHE_DOMAIN", "tools")
            .env(SHA_ENV, sha)
            .env("VELNOR_SENTINEL", self.0.join("executed"))
            .output()
            .expect("run preflight")
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("sandbox cleanup failed: {error}");
        }
    }
}

fn digest(binary: &Path) -> String {
    let output = if Path::new("/usr/bin/sha256sum").exists() {
        Command::new("/usr/bin/sha256sum").arg(binary).output()
    } else {
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(binary)
            .output()
    }
    .expect("hash binary");
    assert!(output.status.success());
    String::from_utf8(output.stdout)
        .expect("hash text")
        .split_whitespace()
        .next()
        .expect("digest")
        .to_owned()
}

#[test]
fn valid_binary_retained_without_execution() {
    let sandbox = Sandbox::new();
    sandbox.install();
    assert!(sandbox.run(&digest(&sandbox.binary())).status.success());
    assert_eq!(
        fs::read_to_string(sandbox.binary()).expect("binary"),
        SENTINEL
    );
    assert!(!sandbox.0.join("executed").exists());
}

#[test]
fn corrupt_binary_removed_without_execution_or_root_cleanup() {
    let sandbox = Sandbox::new();
    sandbox.install();
    let sibling = sandbox.0.join("velnor/mise/keep");
    fs::write(&sibling, "preserved").expect("sibling");
    assert!(sandbox.run(&"0".repeat(64)).status.success());
    assert!(!sandbox.binary().exists());
    assert_eq!(fs::read_to_string(sibling).expect("sibling"), "preserved");
    assert!(!sandbox.0.join("executed").exists());
}

#[test]
fn absent_binary_allows_cold_installation() {
    let sandbox = Sandbox::new();
    assert!(sandbox.run(&"0".repeat(64)).status.success());
    assert!(!sandbox.0.join("velnor").exists());
    fs::create_dir_all(sandbox.binary().parent().expect("parent")).expect("directories");
    assert!(sandbox.run(&"0".repeat(64)).status.success());
}

#[test]
fn owned_ancestor_symlinks_fail_closed() {
    for relative in ["velnor", "velnor/mise", "velnor/mise/bin"] {
        let sandbox = Sandbox::new();
        let outside = sandbox.0.join("outside");
        fs::create_dir(&outside).expect("outside directory");
        let marker = outside.join("preserved");
        fs::write(&marker, SENTINEL).expect("outside marker");
        let link = sandbox.0.join(relative);
        fs::create_dir_all(link.parent().expect("parent")).expect("link parent");
        symlink(
            if relative.ends_with("/mise") {
                &marker
            } else {
                &outside
            },
            &link,
        )
        .expect("symlink");
        assert!(!sandbox.run(&"0".repeat(64)).status.success(), "{relative}");
        assert!(
            fs::symlink_metadata(link)
                .expect("link remains")
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(marker).expect("outside marker"),
            SENTINEL
        );
        assert!(!sandbox.0.join("executed").exists());
    }
}

#[test]
fn binary_symlink_unlinked_without_touching_target() {
    let sandbox = Sandbox::new();
    fs::create_dir_all(sandbox.binary().parent().expect("parent")).expect("directories");
    let outside = sandbox.0.join("outside");
    fs::write(&outside, SENTINEL).expect("outside sentinel");
    symlink(&outside, sandbox.binary()).expect("binary link");
    assert!(sandbox.run(&"0".repeat(64)).status.success());
    assert!(fs::symlink_metadata(sandbox.binary()).is_err());
    assert_eq!(fs::read_to_string(outside).expect("target"), SENTINEL);
    assert!(!sandbox.0.join("executed").exists());
}

#[test]
fn nonexecutable_regular_leaf_repairs_but_directory_leaf_fails_closed() {
    let sandbox = Sandbox::new();
    sandbox.install();
    let sha = digest(&sandbox.binary());
    fs::set_permissions(sandbox.binary(), fs::Permissions::from_mode(0o644))
        .expect("nonexecutable");
    assert!(sandbox.run(&sha).status.success());
    assert!(!sandbox.binary().exists());
    fs::create_dir(sandbox.binary()).expect("directory leaf");
    let child = sandbox.binary().join("keep");
    fs::write(&child, "preserved").expect("child");
    assert!(!sandbox.run(&sha).status.success());
    assert_eq!(
        fs::read_to_string(child).expect("child retained"),
        "preserved"
    );
}

#[test]
fn fixed_script_and_digest_controls_reject_tampering() {
    let argv = vec!["sh".to_owned(), "-c".to_owned(), SCRIPT.to_owned()];
    assert!(is_preflight_argv(&argv));
    let mut scrubbed = argv.clone();
    scrubbed[2] = crate::toolchain_env::with_credential_unset_script(SCRIPT);
    assert!(is_preflight_argv(&scrubbed));
    scrubbed[2] = format!("unset PATH; {SCRIPT}");
    assert!(!is_preflight_argv(&scrubbed));
    let mut changed = argv;
    changed[2].push_str("exit 0");
    assert!(!is_preflight_argv(&changed));
    for sha in ["0".repeat(64), "a".repeat(64)] {
        let env = BTreeMap::from([
            (TEMP_ENV.to_owned(), RUNNER_TEMP.to_owned()),
            ("VELNOR_MISE_CACHE_DOMAIN".to_owned(), "tools".to_owned()),
            (SHA_ENV.to_owned(), sha),
        ]);
        assert!(validate_preflight_env(&env).is_ok());
    }
    for sha in ["0".repeat(63), "A".repeat(64), "$(touch marker)".to_owned()] {
        let env = BTreeMap::from([
            (TEMP_ENV.to_owned(), RUNNER_TEMP.to_owned()),
            ("VELNOR_MISE_CACHE_DOMAIN".to_owned(), "tools".to_owned()),
            (SHA_ENV.to_owned(), sha),
        ]);
        assert!(validate_preflight_env(&env).is_err());
    }
    let env = BTreeMap::from([
        (TEMP_ENV.to_owned(), "/tmp/foreign".to_owned()),
        (SHA_ENV.to_owned(), "0".repeat(64)),
    ]);
    assert!(validate_preflight_env(&env).is_err());
}

#[test]
fn preflight_binds_qualified_binary_digest_and_closed_root() {
    let setup = crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64));
    let domain = ToolCacheDomain::Full;
    let pin = setup.bootstrap(domain, "ubuntu-26.04").expect("bootstrap");
    let step = preflight_step(pin, domain).expect("preflight");
    let StepKind::Shell { run, env } = step.kind else {
        panic!("shell step");
    };
    assert!(is_preflight_argv(&run));
    assert_eq!(env.get(SHA_ENV), Some(&pin.binary_sha256));
    let mut changed = pin.clone();
    changed.binary_sha256 = "A".repeat(64);
    assert!(preflight_step(&changed, domain).is_err());
    changed.binary_sha256 = "b".repeat(64);
    assert!(preflight_step(&changed, domain).is_err());
    let mut changed = pin.clone();
    let mut env = changed.helper.environment().clone();
    env.insert("MISE_DATA_DIR".to_owned(), "/tmp/foreign".to_owned());
    changed.helper = changed.helper.with_environment(env);
    assert!(preflight_step(&changed, domain).is_err());
}
