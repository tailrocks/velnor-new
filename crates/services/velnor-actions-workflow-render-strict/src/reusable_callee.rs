//! Strict standalone rendering for the V1 reusable-workflow callee.

use velnor_actions_contract_workflow::workflow::reusable_callee::ReusableCalleePolicy;
use velnor_actions_workflow_document::document::reusable_callee_document;
use velnor_actions_workflow_steps::RenderError;
use velnor_actions_workflow_tree::{
    marker::with_marker, workflow_size::check_workflow_size, yaml::render_yaml,
};

/// Render the strict V1 reusable callee with the exact generator marker.
///
/// This function emits contract and identity structure only. It does not
/// enable the V2 publisher runtime or perform any repository/release action.
///
/// # Errors
///
/// Returns [`RenderError`] for ambiguous policy, a bad version, a private
/// subcommand token, or an unexpectedly oversized document.
pub fn render_reusable_callee(
    policy: &ReusableCalleePolicy,
    generator_version: &str,
) -> Result<String, RenderError> {
    let document = reusable_callee_document(policy)?;
    let body = render_yaml(&document);
    let text = with_marker(generator_version, &body)?;
    check_workflow_size(".github/workflows/reusable-callee.yaml", &text)?;
    velnor_actions_workflow_steps::scan_for_private_subcommands(&text)?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::render_reusable_callee;
    use velnor_actions_contract_workflow::workflow::reusable_callee::{
        REUSABLE_CALLEE_GUARD_JOB, REUSABLE_CALLEE_INPUT_TYPE, REUSABLE_CALLEE_INPUTS,
        REUSABLE_CALLEE_WRITE_JOB, ReusableCalleePolicy,
    };
    use velnor_actions_workflow_document::document::reusable_callee_document;
    use velnor_actions_workflow_steps::RenderError;
    use velnor_actions_workflow_tree::marker::MARKER_PREFIX;
    use velnor_actions_workflow_tree::yaml::Yaml;

    const VERSION: &str = "0.0.0-test";
    const GUARD_CONDITION: &str = "github.workflow_sha == github.sha";
    const WRITER_CONDITION: &str = "needs.identity-guard.result == 'success'";

    fn policy() -> ReusableCalleePolicy {
        ReusableCalleePolicy {
            caller_repository: "tailrocks/example-caller".to_owned(),
            caller_workflow_path: ".github/workflows/publish-velnor.yml".to_owned(),
            caller_branch: "main".to_owned(),
            callee_repository: "tailrocks/example-callee".to_owned(),
        }
    }

    fn entry<'a>(value: &'a Yaml, key: &str) -> Option<&'a Yaml> {
        let Yaml::Map(entries) = value else {
            return None;
        };
        entries
            .iter()
            .find_map(|(name, item)| (name == key).then_some(item))
    }

    fn entries(value: &Yaml) -> Option<&Vec<(String, Yaml)>> {
        match value {
            Yaml::Map(entries) => Some(entries),
            _ => None,
        }
    }

    fn names(value: &Yaml) -> Option<Vec<&str>> {
        entries(value).map(|fields| fields.iter().map(|(name, _)| name.as_str()).collect())
    }

    fn sequence(value: &Yaml) -> Option<&Vec<Yaml>> {
        match value {
            Yaml::Seq(items) => Some(items),
            _ => None,
        }
    }

    fn step(name: &str, command: &str) -> Yaml {
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str(name)),
            ("run".to_owned(), Yaml::str(command)),
        ])
    }

    #[test]
    fn strict_document_has_exact_closed_shape() {
        let document = reusable_callee_document(&policy());
        assert!(matches!(document, Ok(_)));
        let Ok(document) = document else {
            return;
        };
        assert_eq!(
            names(&document),
            Some(vec!["name", "on", "permissions", "jobs"])
        );

        let trigger = entry(&document, "on").unwrap_or(&Yaml::Null);
        assert_eq!(names(trigger), Some(vec!["workflow_call"]));
        assert!(
            matches!(entry(trigger, "workflow_call"), Some(Yaml::Map(inputs)) if inputs.len() == 1)
        );
        assert_eq!(
            entry(trigger, "workflow_call")
                .and_then(|workflow_call| entry(workflow_call, "inputs"))
                .and_then(entries)
                .map(Vec::len),
            Some(REUSABLE_CALLEE_INPUTS.len())
        );

        let jobs = entry(&document, "jobs").unwrap_or(&Yaml::Null);
        assert_eq!(
            names(jobs),
            Some(vec![REUSABLE_CALLEE_GUARD_JOB, REUSABLE_CALLEE_WRITE_JOB])
        );
        let guard = entry(jobs, REUSABLE_CALLEE_GUARD_JOB).unwrap_or(&Yaml::Null);
        let writer = entry(jobs, REUSABLE_CALLEE_WRITE_JOB).unwrap_or(&Yaml::Null);
        assert_eq!(
            names(guard),
            Some(vec![
                "runs-on",
                "timeout-minutes",
                "permissions",
                "if",
                "steps"
            ])
        );
        assert_eq!(
            names(writer),
            Some(vec![
                "runs-on",
                "timeout-minutes",
                "permissions",
                "needs",
                "if",
                "steps"
            ])
        );

        let guard_steps = vec![step("Confirm V1 identity policy", "exit 0")];
        let writer_steps = vec![step("V2 runtime is not enabled", "exit 108")];
        assert_eq!(
            sequence(entry(guard, "steps").unwrap_or(&Yaml::Null)),
            Some(&guard_steps)
        );
        assert_eq!(
            sequence(entry(writer, "steps").unwrap_or(&Yaml::Null)),
            Some(&writer_steps)
        );
    }

    #[test]
    fn document_has_closed_callee_structure() {
        let document = reusable_callee_document(&policy());
        assert!(matches!(document, Ok(_)));
        let Ok(document) = document else {
            return;
        };
        let trigger = entry(
            entry(&document, "on").unwrap_or(&Yaml::Null),
            "workflow_call",
        );
        assert!(matches!(entry(&document, "on"), Some(Yaml::Map(trigger)) if trigger.len() == 1));
        let inputs = entry(trigger.unwrap_or(&Yaml::Null), "inputs");
        assert_eq!(
            entries(inputs.unwrap_or(&Yaml::Null)).map(Vec::len),
            Some(REUSABLE_CALLEE_INPUTS.len())
        );
        if let Some(inputs) = entries(inputs.unwrap_or(&Yaml::Null)) {
            for input in REUSABLE_CALLEE_INPUTS {
                let spec = inputs
                    .iter()
                    .find_map(|(name, item)| (name == input.name).then_some(item));
                let fields = entries(spec.unwrap_or(&Yaml::Null));
                assert_eq!(fields.as_ref().map(|fields| fields.len()), Some(2));
                assert_eq!(
                    fields
                        .and_then(|fields| fields.iter().find(|(key, _)| key == "type"))
                        .map(|(_, value)| value),
                    Some(&Yaml::str(REUSABLE_CALLEE_INPUT_TYPE))
                );
                assert_eq!(
                    fields
                        .and_then(|fields| fields.iter().find(|(key, _)| key == "required"))
                        .map(|(_, value)| value),
                    Some(&Yaml::Bool(true))
                );
            }
            assert!(!inputs.iter().any(|(name, _)| {
                !REUSABLE_CALLEE_INPUTS
                    .iter()
                    .any(|input| input.name == name)
            }));
        }
        let jobs = entry(&document, "jobs").unwrap_or(&Yaml::Null);
        let guard = entry(jobs, REUSABLE_CALLEE_GUARD_JOB).unwrap_or(&Yaml::Null);
        let writer = entry(jobs, REUSABLE_CALLEE_WRITE_JOB).unwrap_or(&Yaml::Null);
        assert!(
            matches!(entry(guard, "permissions"), Some(Yaml::Map(fields)) if fields.is_empty())
        );
        assert!(
            matches!(entry(guard, "if"), Some(Yaml::Str(value)) if value.contains(GUARD_CONDITION))
        );
        assert!(matches!(entry(writer, "needs"), Some(Yaml::Seq(needs))
                if needs.contains(&Yaml::str(REUSABLE_CALLEE_GUARD_JOB))));
        assert!(matches!(entry(writer, "if"), Some(Yaml::Str(value))
                if value.contains(WRITER_CONDITION)));
        let permissions = [
            ("actions", "read"),
            ("contents", "write"),
            ("id-token", "none"),
            ("pull-requests", "none"),
        ];
        if let Some(Yaml::Map(writer_permissions)) = entry(writer, "permissions") {
            assert_eq!(writer_permissions.len(), permissions.len());
            for (scope, level) in permissions {
                assert!(
                    writer_permissions
                        .iter()
                        .any(|(name, value)| name == scope && value == &Yaml::str(level))
                );
            }
        } else {
            assert!(matches!(entry(writer, "permissions"), Some(Yaml::Map(_))));
        }
    }

    #[test]
    fn strict_render_is_marked_and_v2_fail_closed() {
        let text = render_reusable_callee(&policy(), VERSION);
        assert!(matches!(text, Ok(_)));
        let Ok(text) = text else {
            return;
        };
        assert!(text.starts_with(&format!("{MARKER_PREFIX}{VERSION}")));
        assert!(text.contains("workflow_call:"));
        assert!(text.contains(GUARD_CONDITION));
        assert!(text.contains(WRITER_CONDITION));
        assert!(text.contains("exit 108"));
        assert_eq!(
            text.lines()
                .filter(|line| line.trim_start().starts_with("uses:"))
                .count(),
            0
        );
        assert_eq!(
            text.lines()
                .filter(|line| line.trim_start().starts_with("run:"))
                .count(),
            2
        );
        assert!(!text.contains("fromJSON"));
        assert!(!text.contains("${{ inputs."));
        assert!(!text.contains("\n  default:"));
    }

    #[test]
    fn ambiguous_policy_fails_closed() {
        for mut bad in [policy(), policy(), policy()] {
            bad.caller_repository.clear();
            assert!(matches!(
                render_reusable_callee(&bad, VERSION),
                Err(RenderError::PolicyRejected { .. })
            ));
        }
        for field in ["caller_workflow_path", "caller_branch"] {
            let mut bad = policy();
            match field {
                "caller_workflow_path" => bad.caller_workflow_path.clear(),
                _ => bad.caller_branch.clear(),
            }
            assert!(matches!(
                render_reusable_callee(&bad, VERSION),
                Err(RenderError::PolicyRejected { .. })
            ));
        }
    }
}
