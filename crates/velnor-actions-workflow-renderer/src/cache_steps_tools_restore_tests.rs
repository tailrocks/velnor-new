use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use super::TOOLS_CACHE_ADMISSION_SCRIPT;

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "velnor-tools-restore-{name}-{}",
        std::process::id()
    ));
    fs::remove_dir_all(&path).ok();
    fs::create_dir_all(&path).expect("scratch");
    path
}

fn populate_cache_paths(root: &Path) -> (PathBuf, PathBuf) {
    let home = root.join("home");
    let runner_temp = root.join("runner-temp");
    let paths = [
        home.join(".local/share/mise"),
        runner_temp.join("velnor/rustup"),
        runner_temp.join("velnor/cargo/bin"),
    ];
    for path in paths {
        fs::create_dir_all(path).expect("cache directory");
    }
    for path in [
        runner_temp.join("velnor/cargo/.crates.toml"),
        runner_temp.join("velnor/cargo/.crates2.json"),
    ] {
        fs::write(path, "seed-state").expect("cache file");
    }
    (home, runner_temp)
}

fn run_script(
    home: &Path,
    runner_temp: &Path,
    cache_hit: Option<&str>,
    expected_key: Option<&str>,
    matched_key: Option<&str>,
    seed_admitted: Option<&str>,
) -> Output {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(TOOLS_CACHE_ADMISSION_SCRIPT)
        .env("HOME", home)
        .env("RUNNER_TEMP", runner_temp);
    for (name, value) in [
        ("TOOLS_CACHE_HIT", cache_hit),
        ("TOOLS_EXPECTED_KEY", expected_key),
        ("TOOLS_MATCHED_KEY", matched_key),
        ("TOOLS_SEED_ADMITTED", seed_admitted),
    ] {
        if let Some(value) = value {
            command.env(name, value);
        } else {
            command.env_remove(name);
        }
    }
    command.output().expect("run cache admission")
}

fn assert_paths_kept(home: &Path, runner_temp: &Path) {
    assert!(home.join(".local/share/mise").is_dir());
    assert!(runner_temp.join("velnor/rustup").is_dir());
    assert!(runner_temp.join("velnor/cargo/bin").is_dir());
    assert!(runner_temp.join("velnor/cargo/.crates.toml").is_file());
    assert!(runner_temp.join("velnor/cargo/.crates2.json").is_file());
    assert_eq!(
        fs::read_to_string(runner_temp.join("velnor/cargo/.crates.toml"))
            .expect("cache file bytes"),
        "seed-state"
    );
}

