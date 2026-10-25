use super::{ExecutionConfig, ExecutionMode, ProfileKind};
use crate::config::SCALE_SET_PROFILE_ID;

#[test]
fn optional_scale_set_image_source_preserves_hosted_and_ordinary_scale_set_config() {
    let hosted = ExecutionConfig::hosted_default("ubuntu-26.04").expect("hosted config");
    assert_eq!(hosted.scale_set_image_source_sha(), None);
    hosted
        .validate("config.toml")
        .expect("hosted remains valid");

    let mut scale_set = hosted;
    scale_set.mode = Some(ExecutionMode::ScaleSet);
    scale_set
        .validate("config.toml")
        .expect("ordinary scale-set mode needs no image source pin");
    assert!(scale_set.emits_scale_set_selector());
}

#[test]
fn scale_set_image_source_must_be_lowercase_full_sha_and_hosted_cannot_set_it() {
    let mut config = ExecutionConfig::hosted_default("ubuntu-26.04").expect("hosted config");
    let scale = config
        .profiles
        .get_mut(SCALE_SET_PROFILE_ID)
        .expect("scale-set profile");
    scale.image_source_sha = Some("a".repeat(40));
    config.validate("config.toml").expect("full lowercase SHA");
    assert_eq!(
        config.scale_set_image_source_sha(),
        Some("a".repeat(40).as_str())
    );

    for invalid in ["A".repeat(40), "a".repeat(39), "a".repeat(41)] {
        let mut invalid_config = config.clone();
        invalid_config
            .profiles
            .get_mut(SCALE_SET_PROFILE_ID)
            .expect("scale-set profile")
            .image_source_sha = Some(invalid);
        assert!(invalid_config.validate("config.toml").is_err());
    }

    let mut hosted_source = config;
    hosted_source
        .profiles
        .get_mut("hosted")
        .expect("hosted profile")
        .image_source_sha = Some("b".repeat(40));
    assert!(hosted_source.validate("config.toml").is_err());
    assert_eq!(
        hosted_source.profiles["local"].kind,
        ProfileKind::GithubScaleSet
    );
}
