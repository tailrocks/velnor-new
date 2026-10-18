use std::collections::BTreeMap;

use velnor_actions_contract::{Step, StepKind, StepRole};

use super::cache_action_env;

fn step(role: Option<StepRole>) -> Step {
    Step {
        name: "Cache operation".to_owned(),
        id: None,
        role,
        condition: None,
        kind: StepKind::Action {
            uses: "actions/cache/restore@0123456789012345678901234567890123456789".to_owned(),
            with: BTreeMap::new(),
            env: BTreeMap::new(),
        },
    }
}

#[test]
fn typed_cache_action_gets_only_the_installed_action_local_path() -> Result<(), crate::RenderError>
{
    let rendered = cache_action_env(&step(Some(StepRole::CargoSourcesRestore)), &BTreeMap::new())?;

    assert_eq!(
        rendered,
        BTreeMap::from([(
            "PATH".to_owned(),
            crate::steps::CACHE_TAR_LAUNCHER_PATH_OUTPUT_EXPR.to_owned(),
        )])
    );
    Ok(())
}

#[test]
fn ordinary_actions_keep_their_existing_environment() -> Result<(), crate::RenderError> {
    let env = BTreeMap::from([("LANG".to_owned(), "C.UTF-8".to_owned())]);
    let rendered = cache_action_env(&step(None), &env)?;

    assert_eq!(rendered, env);
    Ok(())
}

#[test]
fn typed_cache_action_cannot_choose_another_path() -> Result<(), crate::RenderError> {
    let env = BTreeMap::from([("PATH".to_owned(), "/tmp/unverified".to_owned())]);

    let Err(error) = cache_action_env(&step(Some(StepRole::CargoSourcesSave)), &env) else {
        return Err(crate::RenderError::InvalidWorkflow(
            "test_expected_cache_path_rejection".to_owned(),
        ));
    };

    assert!(error.to_string().contains("cache_action_path_override"));
    Ok(())
}
