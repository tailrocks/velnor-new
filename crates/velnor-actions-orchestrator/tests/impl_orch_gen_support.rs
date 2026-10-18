//! Test fixtures for the regeneration-stability integration cases.

use velnor_actions_mise::catalog::RUST_VERSION;

/// Tool files plus current, repository-qualified contents for preservation cases.
pub(super) fn tool_contents() -> [(&'static str, String); 4] {
    [
        ("mise.toml", "[tools]\n".to_owned()),
        (".mise.toml", "[tools]\n".to_owned()),
        ("mise.lock", "{}\n".to_owned()),
        (
            "rust-toolchain.toml",
            format!("[toolchain]\nchannel = \"{RUST_VERSION}\"\n"),
        ),
    ]
}
