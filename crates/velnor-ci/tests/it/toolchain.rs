// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::Path;
use std::sync::OnceLock;

pub(crate) fn pinned_channel() -> &'static str {
    static CHANNEL: OnceLock<String> = OnceLock::new();
    CHANNEL.get_or_init(|| {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.velnor/rust-toolchain.toml");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| unreachable!("cannot read {}: {e}", path.display()));
        let pin: toml::Table =
            toml::from_str(&text).unwrap_or_else(|e| unreachable!("{}: {e}", path.display()));
        pin["toolchain"]["channel"]
            .as_str()
            .unwrap_or_else(|| unreachable!("{} has no [toolchain] channel", path.display()))
            .to_owned()
    })
}

pub(crate) fn mise_rust_pin() -> String {
    format!("[tools]\nrust = \"{}\"\n", pinned_channel())
}
