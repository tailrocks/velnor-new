// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;

use super::{
    crane, pinned_crane_version,
    registry::{ManifestFault, Registry},
};

#[test]
fn pinned_crane_manifest_tag_lookup_captures_registry_outcomes() -> Result<(), Box<dyn Error>> {
    let version = pinned_crane_version()?;
    eprintln!("Crane manifest tag lookup probe uses pinned version {version}");
    for (name, fault) in [
        ("missing", ManifestFault::None),
        ("unauthorized", ManifestFault::Unauthorized),
        ("transport", ManifestFault::Transport),
    ] {
        let registry = Registry::with_manifest_fault(fault)?;
        let reference = registry.reference(name);
        let output = crane(&["manifest", "--insecure", &reference])?;
        eprintln!(
            "manifest tag {name}: code={:?} signal={:?} success={} stdout={:?} stderr={:?}",
            output.code,
            output.signal,
            output.success,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !output.success,
            "crane manifest accepted {name} fixture outcome"
        );
        registry.require_healthy()?;
    }
    Ok(())
}
