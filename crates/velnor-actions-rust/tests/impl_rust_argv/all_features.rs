//! Cargo all-features mode replaces every other feature flag.

use super::{group, text};
use velnor_actions_contract::ContractError;
use velnor_actions_rust::tasks::TaskKind;

#[test]
fn all_features_is_the_only_feature_mode_for_every_compiling_task() -> Result<(), ContractError> {
    for kind in [
        TaskKind::Clippy,
        TaskKind::Test,
        TaskKind::Nextest,
        TaskKind::Doctest,
        TaskKind::Doc,
        TaskKind::Build,
    ] {
        let mut group = group(kind);
        group.all_features = true;
        group.features = vec!["declared-a".to_owned(), "declared-b".to_owned()];
        let argv = text(&group)?;
        assert_eq!(
            argv.iter()
                .filter(|arg| arg.as_str() == "--all-features")
                .count(),
            1,
            "all-features mode for {kind:?}: {argv:?}"
        );
        assert!(
            !argv.contains(&"--no-default-features".to_owned()),
            "{argv:?}"
        );
        assert!(!argv.contains(&"--features".to_owned()), "{argv:?}");
    }

    let mut fmt = group(TaskKind::Fmt);
    fmt.all_features = true;
    let argv = text(&fmt)?;
    assert!(
        !argv.contains(&"--all-features".to_owned()),
        "fmt ignores Cargo features: {argv:?}"
    );
    Ok(())
}
