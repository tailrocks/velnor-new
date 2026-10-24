use std::{ffi::OsString, path::Path};

use super::ImageCandidateCommand;

#[test]
fn docker_commands_bind_private_config_and_ignore_ambient_value() {
    let private = Path::new("/runner/temp/private/docker-config");
    let command = ImageCandidateCommand::buildx_version(private).expect("fixed Docker command");
    let parent = vec![(
        OsString::from("DOCKER_CONFIG"),
        OsString::from("/ambient/docker-config"),
    )];
    let effective = command.spawn_env(&parent);
    assert_eq!(
        effective
            .iter()
            .find(|(key, _)| key == "DOCKER_CONFIG")
            .map(|(_, value)| value.as_os_str()),
        Some(private.as_os_str())
    );
    assert!(
        command
            .argv()
            .iter()
            .all(|argument| argument != "DOCKER_CONFIG")
    );
}

#[test]
fn plugin_inspection_and_build_use_the_same_private_config() {
    let private = Path::new("/runner/temp/private/docker-config");
    for command in [
        ImageCandidateCommand::buildx_plugin_info(private).expect("plugin inspection command"),
        ImageCandidateCommand::build_oci(
            Path::new("/runner/temp/context"),
            Path::new("/runner/temp/image.oci.tar"),
            private,
        )
        .expect("build command"),
    ] {
        assert!(
            command
                .full_env()
                .iter()
                .any(|(key, value)| { key == "DOCKER_CONFIG" && value == private.as_os_str() })
        );
    }
}
