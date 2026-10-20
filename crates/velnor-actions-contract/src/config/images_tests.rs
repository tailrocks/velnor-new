use super::image_grammar::is_valid_image_repository;
use super::images::{ImageConfig, ImageDockerfile, ImagePlatform, ImagePrebuilt, validate_images};
use std::collections::BTreeMap;

fn image(id: &str) -> ImageConfig {
    ImageConfig {
        id: id.to_owned(),
        repository: format!("ghcr.io/chainargos/{id}"),
        dockerfile: ImageDockerfile::Single(format!("{id}/Dockerfile")),
        context: ".".to_owned(),
        platforms: vec![ImagePlatform::LinuxAmd64, ImagePlatform::LinuxArm64],
        version_file: Some(format!("{id}/Cargo.toml")),
        prebuilt: Some(ImagePrebuilt::CargoBin {
            package: id.to_owned(),
            bin: id.to_owned(),
            stage: format!("dist/{id}"),
        }),
        timeout_minutes: 30,
    }
}

fn problem(image: &ImageConfig) -> String {
    image
        .validate("config.toml", 0)
        .expect_err("expected rejection")
        .to_string()
}

#[test]
fn accepts_the_documented_shape() {
    assert!(image("eth-processor-app").validate("c", 0).is_ok());
    assert!(validate_images(&[image("a-one"), image("b-two")], "c").is_ok());
}

#[test]
fn parses_single_and_per_platform_dockerfiles_strictly() {
    let json = serde_json::json!({
        "id": "dbt",
        "repository": "ghcr.io/chainargos/dbt",
        "platforms": ["linux_amd64", "linux_arm64"],
        "dockerfile": {"linux_amd64": "d/Dockerfile.amd64", "linux_arm64": "d/Dockerfile.arm64"},
        "context": "d"
    });
    let parsed: ImageConfig = serde_json::from_value(json.clone()).expect("parse");
    assert!(parsed.validate("c", 0).is_ok());
    assert_eq!(
        parsed.dockerfile.for_platform(ImagePlatform::LinuxArm64),
        Some("d/Dockerfile.arm64")
    );
    let mut unknown = json;
    unknown["extra"] = serde_json::json!(1);
    assert!(serde_json::from_value::<ImageConfig>(unknown).is_err());
}

#[test]
fn parses_the_closed_prebuilt_union_and_rejects_unknown_kinds() {
    let base = |prebuilt: serde_json::Value| {
        serde_json::json!({
            "id": "a", "repository": "ghcr.io/o/a", "platforms": ["linux_amd64"],
            "dockerfile": "Dockerfile", "prebuilt": prebuilt
        })
    };
    let ok = base(serde_json::json!({
        "kind": "cargo_bin", "package": "a", "bin": "a", "stage": "dist/a"
    }));
    assert!(serde_json::from_value::<ImageConfig>(ok).is_ok());
    let gradle = base(serde_json::json!({"kind": "gradle_task"}));
    assert!(serde_json::from_value::<ImageConfig>(gradle).is_err());
}

#[test]
fn repository_must_be_lowercase_ghcr_owner_and_name() {
    for good in ["ghcr.io/o/a", "ghcr.io/chain-args/a.b_c", "ghcr.io/o/a/b"] {
        assert!(is_valid_image_repository(good), "{good}");
    }
    for bad in [
        "docker.io/o/a",
        "ghcr.io/o",
        "ghcr.io/O/a",
        "ghcr.io/o/A",
        "ghcr.io/o/a:tag",
        "ghcr.io/o/a@sha256:00",
        "ghcr.io//a",
        "ghcr.io/-o/a",
        "ghcr.io/o/a..b",
        "ghcr.io/o/a/",
    ] {
        assert!(!is_valid_image_repository(bad), "{bad}");
    }
}

#[test]
fn rejects_bad_ids_platforms_and_paths() {
    let mut bad = image("ok-id");
    bad.id = "Required".to_owned();
    assert!(problem(&bad).contains("bad_image_id"));
    bad = image("required");
    assert!(problem(&bad).contains("bad_image_id"));
    bad = image("ok-id");
    bad.platforms = vec![];
    assert!(problem(&bad).contains("empty_image_platforms"));
    bad.platforms = vec![ImagePlatform::LinuxArm64, ImagePlatform::LinuxAmd64];
    assert!(problem(&bad).contains("not_sorted_unique"));
    for path in [
        "../x/Dockerfile",
        "/abs",
        "a//b",
        "a/./b",
        "a b",
        "a/${{ x }}",
        "",
    ] {
        bad = image("ok-id");
        bad.dockerfile = ImageDockerfile::Single(path.to_owned());
        assert!(problem(&bad).contains("bad_image_path"), "{path:?}");
    }
}

#[test]
fn per_platform_dockerfiles_must_match_platforms() {
    let mut bad = image("ok-id");
    bad.dockerfile = ImageDockerfile::PerPlatform(BTreeMap::from([(
        ImagePlatform::LinuxAmd64,
        "d/Dockerfile.amd64".to_owned(),
    )]));
    assert!(problem(&bad).contains("image_dockerfile_platforms_mismatch"));
}

#[test]
fn stage_must_live_inside_a_non_root_context() {
    let mut bad = image("ok-id");
    bad.context = "svc".to_owned();
    assert!(problem(&bad).contains("image_stage_outside_context"));
    bad.prebuilt = Some(ImagePrebuilt::CargoBin {
        package: "a".to_owned(),
        bin: "a".to_owned(),
        stage: "svc/dist/a".to_owned(),
    });
    assert!(bad.validate("c", 0).is_ok());
}

#[test]
fn inventory_rejects_duplicates() {
    let mut second = image("other");
    second.id = "ok-id".to_owned();
    assert!(validate_images(&[image("ok-id"), second], "c").is_err());
    let mut third = image("third");
    third.repository = image("fourth").repository;
    assert!(validate_images(&[image("fourth"), third], "c").is_err());
}

#[test]
fn job_ids_are_deterministic() {
    let image = image("eth");
    assert_eq!(
        image.build_job_id(ImagePlatform::LinuxArm64),
        "image-eth-arm64"
    );
    assert_eq!(image.publish_job_id(), "image-publish-eth");
}

#[test]
fn timeout_is_bounded() {
    let mut bad = image("ok-id");
    for minutes in [0, 361] {
        bad.timeout_minutes = minutes;
        assert!(problem(&bad).contains("bad_image_timeout"));
    }
    bad.timeout_minutes = 360;
    assert!(bad.validate("c", 0).is_ok());
}

#[test]
fn version_file_must_be_a_supported_manifest() {
    let mut bad = image("demo");
    bad.version_file = Some("demo/VERSION".to_owned());
    assert!(problem(&bad).contains("unsupported_image_version_file"));
    for ok in ["demo/Cargo.toml", "g/gradle.properties", "web/package.json"] {
        let mut good = image("demo");
        good.version_file = Some(ok.to_owned());
        assert!(good.validate("c", 0).is_ok(), "{ok}");
    }
}

#[test]
fn generated_job_ids_never_collide_across_images() {
    // Build job `image-publish-foo-amd64` equals the publish job of `foo-amd64`.
    let error = validate_images(&[image("foo-amd64"), image("publish-foo")], "c")
        .expect_err("collision")
        .to_string();
    assert!(
        error.contains("image_job_id_collision:image-publish-foo-amd64"),
        "{error}"
    );
    assert!(validate_images(&[image("foo"), image("bar")], "c").is_ok());
}
