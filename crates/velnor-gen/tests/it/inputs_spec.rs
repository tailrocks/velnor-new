// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{BaseExpiry, InputsCli, RepoPath, StepKind, WORKFLOW_PATH};
use velnor_render::{MAX_RUN_CHARACTERS, render};

use crate::facts::TestResult;
use crate::rules::plan;

#[test]
fn large_root_lists_use_short_commands_and_expression_safe_path_data() -> TestResult {
    for count in [1, 60] {
        let manifest = RepoPath::new("dollar-${{x}} 'quoted/Cargo.toml")?;
        let target = RepoPath::new("cache-${{x}} 'quoted")?;
        let files = render(&plan(vec![
            StepKind::InputsRestore {
                roots: vec![(manifest, target.clone()); count],
                cli: InputsCli::Batched,
            },
            StepKind::RestoredTargets {
                targets: vec![target; count],
                expiry: BaseExpiry::Never,
                cli: InputsCli::Batched,
            },
        ])?)
        .map_err(|found| format!("{found:?}"))?;
        let yaml: serde_json::Value =
            serde_saphyr::from_str(files.get(&WORKFLOW_PATH).ok_or("no workflow")?)?;
        let steps = yaml["jobs"]["format"]["steps"]
            .as_array()
            .ok_or("no steps")?;
        let roots = steps[0]["env"]["VELNOR_INPUT_ROOTS"]
            .as_str()
            .ok_or("no root data")?;
        let targets = steps[1]["env"]["VELNOR_INPUT_TARGETS"]
            .as_str()
            .ok_or("no target data")?;
        assert!(!roots.contains("${{") && !targets.contains("${{"));
        let roots: Vec<[String; 2]> = serde_json::from_str(roots)?;
        let targets: Vec<String> = serde_json::from_str(targets)?;
        assert_eq!(roots.len(), count);
        assert_eq!(targets.len(), count);
        assert!(roots.iter().all(
            |pair| pair[0] == "646f6c6c61722d247b7b787d7d202771756f746564"
                && pair[1] == "63616368652d247b7b787d7d202771756f746564"
        ));
        assert!(
            targets
                .iter()
                .all(|path| path == "63616368652d247b7b787d7d202771756f746564")
        );
        for step in steps {
            assert!(
                step["run"]
                    .as_str()
                    .is_some_and(|run| run.chars().count() <= MAX_RUN_CHARACTERS),
                "{step}"
            );
        }
        assert!(
            steps[0]["run"]
                .as_str()
                .is_some_and(|run| run.contains("--roots-env VELNOR_INPUT_ROOTS"))
        );
        assert_eq!(
            steps[1]["run"],
            "velnor-ci restored --targets-env VELNOR_INPUT_TARGETS"
        );
    }
    Ok(())
}
