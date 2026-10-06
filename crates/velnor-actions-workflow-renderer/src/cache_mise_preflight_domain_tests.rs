use super::*;
use std::fs;
use std::os::unix::fs::symlink;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use velnor_actions_contract::StepKind;

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "velnor-tool-domain-preflight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        fs::create_dir(&root).expect("fixture");
        Self(root.canonicalize().expect("canonical root"))
    }
    fn run(&self, domain: ToolCacheDomain) -> bool {
        Command::new("sh")
            .args(["-c", SCRIPT])
            .env(TEMP_ENV, &self.0)
            .env(SHA_ENV, "0".repeat(64))
            .env("VELNOR_MISE_CACHE_DOMAIN", domain_name(domain))
            .status()
            .expect("preflight")
            .success()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).expect("cleanup");
    }
}

#[test]
fn all_owned_domain_ancestors_fail_closed_before_binary_inspection() {
    for domain in domains() {
        let relative = domain
            .root()
            .strip_prefix("${{ runner.temp }}/")
            .expect("owned");
        let components: Vec<_> = relative.split('/').chain(["bin"]).collect();
        for depth in 1..=components.len() {
            let fixture = Fixture::new();
            let outside = fixture.0.join("outside");
            fs::create_dir(&outside).expect("outside");
            fs::write(outside.join("mise"), "preserved").expect("marker");
            let link = fixture.0.join(components[..depth].join("/"));
            fs::create_dir_all(link.parent().expect("parent")).expect("parents");
            symlink(&outside, &link).expect("link");
            assert!(!fixture.run(domain), "{domain:?} {depth}");
            assert!(
                fs::symlink_metadata(link)
                    .expect("link remains")
                    .file_type()
                    .is_symlink()
            );
            assert_eq!(
                fs::read_to_string(outside.join("mise")).expect("marker"),
                "preserved"
            );
        }
    }
}

#[test]
fn bootstrap_preflight_accepts_only_exact_closed_domain_roots() {
    let setup = crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64));
    for domain in domains() {
        let pin = setup.bootstrap(domain, "ubuntu-26.04").expect("bootstrap");
        let step = preflight_step(pin, domain).expect("domain preflight");
        let StepKind::Shell { env, .. } = step.kind else {
            panic!("shell")
        };
        assert_eq!(
            env.get("VELNOR_MISE_CACHE_DOMAIN").map(String::as_str),
            Some(domain_name(domain))
        );
        assert!(validate_preflight_env(&env).is_ok());
        let mut changed = pin.clone();
        let mut env = changed.helper.environment().clone();
        env.insert(
            "MISE_DATA_DIR".to_owned(),
            format!("{}/foreign", domain.root()),
        );
        changed.helper = changed.helper.with_environment(env);
        assert!(preflight_step(&changed, domain).is_err());
        let wrong_domain = if domain == ToolCacheDomain::Full {
            ToolCacheDomain::Planning
        } else {
            ToolCacheDomain::Full
        };
        assert!(preflight_step(pin, wrong_domain).is_err());
    }
}

#[test]
fn acquisition_digest_does_not_activate_preflight_control_namespace() {
    let setup = crate::setup::fixture::mise_setup("2026.9.16", &"a".repeat(64));
    let pin = setup
        .bootstrap(ToolCacheDomain::Full, "ubuntu-26.04")
        .expect("bootstrap");
    assert!(validate_preflight_env(pin.helper.environment()).is_ok());
    let mut env = pin.helper.environment().clone();
    env.insert(SHA_ENV.to_owned(), pin.binary_sha256.clone());
    assert!(validate_preflight_env(&env).is_err());
}
