#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use velnor_actions_contract::{Step, StepKind};

    use super::action_step_to_yaml;
    use crate::yaml;

    #[test]
    fn only_generated_task_calls_use_a_compact_flow_input_map() {
        let digest = format!("b3-{}", "a".repeat(64));
        let with = BTreeMap::from([("digest".to_owned(), digest.clone())]);
        let env = BTreeMap::new();
        let job_env = BTreeMap::new();
        let task_uses = "./.github/actions/declared-task-0";
        let task_step = Step {
            name: "Run declared task".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: task_uses.to_owned(),
                with: with.clone(),
                env: env.clone(),
            },
        };
        let task_yaml = action_step_to_yaml(
            "rust-crate-0",
            &task_step,
            task_uses,
            &with,
            &env,
            &job_env,
            Some("ubuntu-24.04"),
        )
        .expect("generated task action is valid");
        let task_yaml = yaml::render_yaml(&task_yaml);
        assert!(task_yaml.contains(&format!("with: {{\"digest\": \"{digest}\"}}")));

        let ordinary_uses = format!("actions/cache@{}", "a".repeat(40));
        let ordinary_step = Step {
            name: "Ordinary action".to_owned(),
            id: None,
            role: None,
            condition: None,
            kind: StepKind::Action {
                uses: ordinary_uses.clone(),
                with: with.clone(),
                env: env.clone(),
            },
        };
        let ordinary_yaml = action_step_to_yaml(
            "rust-crate-0",
            &ordinary_step,
            &ordinary_uses,
            &with,
            &env,
            &job_env,
            Some("ubuntu-24.04"),
        )
        .expect("ordinary pinned action is valid");
        let ordinary_yaml = yaml::render_yaml(&ordinary_yaml);
        assert!(ordinary_yaml.contains(&format!("with:\n  digest: {digest}")));
    }
}
