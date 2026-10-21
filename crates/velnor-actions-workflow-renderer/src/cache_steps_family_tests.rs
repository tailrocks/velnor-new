use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_steps::{
    family_key_expr, family_restore_keys_expr, family_save_condition,
};
use velnor_actions_contract::{StepKind, StepRole};

use super::*;

fn bun() -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::BunInstall,
        locator: None,
        snapshot: FamilySnapshot::Files(vec!["web/bun.lock".to_owned()]),
        compat: vec!["bun=${VELNOR_TOOL_BUN:-}".to_owned()],
    }
}

fn dbt() -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::DbtPackages,
        locator: Some("analytics".to_owned()),
        snapshot: FamilySnapshot::Files(vec!["analytics/package-lock.yml".to_owned()]),
        compat: Vec::new(),
    }
}

fn ansible() -> FamilyCachePlan {
    FamilyCachePlan {
        family: CacheFamily::AnsibleCollections,
        locator: None,
        snapshot: FamilySnapshot::Lines(vec!["community.general\t9.0.0\tabc".replace('\t', " ")]),
        compat: Vec::new(),
    }
}

fn run_of(step: &velnor_actions_contract::Step) -> &str {
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("shell step");
    };
    &run[2]
}

fn with_of(step: &velnor_actions_contract::Step) -> &std::collections::BTreeMap<String, String> {
    let StepKind::Action { with, .. } = &step.kind else {
        panic!("action step");
    };
    with
}

#[test]
fn restore_and_save_share_one_path_and_one_key_per_family() {
    for plan in [bun(), dbt(), ansible()] {
        let restore = family_restore_step(&plan).expect("restore");
        let save = family_save_step(&plan).expect("save");
        assert_eq!(restore.role, Some(StepRole::FamilyRestore));
        assert_eq!(save.role, Some(StepRole::FamilySave));
        assert_eq!(with_of(&restore).get("path"), with_of(&save).get("path"));
        assert_eq!(
            with_of(&restore).get("path").map(String::as_str),
            Some(
                plan.family
                    .spec()
                    .path_input(plan.locator.as_deref())
                    .as_str()
            )
        );
        assert_eq!(
            with_of(&restore).get("key").map(String::as_str),
            Some(family_key_expr(plan.family).as_str())
        );
        assert_eq!(
            with_of(&restore).get("restore-keys").map(String::as_str),
            Some(family_restore_keys_expr(plan.family).as_str())
        );
        assert!(!with_of(&save).contains_key("restore-keys"));
        assert_eq!(save.condition, Some(family_save_condition(plan.family)));
        assert!(
            restore.condition.is_none(),
            "restores are read-only and ungated"
        );
    }
}

#[test]
fn plans_reject_a_locator_mismatch_and_unsafe_inputs() {
    let mut shared_with_locator = bun();
    shared_with_locator.locator = Some("x".to_owned());
    assert!(family_restore_step(&shared_with_locator).is_err());
    let mut per_root_without = dbt();
    per_root_without.locator = None;
    assert!(family_restore_step(&per_root_without).is_err());
    let mut traversal = bun();
    traversal.snapshot = FamilySnapshot::Files(vec!["../bun.lock".to_owned()]);
    assert!(family_keys_step(&[traversal]).is_err());
    let mut injected = bun();
    injected.compat = vec!["x=$(evil)".to_owned()];
    assert!(family_keys_step(&[injected]).is_err());
    let mut sources = bun();
    sources.family = CacheFamily::Sources;
    assert!(family_save_step(&sources).is_err(), "only stack families");
    assert!(family_keys_step(&[]).is_err());
}

#[test]
fn the_key_step_is_trusted_push_only_and_names_no_job_or_run() {
    let step = family_keys_step(&[bun(), dbt()]).expect("keys");
    assert_eq!(step.id, Some(StepId::FamilyKeys));
    let script = run_of(&step);
    for banned in ["github.job", "github.run_", "hashFiles"] {
        assert!(!script.contains(banned), "{banned}");
    }
    assert!(script.contains("velnor-v1-bun-install-trusted-$compat-"));
    assert!(script.contains("velnor-v1-dbt-packages-trusted-$compat-"));
    assert!(script.contains("[ \"$trust\" != trusted ] || save=true"));
}

fn write_exec(path: &Path, body: &str) {
    std::fs::write(path, body).expect("stub");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
}

struct Sandbox(std::path::PathBuf);

