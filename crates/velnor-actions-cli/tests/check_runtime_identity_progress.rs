// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn timed_out_check_streams_sanitized_progress() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let runtime = std::env::split_paths(&std::env::var_os("PATH").ok_or("PATH missing")?)
        .map(|entry| entry.join("mise"))
        .find(|path| path.is_file())
        .ok_or("qualified mise missing")?;
    fs::copy(runtime, fixture.environment.path().join("bin/mise"))?;
    fs::write(
        fixture.root().join(".velnor/controller-approvals.json"),
        compiled_runtime_approvals()?,
    )?;
    let (label, platform, _) = host_platform()?;
    fs::write(
        fixture.root().join(".velnor/config.toml"),
        config(label, platform).replace("[checks.runner]", "timeout_minutes = 1\n[checks.runner]"),
    )?;
    fs::write(
        fixture.root().join("mise.toml"),
        "[tasks.fixture-check]\nrun = \"printf 'maintenance-phase: probe # ::warning::danger\\n'; sleep 120\"\n",
    )?;
    let head = fixture.head()?;
    let plan = fixture.public_plan(&head)?;
    let result = fixture.execute(&plan, &plan_json_bytes(&plan)?)?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(stderr.contains("maintenance-phase: probe"), "{stderr}");
    assert!(
        stderr.contains("timeout_after_absolute_deadline"),
        "{stderr}"
    );
    assert!(!stderr.contains("\n::warning::danger"), "{stderr}");
    assert!(stderr.contains("\\x23"), "{stderr}");
    Ok(())
}