fn assert_paths_removed(home: &Path, runner_temp: &Path) {
    assert!(!home.join(".local/share/mise").exists());
    assert!(!runner_temp.join("velnor/rustup").exists());
    assert!(!runner_temp.join("velnor/cargo/bin").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates.toml").exists());
    assert!(!runner_temp.join("velnor/cargo/.crates2.json").exists());
}

fn assert_case(
    name: &str,
    cache_hit: Option<&str>,
    expected_key: Option<&str>,
    matched_key: Option<&str>,
    seed_admitted: Option<&str>,
    keep: bool,
) {
    let root = scratch(name);
    let (home, runner_temp) = populate_cache_paths(&root);
    let output = run_script(
        &home,
        &runner_temp,
        cache_hit,
        expected_key,
        matched_key,
        seed_admitted,
    );
    assert!(output.status.success(), "{output:?}");
    if keep {
        assert_paths_kept(&home, &runner_temp);
    } else {
        assert_paths_removed(&home, &runner_temp);
    }
    fs::remove_dir_all(root).expect("cleanup");
}

#[test]
fn preserves_exact_hits_and_validated_seed_on_pure_cache_miss() {
    let expected_key = format!("mise-tools-v2-{}", "a".repeat(64));
    assert_case(
        "exact-hit",
        Some("true"),
        Some(&expected_key),
        Some(&expected_key),
        Some("false"),
        true,
    );
    assert_case(
        "seed-miss",
        None,
        Some(&expected_key),
        None,
        Some("true"),
        true,
    );
}

#[test]
fn removes_untrusted_partial_and_malformed_restore_state() {
    let expected_key = format!("mise-tools-v2-{}", "a".repeat(64));
    assert_case(
        "miss-without-admitted-seed",
        None,
        Some(&expected_key),
        None,
        None,
        false,
    );
    assert_case(
        "non-exact-restore",
        Some("false"),
        Some(&expected_key),
        Some("mise-tools-v2-other"),
        Some("true"),
        false,
    );
    assert_case(
        "false-without-matched-key",
        Some("false"),
        Some(&expected_key),
        Some(""),
        Some("true"),
        false,
    );
    assert_case(
        "inconsistent-restore-outputs",
        Some("false"),
        Some(&expected_key),
        Some(&expected_key),
        Some("true"),
        false,
    );
    assert_case(
        "exact-hit-with-malformed-key",
        Some("true"),
        Some("mise-tools-v2-"),
        Some("mise-tools-v2-"),
        Some("true"),
        false,
    );
    assert_case(
        "missing-expected-key",
        Some("false"),
        None,
        Some(""),
        Some("true"),
        false,
    );
    assert_case(
        "malformed-expected-key",
        None,
        Some("mise-tools-v2-"),
        None,
        Some("true"),
        false,
    );
}

#[test]
fn valid_seed_without_file_payload_falls_through_to_miss_cleanup() {
    let root = scratch("empty-seed-to-miss");
    let (home, runner_temp) = populate_cache_paths(&root);
    let seed = root.join("seed");
    let key = format!("mise-tools-v2-{}", "a".repeat(64));
    fs::create_dir_all(seed.join("mise/tree")).expect("empty mise tree");
    fs::create_dir_all(seed.join("rustup/tree")).expect("empty rustup tree");
    fs::write(seed.join("PROVENANCE"), "velnor-host-seed-v1\n").expect("provenance");
    fs::write(seed.join("mise/KEY"), &key).expect("seed key");
    let github_output = root.join("GITHUB_OUTPUT");
    fs::write(&github_output, "").expect("composite output");

    let script = crate::tool_seed::tool_seed_action_script(seed.to_str().expect("seed path"))
        .expect("seed script");
    let script = crate::tool_seed_test_support::mock_trust_commands(&script, &root);
    let seed_run = Command::new("bash")
        .arg("-c")
        .arg(script)
        .env("HOME", &home)
        .env("RUNNER_TEMP", &runner_temp)
        .env("RUNNER_OS", "Linux")
        .env("SEED_KEY", &key)
        .env("SEED_TEST_ROOT", &seed)
        .env(
            "SEED_TEST_MOUNTS",
            format!("{} ext4 0:77 ro,nosuid,nodev", seed.display()),
        )
        .env("SEED_TEST_SKIP_OWNER_SCAN", "1")
        .env("GITHUB_OUTPUT", &github_output)
        .output()
        .expect("run seed import");
    assert!(seed_run.status.success(), "{seed_run:?}");
    let admitted_output = fs::read_to_string(&github_output).expect("seed output");
    let admitted = admitted_output
        .strip_prefix("seed_admitted=")
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let restore = run_script(&home, &runner_temp, None, Some(&key), None, admitted);
    assert!(restore.status.success(), "{restore:?}");
    assert_paths_removed(&home, &runner_temp);
    assert_eq!(admitted_output, "");
    fs::remove_dir_all(root).expect("cleanup empty-seed miss");
}

#[test]
fn rendered_action_wires_seed_admission_and_unset_miss_outputs() {
    let file = super::action_file("0.1.0").expect("restore action");
    for expected in [
        "seed-admitted:",
        "default: \"false\"",
        "TOOLS_SEED_ADMITTED: ${{ inputs.seed-admitted }}",
        "cache_hit=\\\"${TOOLS_CACHE_HIT-}\\\"",
        "matched_key=\\\"${TOOLS_MATCHED_KEY-}\\\"",
        "elif [ -z \\\"$cache_hit\\\" ]",
        "[ \\\"$seed_admitted\\\" = true ]",
    ] {
        assert!(file.bytes.contains(expected), "missing {expected}");
    }
}

#[test]
fn generated_action_tree_scopes_seed_input_to_the_restore_composite() {
    use crate::yaml::Yaml;

    fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
        let Yaml::Map(entries) = node else {
            panic!("expected mapping containing {key}");
        };
        entries
            .iter()
            .find_map(|(name, value)| (name == key).then_some(value))
            .unwrap_or_else(|| panic!("missing mapping field {key}"))
    }

    let body = super::action_body().expect("restore action tree");
    let seed_input = field(field(&body, "inputs"), "seed-admitted");
    assert_eq!(field(seed_input, "required"), &Yaml::Bool(false));
    assert_eq!(
        field(seed_input, "default"),
        &Yaml::Quoted("false".to_owned())
    );

    let Yaml::Seq(steps) = field(field(&body, "runs"), "steps") else {
        panic!("composite steps are a sequence");
    };
    assert_eq!(field(&steps[0], "id"), &Yaml::Str("restore".to_owned()));
    let admission_env = field(&steps[1], "env");
    assert_eq!(
        field(admission_env, "TOOLS_SEED_ADMITTED"),
        &Yaml::Str("${{ inputs.seed-admitted }}".to_owned())
    );
}

#[test]
fn restore_call_requires_the_exact_prelude_output_input() {
    let mut wrong = restore_step();
    let velnor_actions_contract::StepKind::Action { with, .. } = &mut wrong.kind else {
        panic!("restore is an action");
    };
    with.insert(
        crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT.to_owned(),
        "caller-value".to_owned(),
    );
    assert!(super::validate_call(&wrong).is_err());

    let mut missing = restore_step();
    let velnor_actions_contract::StepKind::Action { with, .. } = &mut missing.kind else {
        panic!("restore is an action");
    };
    with.remove(crate::cache_steps::TOOLS_SEED_ADMITTED_INPUT);
    assert!(super::validate_call(&missing).is_err());
}

fn restore_step() -> velnor_actions_contract::Step {
    crate::cache_steps::tools_cache_step(
        true,
        crate::cache_p08::TOOLS_CACHE_KEY_EXPRESSION,
        Some(crate::cache_p08::TOOLS_CACHE_RESTORE_CONDITION.to_owned()),
    )
    .expect("typed restore step")
}