impl Sandbox {
    fn new(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("velnor-family-{tag}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&root));
        for dir in ["bin", "runner/velnor", "ws/web", "ws/analytics"] {
            std::fs::create_dir_all(root.join(dir)).expect("dir");
        }
        write_exec(
            &root.join("bin/sha256sum"),
            "#!/bin/sh\nprintf '%064d  %s\\n' 5 \"$1\"\n",
        );
        Self(root)
    }

    fn run(&self, script: &str, env: &[(&str, &str)]) -> (String, String) {
        let out = self.0.join("out");
        std::fs::write(&out, "").expect("out");
        let path = format!(
            "{}:{}",
            self.0.join("bin").display(),
            std::env::var("PATH").unwrap_or_default()
        );
        let mut command = Command::new("sh");
        command
            .args(["-c", script])
            .env("PATH", path)
            .env("RUNNER_TEMP", self.0.join("runner"))
            .env("GITHUB_WORKSPACE", self.0.join("ws"))
            .env("GITHUB_OUTPUT", &out);
        for (key, value) in env {
            command.env(key, value);
        }
        let output = command.output().expect("runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        (
            String::from_utf8_lossy(&output.stdout).into_owned(),
            std::fs::read_to_string(&out).expect("outputs"),
        )
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[test]
fn keys_script_publishes_per_family_keys_and_grants_save_only_to_a_trusted_push() {
    let script = run_of(&family_keys_step(&[bun(), dbt(), ansible()]).expect("keys")).to_owned();
    for (event, protected, expected) in [
        ("push", "true", "save=true"),
        ("push", "false", "save=false"),
        ("pull_request", "false", "save=false"),
    ] {
        let sandbox = Sandbox::new(&format!("keys-{event}-{protected}"));
        std::fs::write(sandbox.0.join("ws/web/bun.lock"), "lock").expect("lock");
        let env = [
            ("VELNOR_DEFAULT_REF", "refs/heads/main"),
            ("GITHUB_EVENT_NAME", event),
            ("GITHUB_REF", "refs/heads/main"),
            ("GITHUB_REF_PROTECTED", protected),
            ("VELNOR_TOOL_BUN", "1.4.2"),
        ];
        let (_, outputs) = sandbox.run(&script, &env);
        assert!(outputs.contains(expected), "{outputs}");
        for stem in ["bun_install", "dbt_packages", "ansible_collections"] {
            assert!(
                outputs.contains(&format!("{stem}_key=velnor-v1-")),
                "{stem}: {outputs}"
            );
            assert!(
                outputs.contains(&format!("{stem}_restore_keys=velnor-v1-")),
                "{stem}"
            );
        }
    }
}

fn size_script(plan: &FamilyCachePlan) -> String {
    run_of(&family_size_step(plan).expect("size")).to_owned()
}

#[test]
fn the_size_step_admits_a_small_directory_and_refuses_a_missing_one() {
    let plan = bun();
    let sandbox = Sandbox::new("size-small");
    let (stdout, outputs) = sandbox.run(&size_script(&plan), &[]);
    assert!(stdout.contains("missing_directory"), "{stdout}");
    assert_eq!(outputs.trim(), "admitted=false");
    let dir = sandbox.0.join("runner/velnor/bun-cache");
    std::fs::create_dir_all(&dir).expect("cache dir");
    std::fs::write(dir.join("blob"), vec![7_u8; 4096]).expect("blob");
    let (_, outputs) = sandbox.run(&size_script(&plan), &[]);
    assert_eq!(outputs.trim(), "admitted=true");
}

#[test]
fn the_size_step_refuses_over_the_ceiling_with_or_without_zstd() {
    let plan = bun();
    let sandbox = Sandbox::new("size-big");
    let dir = sandbox.0.join("runner/velnor/bun-cache");
    std::fs::create_dir_all(&dir).expect("cache dir");
    // A stub zstd/wc pair reporting a stored size just over the ceiling.
    let over = (plan.family.spec().ceiling_bytes + 1).to_string();
    write_exec(&sandbox.0.join("bin/zstd"), "#!/bin/sh\ncat >/dev/null\n");
    write_exec(
        &sandbox.0.join("bin/wc"),
        &format!("#!/bin/sh\nprintf '%s\\n' {over}\n"),
    );
    let (stdout, outputs) = sandbox.run(&size_script(&plan), &[]);
    assert!(stdout.contains("over_ceiling"), "{stdout}");
    assert_eq!(outputs.trim(), "admitted=false");
}
