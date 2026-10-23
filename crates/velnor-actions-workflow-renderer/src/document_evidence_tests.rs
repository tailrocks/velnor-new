use std::collections::BTreeMap;

use super::internal_env;
use crate::Yaml;

fn specification() -> BTreeMap<String, String> {
    BTreeMap::from([(
        velnor_actions_contract::GITHUB_EVIDENCE_SPEC_ENV.to_owned(),
        "{}".to_owned(),
    )])
}

#[test]
fn evidence_environment_projection_accepts_only_exact_fetch_and_import_bindings() {
    let mut fetch = specification();
    fetch.insert(
        velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV.to_owned(),
        "${{ github.token }}".to_owned(),
    );
    assert!(matches!(
        internal_env(
            velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION,
            &fetch
        ),
        Ok(Some(Yaml::Map(_)))
    ));
    assert!(matches!(
        internal_env(
            velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION,
            &specification()
        ),
        Ok(Some(Yaml::Map(_)))
    ));

    fetch.insert("EXTRA_TOKEN".to_owned(), "secret".to_owned());
    assert!(
        internal_env(
            velnor_actions_contract::GITHUB_EVIDENCE_FETCH_OPERATION,
            &fetch
        )
        .is_err()
    );
    let mut import = specification();
    import.insert(
        velnor_actions_contract::GITHUB_EVIDENCE_TOKEN_ENV.to_owned(),
        "${{ github.token }}".to_owned(),
    );
    assert!(
        internal_env(
            velnor_actions_contract::GITHUB_EVIDENCE_IMPORT_OPERATION,
            &import
        )
        .is_err()
    );
    assert!(
        internal_env("unknown-operation", &BTreeMap::new())
            .expect("not an evidence operation")
            .is_none()
    );
}
