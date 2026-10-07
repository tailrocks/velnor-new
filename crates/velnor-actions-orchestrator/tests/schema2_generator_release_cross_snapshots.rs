use super::action_snapshots::{Actions, action};
use velnor_actions_mise::catalog::{
    ACTIONLINT_VERSION, RUST_VERSION, SHELLCHECK_VERSION, ZIZMOR_VERSION,
};
use velnor_actions_workflow_renderer::setup::{
    MISE_BINARY_SHA256_MACOS_ARM64, MISE_BINARY_SHA256_MACOS_X64,
};

/// The Intel leg builds on ARM runners and qualifies natively on Intel.
pub(super) fn assert_intel_cross_build(
    body: &str,
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let intel = super::super::job_body(body, "build-macos-intel")?;
    assert!(intel.contains("runs-on: macos-15\n"), "{intel}");
    let intel_action = action(actions, "generator-release-build-macos-intel")?;
    assert!(intel_action.contains("*Mach-O*x86_64*"), "{intel_action}");
    assert!(intel_action.contains("shasum -a 256"), "{intel_action}");
    assert!(
        intel_action.contains("--target x86_64-apple-darwin"),
        "{intel_action}"
    );
    assert!(
        intel_action.contains("target/x86_64-apple-darwin/release/velnor-actions"),
        "{intel_action}"
    );
    assert!(
        !intel_action.contains("cp target/release/velnor-actions"),
        "{intel_action}"
    );
    assert!(
        intel_action.contains(
            "rustup target add --toolchain 1.98.1-aarch64-apple-darwin x86_64-apple-darwin"
        ),
        "{intel_action}"
    );
    assert!(
        intel_action.contains(&format!("sha256: {MISE_BINARY_SHA256_MACOS_ARM64}")),
        "{intel_action}"
    );
    assert!(
        !intel_action.contains(MISE_BINARY_SHA256_MACOS_X64),
        "{intel_action}"
    );
    let qualify = super::super::job_body(body, "qualify-macos-intel")?;
    assert!(qualify.contains("runs-on: macos-15-intel\n"), "{qualify}");
    Ok(())
}

/// Every GitHub-CLI-bearing action carries the portable watchdog, never GNU `timeout`.
///
/// GH-hosted macOS runners ship neither `timeout` nor `gtimeout`, so the
/// emitted `gh()` wrapper bounds each call with a pure-bash watchdog: TERM
/// at 60 s, KILL 5 s later, and the command's own exit status otherwise.
pub(super) fn assert_portable_gh_watchdog(actions: &Actions) {
    let mut checked = 0;
    for (name, body) in actions {
        if !["gh api ", "gh release ", "gh attestation "]
            .iter()
            .any(|needle| body.contains(needle))
        {
            continue;
        }
        checked += 1;
        assert!(
            !body.contains("timeout --signal"),
            "{name} invokes GNU timeout, which macOS runners lack: {body}"
        );
        for marker in [
            "( sleep 60; kill -TERM",
            "sleep 5; kill -KILL",
            "</dev/null >/dev/null 2>&1 & _velnor_gh_watch=$!",
            r#"wait \"$_velnor_gh_pid\" || _velnor_gh_status=$?"#,
            r#"exit \"$_velnor_gh_status\""#,
            ") </dev/null",
        ] {
            assert!(
                body.contains(marker),
                "{name} lost watchdog marker {marker}: {body}"
            );
        }
    }
    assert!(checked > 0, "no GitHub-CLI-bearing action to check");
}

/// Every qualify action installs the exact tools `generate` validates with.
///
/// `validate_staged` fails closed when actionlint, shellcheck, or zizmor
/// are missing, and the Mise setup step never installs (`install: "false"`),
/// so the composite action installs the pinned qualification tools itself
/// before downloading the candidate.
pub(super) fn assert_qualify_install_tools(
    actions: &Actions,
) -> Result<(), Box<dyn std::error::Error>> {
    let run = format!(
        "mise --no-config --no-env --no-hooks install rust@{RUST_VERSION} actionlint@{ACTIONLINT_VERSION} shellcheck@{SHELLCHECK_VERSION} zizmor@{ZIZMOR_VERSION}"
    );
    for name in [
        "generator-release-qualify-linux",
        "generator-release-qualify-macos",
        "generator-release-qualify-macos-intel",
    ] {
        let body = action(actions, name)?;
        let setup = body.find("Setup Mise").ok_or("missing Mise setup step")?;
        let install = body
            .find("Install pinned qualification tools")
            .ok_or("missing pinned qualification tools install")?;
        let download = body
            .find("Download built asset archive")
            .ok_or("missing built asset archive download")?;
        assert!(
            setup < install && install < download,
            "{name}: install step must follow Mise setup and precede the download: {body}"
        );
        assert!(
            body.contains(&run),
            "{name}: install step must carry the pinned qualification specs: {body}"
        );
        assert!(
            body.contains("install: \"false\""),
            "{name}: Mise setup must stay install-free: {body}"
        );
    }
    Ok(())
}
