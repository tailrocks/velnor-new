use std::collections::BTreeMap;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::process::Command;

use super::*;

struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("current epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "velnor-cache-budget-{}-{unique}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .expect("exclusive clock fixture");
        Self(path)
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

fn run_clock_script(script: &str, now: &str, started: &str, deadline: &str) -> (String, String) {
    run_with_output(script, now, started, deadline, false)
}

fn run_with_output(
    script: &str,
    now: &str,
    started: &str,
    deadline: &str,
    broken: bool,
) -> (String, String) {
    let dir = Scratch::new();
    let date = dir.path().join("date");
    std::fs::write(&date, "#!/bin/sh\nprintf '%s\\n' \"$FIXTURE_NOW\"\n")
        .expect("controlled OS clock");
    std::fs::set_permissions(&date, std::fs::Permissions::from_mode(0o700))
        .expect("executable clock");
    let output = dir.path().join("outputs");
    if broken {
        std::fs::create_dir(&output).expect("unwritable output directory");
    } else {
        std::fs::write(&output, "").expect("runner outputs");
    }
    let result = Command::new("/bin/bash")
        .args(["-c", script])
        .env_clear()
        .env("PATH", format!("{}:/usr/bin:/bin", dir.path().display()))
        .env("FIXTURE_NOW", now)
        .env("GITHUB_OUTPUT", &output)
        .env("VELNOR_CACHE_JOB_SECONDS", "300")
        .env("VELNOR_CACHE_REQUIRED_SECONDS", "90")
        .env("VELNOR_CACHE_STARTED", started)
        .env("VELNOR_CACHE_DEADLINE", deadline)
        .output()
        .expect("bounded finite clock script");
    assert!(result.status.success(), "{:?}", result.stderr);
    (
        if broken {
            String::new()
        } else {
            std::fs::read_to_string(output).expect("recorded outputs")
        },
        String::from_utf8(result.stdout).expect("fixed diagnostics"),
    )
}

#[test]
fn actual_scripts_share_one_clock_across_slow_and_successive_writes() {
    let (clock, _) = run_clock_script(CLOCK_SCRIPT, "1000", "", "");
    assert_eq!(clock, "started=1000\ndeadline=1300\n");
    for now in ["1100", "1150", "1210"] {
        let (out, warning) = run_clock_script(ADMISSION_SCRIPT, now, "1000", "1300");
        assert_eq!(out, "admitted=true\n", "{now}");
        assert_eq!(warning, "");
    }
    // A first write taking its entire minute does not grant a fresh minute
    // to the second write: both use the original deadline and 30s reserve.
    let (second, warning) = run_clock_script(ADMISSION_SCRIPT, "1211", "1000", "1300");
    assert_eq!(second, "admitted=false\n");
    assert!(warning.contains("cache_unavailable"));
    assert_eq!(run_clock_script(CLOCK_SCRIPT, "1000", "", "").0, clock);
}

#[test]
fn missing_malformed_backward_or_expired_clock_refuses_without_failing_quality() {
    for (now, start, end) in [
        ("1100", "", "1300"),
        ("bad", "1000", "1300"),
        ("1100", "1000", "1300\ninjected"),
        ("999", "1000", "1300"),
        ("1300", "1000", "1300"),
        ("1100", "1000", "1400"),
        ("001100", "1000", "1300"),
        ("000000009", "1000", "1300"),
        ("1100", "0008", "1300"),
        ("1100", "99999999999", "1300"),
    ] {
        let (out, warning) = run_clock_script(ADMISSION_SCRIPT, now, start, end);
        assert_eq!(out, "admitted=false\n", "{now}/{start}/{end}");
        assert!(warning.contains("cache_unavailable"));
    }
    for now in ["", "bad", "001000", "000000009", "0008"] {
        let (out, warning) = run_clock_script(CLOCK_SCRIPT, now, "", "");
        assert_eq!(out, "");
        assert!(warning.contains("cache_unavailable"));
    }
}

fn save(role: StepRole) -> Step {
    Step {
        name: "Native save".to_owned(),
        id: None,
        role: Some(role),
        condition: Some("always() && !cancelled() && producer == 'success'".to_owned()),
        parallel: false,
        kind: StepKind::Action {
            uses: crate::steps::TOOLS_SAVE_USES.to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

fn rendered(step: &Step) -> Result<Yaml, RenderError> {
    crate::document_steps::non_internal_step_to_yaml("plan", step, &BTreeMap::new(), "ubuntu-26.04")
}

fn field<'a>(step: &'a Yaml, key: &str) -> &'a Yaml {
    let Yaml::Map(entries) = step else {
        panic!("step mapping")
    };
    &entries
        .iter()
        .find(|(name, _)| name == key)
        .expect("field")
        .1
}

#[test]
fn actual_renderer_bounds_all_native_saves_and_preserves_their_original_gates() {
    let saves = [
        StepRole::CargoSourcesSave,
        StepRole::ToolsCacheSave,
        StepRole::TofuProvidersSave,
    ];
    let mut steps = vec![Yaml::Map(vec![("run".to_owned(), Yaml::str("false"))])];
    for role in saves {
        let step = rendered(&save(role)).expect("native save");
        assert_eq!(field(&step, "timeout-minutes"), &Yaml::Int(1));
        assert_eq!(field(&step, "continue-on-error"), &Yaml::Bool(true));
        let Yaml::Str(condition) = field(&step, "if") else {
            panic!("condition")
        };
        assert_eq!(
            condition,
            "always() && !cancelled() && producer == 'success'"
        );
        steps.push(step);
    }
    apply(&mut steps, 5).expect("shared job budget");
    assert_eq!(steps.len(), 11);
    assert_eq!(field(&steps[0], "id"), &Yaml::str(CLOCK_ID));
    assert_eq!(field(&steps[1], "run"), &Yaml::str("false"));
    assert_eq!(
        field(&steps[2], "if"),
        &Yaml::str("always() && !cancelled() && producer == 'success'")
    );
    assert_eq!(
        field(&steps[5], "if"),
        &Yaml::str("always() && !cancelled() && producer == 'success'")
    );
    assert!(crate::yaml::render_yaml(&steps[2]).contains("velnor-cache-clock.outputs.deadline"));
    assert!(crate::yaml::render_yaml(&steps[5]).contains("velnor-cache-clock.outputs.deadline"));
    for index in [3, 6, 9] {
        let Yaml::Str(condition) = field(&steps[index], "if") else {
            panic!("save condition")
        };
        assert!(
            condition.contains("producer == 'success'")
                && condition.contains("-budget.outputs.admitted")
        );
    }
    for index in [4, 7, 10] {
        assert!(crate::yaml::render_yaml(&steps[index]).contains(".outcome == 'failure'"));
        assert!(crate::yaml::render_yaml(&steps[index]).contains("cache_unavailable"));
    }
    assert!(!crate::yaml::render_yaml(&steps[1]).contains("continue-on-error"));
}

#[test]
fn forged_save_roles_and_non_cache_quality_steps_cannot_get_the_exception() {
    let mut forged = save(StepRole::CargoSourcesSave);
    forged.kind = StepKind::Shell {
        run: vec!["false".to_owned()],
        env: BTreeMap::new(),
    };
    let mut entries = vec![("if".to_owned(), Yaml::str("always()"))];
    assert!(decorate(&forged, &mut entries, true).is_err());
    forged.kind = StepKind::Action {
        uses: "actions/upload-artifact@bad".to_owned(),
        with: BTreeMap::new(),
        env: BTreeMap::new(),
    };
    assert!(decorate(&forged, &mut entries, true).is_err());
    let mut quality = save(StepRole::CargoSourcesSave);
    quality.role = None;
    quality.kind = StepKind::Shell {
        run: vec!["false".to_owned()],
        env: BTreeMap::new(),
    };
    let quality = rendered(&quality).expect("ordinary quality failure");
    let mut steps = vec![quality.clone()];
    apply(&mut steps, 5).expect("no save budget needed");
    assert_eq!(steps, vec![quality]);
    assert!(!crate::yaml::render_yaml(&Yaml::Seq(steps)).contains("continue-on-error"));
}

#[test]
fn unavailable_output_does_not_fail_clock_or_admission() {
    for script in [CLOCK_SCRIPT, ADMISSION_SCRIPT] {
        let (out, warning) = run_with_output(script, "1100", "1000", "1300", true);
        assert_eq!(out, "");
        assert!(warning.contains("cache_unavailable"));
    }
}

#[test]
fn reserved_helper_ids_and_duplicate_saves_are_rejected_without_mutation() {
    for id in [
        CLOCK_ID.to_owned(),
        format!("{SOURCES_ID}-budget"),
        format!("{TOOLS_ID}-budget"),
        format!("{PROVIDERS_ID}-budget"),
    ] {
        for value in [Yaml::str(&id), Yaml::Quoted(id.clone())] {
            let mut steps = vec![
                Yaml::Map(vec![("id".to_owned(), value)]),
                rendered(&save(StepRole::CargoSourcesSave)).expect("save"),
            ];
            let original = steps.clone();
            assert!(apply(&mut steps, 5).is_err(), "{id}");
            assert_eq!(steps, original);
        }
    }
    let save = rendered(&save(StepRole::ToolsCacheSave)).expect("save");
    let mut steps = vec![save.clone(), save];
    let original = steps.clone();
    assert!(apply(&mut steps, 5).is_err());
    assert_eq!(steps, original);
}

#[test]
fn composite_save_cannot_receive_an_orphaned_job_budget() {
    let mut step = crate::tofu_cache::tofu_providers_save_step().expect("typed provider save");
    step.condition = Some(
        velnor_actions_contract::workflow::cache_family::TOFU_PROVIDERS_SAVE_CONDITION.to_owned(),
    );
    let mut entries = vec![("if".to_owned(), Yaml::str("always()"))];
    assert!(decorate(&step, &mut entries, false).is_err());
    assert_eq!(entries.len(), 1);
    let ctx = crate::render::RenderContext {
        tool_pins: Vec::new(),
        generator_version: "0.1.29".to_owned(),
        report_helper_version: "0.1.29".to_owned(),
        runs_on: "ubuntu-26.04".to_owned(),
        scale_set_selector: None,
        staged_binary: "$RUNNER_TEMP/velnor/bin/velnor-actions-0.1.29".to_owned(),
        request_dir: "${{ runner.temp }}/velnor/request".to_owned(),
        checkout_uses: format!("actions/checkout@{:040x}", 0),
        validator_commands: Vec::new(),
        candidate: None,
        preseed: false,
        workflow_tasks: Vec::new(),
        pull_request_cache_policy: velnor_actions_contract::PullRequestCachePolicy::ReadOnly,
        mise_quiet: true,
        plan_consumer_env: BTreeMap::new(),
    };
    let error = crate::lane_share_sections::composite_file("rust-0", &[step], &ctx)
        .expect_err("composite must never reset the job clock");
    assert!(
        error
            .to_string()
            .contains("optional_cache_save_requires_job_scope"),
        "{error}"
    );
}
