use std::error::Error;

use velnor_actions_contract::{StepKind, StepRole};
use velnor_actions_mise::ToolCatalog;

use super::super::{VERIFY_JACTIONLINT_STEP, script, verifier_shell_step, verify_jactionlint_step};

#[test]
fn typed_action_and_inline_step_keep_the_canonical_jactionlint_script() -> Result<(), Box<dyn Error>>
{
    let catalog = ToolCatalog::pinned();
    let action = verify_jactionlint_step(&catalog);
    assert_eq!(action.name, VERIFY_JACTIONLINT_STEP);
    assert_eq!(action.role, Some(StepRole::VerifyJactionlint));
    assert!(matches!(
        action.kind,
        StepKind::Action { uses, ref with, ref env }
            if uses == velnor_actions_contract::workflow::JACTIONLINT_VERIFIER_USES
                && with.is_empty()
                && env.is_empty()
    ));

    let inline = verifier_shell_step(&catalog)?;
    assert_eq!(inline.name, VERIFY_JACTIONLINT_STEP);
    assert_eq!(inline.role, Some(StepRole::VerifyJactionlint));
    let StepKind::Shell { run, env } = &inline.kind else {
        return Err("inline verifier must remain a shell step".into());
    };
    assert_eq!(
        env,
        &velnor_actions_workflow_renderer::toolchain_env::credential_scrub()
    );
    let text = script(&catalog);
    assert_eq!(
        run,
        &vec![
            "sh".to_owned(),
            "-c".to_owned(),
            velnor_actions_workflow_renderer::toolchain_env::with_credential_unset_script(&text),
        ],
        "the inline shell step must execute the complete canonical script",
    );
    for row in velnor_actions_mise::JACTIONLINT_DIGESTS {
        assert!(text.contains(row.executable_sha256), "{}", row.platform);
    }
    let binder = velnor_actions_contract::config::selected_mise::BIND_SELECTED_MISE_FUNCTION;
    assert_eq!(text.matches(binder).count(), 1, "one canonical binder");
    assert!(!text.contains("$("), "{text}");
    assert!(!text.contains('`'), "{text}");
    let without_binder = text.replacen(binder, "", 1);
    assert!(!without_binder.contains('\n'), "{without_binder}");
    assert!(!without_binder.contains("$("), "{without_binder}");
    assert!(!without_binder.contains('`'), "{without_binder}");
    Ok(())
}
