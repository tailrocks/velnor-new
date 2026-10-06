//! Closed post-merge apply configuration is emitted as a separate workflow.

use std::fs;

use velnor_actions_actionlint::actions::PinnedActionRef;
use velnor_actions_contract::TOFU_APPLY_WORKFLOW_PATH;
use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{AWS_CREDENTIALS_USES, AWS_CREDENTIALS_VERSION};

use crate::impl_common::{TestResult, make_repo, plan_for};

const CONFIG: &str = "schema = 1\n\
[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\n\
[workflow.tofu_apply]\nroot = \"stacks/a\"\nenvironment = \"production\"\n\
role_arn = \"arn:aws:iam::123456789012:role/velnor-tofu\"\n\
[workflow.tofu_apply.backend]\nbucket = \"example-tofu-state\"\n\
key = \"chainargos/control-plane.tfstate\"\nregion = \"us-east-1\"\n\
[[workflow.tofu_apply.github_tokens]]\norganization = \"chainargos\"\n\
secret_name = \"GH_TOKEN_CHAINARGOS\"\n\
[[workflow.tofu_apply.github_tokens]]\norganization = \"tailrocks\"\n\
secret_name = \"GH_TOKEN_TAILROCKS\"\n\
[stacks.tofu]\nroots = [\"stacks/a\"]\n";

#[test]
fn generator_emits_configured_apply_workflow_and_plan_lists_it() -> TestResult {
    let repo = make_repo(CONFIG)?;
    fs::create_dir_all(repo.path().join("stacks/a"))?;
    fs::write(
        repo.path().join("stacks/a/main.tf"),
        "variable \"github_tokens\" {\n  type = map(string)\n  sensitive = true\n  default = {}\n}\n",
    )?;
    let prep = prepare(repo.path())?;
    let tree = render_staged_tree(&prep)?;
    let yaml = tree
        .get(TOFU_APPLY_WORKFLOW_PATH)
        .ok_or_else(|| std::io::Error::other("configured apply workflow missing"))?;
    assert!(yaml.contains("branches:\n      - testmain"), "{yaml}");
    assert!(yaml.contains("environment: production"), "{yaml}");
    assert!(yaml.contains("id-token: write"), "{yaml}");
    assert!(yaml.contains("use_lockfile=true"), "{yaml}");
    let pin = PinnedActionRef::aws_credentials();
    assert_eq!(AWS_CREDENTIALS_USES, format!("{}@{}", pin.repo, pin.sha));
    assert_eq!(AWS_CREDENTIALS_VERSION, pin.version_comment);
    let plan = plan_for(&prep)?;
    assert!(plan.contains(TOFU_APPLY_WORKFLOW_PATH), "{plan}");
    Ok(())
}

#[test]
fn unknown_apply_configuration_fields_fail_during_config_load() -> TestResult {
    let config = CONFIG.replace(
        "bucket = \"example-tofu-state\"",
        "bucket = \"example-tofu-state\"\nextra = \"not accepted\"",
    );
    let repo = make_repo(&config)?;
    fs::create_dir_all(repo.path().join("stacks/a"))?;
    fs::write(repo.path().join("stacks/a/main.tf"), "variable \"x\" {}\n")?;
    let error = prepare(repo.path()).expect_err("unknown typed field must be denied");
    assert!(
        error.to_string().contains("extra: unknown_config_field"),
        "{error}"
    );
    Ok(())
}
