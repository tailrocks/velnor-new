//! End-to-end transport proof for the native OpenTofu producer helper.

#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::{Command, Stdio};

use tempfile::TempDir;
use velnor_actions_contract::{CompiledSourceHelper, SourceBoundOperation};
use velnor_actions_workflow_renderer::{Yaml, source_helper};

const NATIVE_LOCK: &str = include_str!("../tests/fixtures/tofu/random-3.6.3.native-lock.fixture");
const NATIVE_LOCK_SHA256: &str = "19946cd1801530076e8219210244fb8f503ca78d75b82dbd251fc6d021519c22";
const TRANSPORT: &str =
    include_str!("../../velnor-actions-workflow-renderer/src/source_helper_transport.py");

#[test]
fn native_producer_record_round_trips_through_rendered_transport() {
    let repository = fixture_repository().expect("fixture repository");
    let preparation = crate::prepare(repository.path()).expect("prepare");
    let record = provider_record(&preparation.workflow.context.source_helpers);
    let args = record.invocation().args();
    assert_eq!(
        record.invocation().descriptor().operation(),
        SourceBoundOperation::TofuProviderExport
    );
    assert_eq!(args.len(), 6);
    assert_eq!(args[4], "dir-");
    assert_eq!(args[0], "aarch64-apple-darwin");
    assert_eq!(args[3], "1.13.1");
    let distribution = velnor_actions_mise::ToolCatalog::pinned()
        .native_distribution(
            velnor_actions_mise::catalog::qualification::DistributionHost::MacosArm64,
            velnor_actions_mise::PinnedTool::Opentofu,
        )
        .expect("qualified producer distribution");
    distribution
        .required_install_plan()
        .expect("measured native install plan");
    assert_eq!(
        args[5],
        distribution
            .required_installed_binary_path()
            .expect("qualified installed binary path")
    );

    let lock = decode_octal(&args[1]);
    assert_eq!(lock, NATIVE_LOCK.as_bytes());
    assert_eq!(
        velnor_actions_contract::compiled_source_sha256(&lock),
        NATIVE_LOCK_SHA256
    );
    assert_eq!(decode_octal(&args[2]), synthetic_config().as_bytes());
    assert_eq!(
        velnor_actions_contract::compiled_source_sha256(record.source().as_bytes()),
        record.invocation().descriptor().source_sha256()
    );

    let provider_job = preparation
        .workflow
        .ir
        .jobs
        .values()
        .find(|job| job.source_producer.is_some())
        .expect("provider producer job");
    let step = provider_job
        .steps
        .iter()
        .find(|step| step.name == "Export verified OpenTofu providers")
        .expect("provider export step");
    let yaml = source_helper::source_helper_step_to_yaml(
        step,
        &preparation.workflow.context.source_helpers,
        env!("CARGO_PKG_VERSION"),
        &preparation.runner_label,
    )
    .expect("transport yaml");
    let (run, environment) = yaml_fields(yaml);
    assert!(run.chars().count() <= 21_000, "run size: {}", run.len());
    assert_transport_round_trip(record, &environment).expect("transport round trip");

    let mise = crate::pins::resolve_mise_setup(&preparation.config, &preparation.runner_label)
        .expect("mise setup");
    let rendered = velnor_actions_workflow_renderer::render_workflow_ir_strict(
        &preparation.workflow.ir,
        preparation.config.workflow.policy,
        preparation.workflow.support.as_ref(),
        &preparation.workflow.context,
        &mise,
    )
    .expect("strict render");
    assert!(rendered.contains("Export verified OpenTofu providers"));
}

#[test]
fn linux_producer_rejects_unmeasured_native_installation() {
    let descriptor = crate::tofu_cache_source::descriptor_from_lock("", NATIVE_LOCK)
        .expect("qualified public lock descriptor");
    assert!(
        crate::tofu_producer_source::compiled_helper(
            &descriptor,
            "x86_64-unknown-linux-gnu",
            &velnor_actions_mise::ToolCatalog::pinned(),
            "candidate",
            "output",
            env!("CARGO_PKG_VERSION"),
        )
        .is_err()
    );
}

fn provider_record(records: &[CompiledSourceHelper]) -> &CompiledSourceHelper {
    records
        .iter()
        .find(|record| {
            record.invocation().descriptor().operation() == SourceBoundOperation::TofuProviderExport
        })
        .expect("compiled tofu provider record")
}

