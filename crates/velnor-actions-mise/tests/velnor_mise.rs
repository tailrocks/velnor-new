// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod checkout_history_head_reads;
#[cfg(unix)]
mod command_stream;
mod gc1_t11;
mod gc1_t17;
mod gc1_t18;
mod install_spec_subject;
mod metadata_cwd;
#[cfg(target_os = "macos")]
mod permission_retry_darwin;
mod rust_spec_kind;
mod source_preview_lock;
mod source_preview_runtime_command;

#[test]
fn job_build_command_binds_validated_source_revision() -> Result<(), Box<dyn std::error::Error>> {
    let source = "a".repeat(40);
    let command = velnor_actions_mise::ImageCandidateCommand::build_oci(
        std::path::Path::new("/tmp/velnor-image-context"),
        std::path::Path::new("/tmp/velnor-image.tar"),
        std::path::Path::new("/tmp/velnor-docker-config"),
        &source,
    )?;
    let argv = command.argv();
    let expected_argument = format!("VELNOR_SOURCE_SHA={source}");
    assert!(argv.windows(2).any(|arguments| {
        arguments[0] == "--build-arg" && arguments[1].to_str() == Some(expected_argument.as_str())
    }));
    assert!(
        velnor_actions_mise::ImageCandidateCommand::build_oci(
            std::path::Path::new("/tmp/velnor-image-context"),
            std::path::Path::new("/tmp/velnor-image.tar"),
            std::path::Path::new("/tmp/velnor-docker-config"),
            "not-a-source-sha",
        )
        .is_err()
    );
    Ok(())
}
