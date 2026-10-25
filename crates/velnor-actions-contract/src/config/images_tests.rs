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

