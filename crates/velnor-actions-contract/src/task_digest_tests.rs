//! Regression evidence for the unchanged canonical obligation preimage.

use super::*;

#[test]
fn absent_authority_preserves_compiler_preimage() -> Result<(), ContractError> {
    let argv = vec!["cargo".to_owned(), "check".to_owned()];
    let expected = br#"{"argv":["cargo","check"],"task_id":"task","toolchain_id":"tools"}"#;
    let preimage = TaskDigestInputs {
        task_id: "task",
        argv: &argv,
        toolchain_id: "tools",
        helper_obligation: None,
        native_recipe: None,
    };
    assert_eq!(canonical_json_bytes(&preimage)?, expected);
    assert_eq!(
        canonical_task_digest("task", &argv, "tools", None, None)?,
        digest_b3(expected)
    );
    Ok(())
}

#[test]
fn native_recipe_and_capability_bind_without_helper_transport() -> Result<(), ContractError> {
    let argv = vec!["native".to_owned()];
    let recipe = NativeValidationDescriptor::HomebrewPreparation {
        repository: "owner/homebrew-demo".to_owned(),
        has_casks: false,
    };
    let expected = br#"{"argv":["native"],"native_recipe":{"has_casks":false,"kind":"homebrew-preparation","repository":"owner/homebrew-demo"},"task_id":"task","toolchain_id":"tools"}"#;
    let digest = canonical_task_digest("task", &argv, "tools", None, Some(&recipe))?;
    assert_eq!(digest, digest_b3(expected));
    let changed = NativeValidationDescriptor::HomebrewPreparation {
        repository: "owner/homebrew-demo".to_owned(),
        has_casks: true,
    };
    assert_ne!(
        digest,
        canonical_task_digest("task", &argv, "tools", None, Some(&changed))?
    );
    assert_ne!(
        digest,
        canonical_task_digest("task", &argv, "tools", None, None)?
    );
    Ok(())
}
