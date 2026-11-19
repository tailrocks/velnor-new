// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[cfg_attr(
    all(target_os = "linux", target_arch = "aarch64"),
    ignore = "old-engine release binaries have no linux-arm64 target; P4 runs velnor-gen workflows (velnor-ci ships linux-arm64)"
)]
#[test]
fn failed_check_streams_sanitized_progress() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let runtime = std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH missing")?)
        .map(|entry| entry.join("mise"))
        .find(|path| path.is_file())
        .ok_or("qualified mise missing")?;
    let (label, platform, target) = host_platform()?;
    let pin = current_mise_binary(target);
    let observed = fs::metadata(&runtime)?.len();
    assert_eq!(
        observed,
        pin.bytes,
        "precondition: the mise on PATH ({}) must be the pinned release {} of {} bytes, found {observed} bytes; install the pinned release with `mise install aqua:jdx/mise@2026.11.1` and put its bin directory first on PATH (`~/.local/share/mise/installs/aqua-jdx-mise/2026.11.1/mise/bin`), because the ~/.local/bin/mise shim is not the pinned binary",
        runtime.display(),
        pin.version,
        pin.bytes
    );
    fs::copy(runtime, fixture.environment.path().join("bin/mise"))?;
    fs::write(
        fixture.root().join(".velnor/controller-approvals.json"),
        compiled_runtime_approvals()?,
    )?;
    fs::write(
        fixture.root().join(".velnor/config.toml"),
        config(label, platform),
    )?;
    fs::write(
        fixture.root().join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"printf 'maintenance-phase: probe # ::warning::danger\\n'; exit 7\"\n",
    )?;
    let head = fixture.head()?;
    let plan = fixture.public_plan(&head)?;
    let result = fixture.execute(&plan, &plan_json_bytes(&plan)?)?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(stderr.contains("maintenance-phase: probe"), "{stderr}");
    assert!(stderr.contains("velnor-progress stdout:"), "{stderr}");
    assert!(!stderr.contains("\n::warning::danger"), "{stderr}");
    assert!(stderr.contains("\\x23"), "{stderr}");
    Ok(())
}
