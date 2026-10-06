//! Neutral source projection. Descriptor data never grants runtime authority.

use super::RustupManagerAuthority;
use crate::catalog::rust_desktop::RustCompilerRole;
use serde_json::{Value, json};

/// Exact neutral metadata policy for embedding in an authenticated helper.
///
/// The runtime must authenticate the complete live tools receipt independently
/// before using any path or digest here. JSON deserialization is not admission.
#[must_use]
pub fn descriptor(authority: RustupManagerAuthority) -> Value {
    let qualification = authority.qualification_descriptor();
    let role = match authority.role() {
        RustCompilerRole::RootLinux => "root-linux",
        RustCompilerRole::ReleaseMac => "release-mac",
        RustCompilerRole::DesktopMac => "desktop-mac",
        RustCompilerRole::DesktopSourceMac => "desktop-source-mac",
    };
    let toolchain = authority.selected_toolchain();
    json!({
        "schema": 1,
        "role": role,
        "host": authority.host().target_triple(),
        "manager_version": authority.version(),
        "manager_sha256": authority.sha256(),
        "qualification": {
            "commit": qualification.source_commit(),
            "tree": qualification.source_tree(),
        },
        "toolchain": toolchain,
        "manager": "cargo/bin/rustup",
        "proxy": "cargo/bin/cargo",
        "settings": "rustup/settings.toml",
        "cargo": format!("rustup/toolchains/{toolchain}/bin/cargo"),
        "rustc": format!("rustup/toolchains/{toolchain}/bin/rustc"),
        "rustdoc": format!("rustup/toolchains/{toolchain}/bin/rustdoc"),
    })
}

#[cfg(test)]
mod tests {
    use super::{RustCompilerRole, RustupManagerAuthority, descriptor};

    #[test]
    fn neutral_projection_tracks_each_closed_catalog_role_without_granting_execution() {
        for role in [
            RustCompilerRole::RootLinux,
            RustCompilerRole::ReleaseMac,
            RustCompilerRole::DesktopMac,
            RustCompilerRole::DesktopSourceMac,
        ] {
            let authority = RustupManagerAuthority::for_role(role);
            let value = descriptor(authority);
            assert_eq!(value["schema"], 1);
            assert_eq!(value["manager_sha256"], authority.sha256());
            assert_eq!(value["manager_version"], authority.version());
            assert_eq!(value["host"], authority.host().target_triple());
            assert_eq!(value["toolchain"], authority.selected_toolchain());
            assert_eq!(
                value["qualification"]["commit"],
                authority.qualification_descriptor().source_commit()
            );
            assert_eq!(
                value["qualification"]["tree"],
                authority.qualification_descriptor().source_tree()
            );
            assert_eq!(value["manager"], "cargo/bin/rustup");
            assert_eq!(value["proxy"], "cargo/bin/cargo");
            assert_eq!(value["settings"], "rustup/settings.toml");
            assert_eq!(
                value["cargo"],
                format!(
                    "rustup/toolchains/{}/bin/cargo",
                    authority.selected_toolchain()
                )
            );
            assert_eq!(
                value["rustc"],
                format!(
                    "rustup/toolchains/{}/bin/rustc",
                    authority.selected_toolchain()
                )
            );
            assert_eq!(
                value["rustdoc"],
                format!(
                    "rustup/toolchains/{}/bin/rustdoc",
                    authority.selected_toolchain()
                )
            );
        }
    }
}
