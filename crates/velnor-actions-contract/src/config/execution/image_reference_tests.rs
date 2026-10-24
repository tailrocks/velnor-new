use super::ValidatedImageReference;
use crate::config::{ExecutionConfig, ExecutionMode, SCALE_SET_PROFILE_ID};

fn digest() -> String {
    "a".repeat(64)
}

#[test]
fn accepts_docker_names_ports_ipv6_and_digest_qualified_tags() {
    for name in [
        "ubuntu",
        "org/repo__name",
        "org/repo--name",
        "REGISTRY/repo",
        "Registry/repo",
        "docker.io/library/ubuntu",
        "registry.example:5000/team/image",
        "registry.example:99999/team/image",
        "localhost:5000/team/image:Release_1",
        "[2001:db8::1]:5000/team/image",
    ] {
        let reference = format!("{name}@sha256:{}", digest());
        let parsed = ValidatedImageReference::parse(&reference).expect("valid image reference");
        assert_eq!(parsed.as_str(), reference);
    }
}

#[test]
fn rejects_mutable_malformed_and_command_like_references() {
    for reference in [
        "",
        "ubuntu:latest",
        "ubuntu@sha512:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "ubuntu@sha256:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        "ubuntu@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "Upper/Image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "REGISTRY/Repo@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "repo//image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "org/repo___name@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "registry.example:/team/image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "registry.example:port/team/image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "[not-ipv6]/team/image@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "ubuntu@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa extra",
        "${{ inputs.image }}@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "ubuntu;echo@sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    ] {
        assert!(
            ValidatedImageReference::parse(reference).is_err(),
            "{reference:?}"
        );
    }
}

#[test]
fn serde_rejects_invalid_values_and_roundtrips_valid_values() {
    let reference = format!(
        "registry.example:5000/team/image:stable@sha256:{}",
        digest()
    );
    let parsed: ValidatedImageReference =
        serde_json::from_str(&serde_json::to_string(&reference).expect("encode string"))
            .expect("Read the valid image reference.");
    assert_eq!(parsed.as_str(), reference);
    for name in ["REGISTRY/repo", "Registry/repo"] {
        let reference = format!("{name}@sha256:{}", digest());
        let json = serde_json::to_string(&reference).expect("encode string");
        let parsed: ValidatedImageReference =
            serde_json::from_str(&json).expect("Read the valid image reference.");
        assert_eq!(parsed.as_str(), reference);
    }
    let upper_repository = format!("REGISTRY/Repo@sha256:{}", digest());
    assert!(
        serde_json::from_str::<ValidatedImageReference>(
            &serde_json::to_string(&upper_repository).expect("encode string")
        )
        .is_err()
    );
    assert!(serde_json::from_str::<ValidatedImageReference>("\"ubuntu:latest\"").is_err());
}

#[test]
fn optional_profile_image_keeps_hosted_shape_and_image_source_separate() {
    let mut config = ExecutionConfig::hosted_default("ubuntu-26.04").expect("hosted config");
    let hosted_json = serde_json::to_value(&config).expect("serialize hosted config");
    assert!(
        hosted_json["profiles"]["hosted"]
            .get("job_container_image")
            .is_none()
    );
    assert!(
        hosted_json["profiles"]["local"]
            .get("job_container_image")
            .is_none()
    );
    let decoded: ExecutionConfig =
        serde_json::from_value(hosted_json.clone()).expect("Read the hosted configuration.");
    assert_eq!(decoded, config);
    let mut invalid_json = hosted_json;
    invalid_json["profiles"]["local"]["job_container_image"] =
        serde_json::Value::String("ubuntu:latest".to_owned());
    assert!(serde_json::from_value::<ExecutionConfig>(invalid_json).is_err());

    config.mode = Some(ExecutionMode::ScaleSet);
    let image = ValidatedImageReference::parse(&format!(
        "ghcr.io/tailrocks/velnor-job@sha256:{}",
        digest()
    ))
    .expect("valid image");
    config
        .profiles
        .get_mut(SCALE_SET_PROFILE_ID)
        .expect("scale-set profile")
        .job_container_image = Some(image.clone());
    config
        .profiles
        .get_mut(SCALE_SET_PROFILE_ID)
        .expect("scale-set profile")
        .image_source_sha = Some("b".repeat(40));
    config.validate("config.toml").expect("valid profile");
    let decoded: ExecutionConfig =
        serde_json::from_value(serde_json::to_value(&config).expect("serialize config"))
            .expect("Read the configuration with its image reference.");
    let profile = &decoded.profiles[SCALE_SET_PROFILE_ID];
    assert_eq!(profile.job_container_image.as_ref(), Some(&image));
    assert_eq!(
        profile.image_source_sha.as_deref(),
        Some("b".repeat(40).as_str())
    );
}
