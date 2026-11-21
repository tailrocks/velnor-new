// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

const PIN: &str = include_str!("../../../../.velnor/rust-toolchain.toml");

pub(crate) fn pinned_channel() -> &'static str {
    static CHANNEL: OnceLock<String> = OnceLock::new();
    CHANNEL.get_or_init(|| {
        let pin: toml::Table = toml::from_str(PIN).unwrap_or_else(|e| unreachable!("{e}"));
        pin["toolchain"]["channel"]
            .as_str()
            .unwrap_or_else(|| unreachable!("pinned toolchain has no [toolchain] channel"))
            .to_owned()
    })
}

pub(crate) fn mise_rust_pin() -> String {
    format!("[tools]\nrust = \"{}\"\n", pinned_channel())
}