fn synthetic_config() -> String {
    "terraform {\n  required_providers {\n    v0 = {\n      source = \"registry.opentofu.org/hashicorp/random\"\n      version = \"= 3.6.3\"\n    }\n  }\n}\n"
        .to_owned()
}

fn decode_octal(encoded: &str) -> Vec<u8> {
    assert_eq!(encoded.len() % 4, 0, "octal argument width");
    encoded
        .as_bytes()
        .chunks_exact(4)
        .map(|chunk| {
            assert_eq!(chunk[0], b'\\');
            assert!(chunk[1..].iter().all(|digit| (b'0'..=b'7').contains(digit)));
            let value = ((u16::from(chunk[1] - b'0')) << 6)
                | ((u16::from(chunk[2] - b'0')) << 3)
                | u16::from(chunk[3] - b'0');
            u8::try_from(value).expect("octal byte")
        })
        .collect()
}

fn yaml_fields(document: Yaml) -> (String, BTreeMap<String, String>) {
    let Yaml::Map(entries) = document else {
        panic!("helper yaml map")
    };
    let run = entries
        .iter()
        .find(|(key, _)| key == "run")
        .and_then(|(_, value)| match value {
            Yaml::Str(value) => Some(value.clone()),
            _ => None,
        })
        .expect("helper run");
    let environment = entries
        .iter()
        .find(|(key, _)| key == "env")
        .and_then(|(_, value)| match value {
            Yaml::Map(values) => Some(
                values
                    .iter()
                    .filter_map(|(key, value)| match value {
                        Yaml::Str(value) => Some((key.clone(), value.clone())),
                        _ => None,
                    })
                    .collect(),
            ),
            _ => None,
        })
        .expect("helper environment");
    (run, environment)
}

fn assert_transport_round_trip(
    record: &CompiledSourceHelper,
    environment: &BTreeMap<String, String>,
) -> Result<(), Box<dyn std::error::Error>> {
    let args = record.invocation().args();
    let argument_sha = velnor_actions_contract::compiled_source_sha256(args[1].as_bytes());
    let source_sha = velnor_actions_contract::compiled_source_sha256(record.source().as_bytes());
    let script = format!(
        "{TRANSPORT}\nimport hashlib, os, sys\nsource, values, prefix = decode_transport(dict(os.environ), sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4]), sys.argv[7], 2, [])\nassert hashlib.sha256(source.encode()).hexdigest() == sys.argv[5]\nassert hashlib.sha256(values[1].encode()).hexdigest() == sys.argv[6]\nassert len(values) == 6\nassert prefix == []\n"
    );
    let encoded_args = serde_json::to_vec(args)?;
    let descriptor_sha = record.invocation().descriptor().source_sha256().to_owned();
    let source_len = record.source().len().to_string();
    let encoded_args_sha = velnor_actions_contract::compiled_source_sha256(&encoded_args);
    let encoded_args_len = encoded_args.len().to_string();
    let execution_sha = velnor_actions_contract::compiled_source_sha256(b"[]");
    let child = Command::new("/usr/bin/python3")
        .env_clear()
        .envs(environment)
        .args([
            "-I",
            "-S",
            "-c",
            &script,
            &descriptor_sha,
            &source_len,
            &encoded_args_sha,
            &encoded_args_len,
            &source_sha,
            &argument_sha,
            &execution_sha,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let output = child.wait_with_output()?;
    if !output.status.success() {
        return Err(format!(
            "transport decoder failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    Ok(())
}

fn fixture_repository() -> Result<TempDir, Box<dyn std::error::Error>> {
    let repository = tempfile::tempdir()?;
    let root = repository.path();
    git(root, &["init", "-b", "testmain"])?;
    fs::create_dir_all(root.join(".velnor"))?;
    fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\nrunner_label = \"macos-26\"\ndefault_branch = \"testmain\"\n[stacks.tofu]\nroots = [\".\"]\n",
    )?;
    fs::write(
        root.join("main.tf"),
        "terraform {\n  required_providers {\n    random = {\n      source = \"hashicorp/random\"\n      version = \"3.6.3\"\n    }\n  }\n}\nresource \"random_pet\" \"example\" {}\n",
    )?;
    fs::write(root.join(".terraform.lock.hcl"), NATIVE_LOCK)?;
    Ok(repository)
}

fn git(root: &Path, args: &[&str]) -> Result<(), Box<dyn std::error::Error>> {
    let status = git_fixture::command(root)?.args(args).status()?;
    if !status.success() {
        return Err(format!("git {args:?} failed").into());
    }
    Ok(())
}
