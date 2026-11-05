// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

const INVENTORY: &str = include_str!("closure_inventory.txt");

static INPUTS: OnceLock<Vec<&str>> = OnceLock::new();

pub fn native_image_candidate_inputs() -> &'static [&'static str] {
    INPUTS
        .get_or_init(|| INVENTORY.lines().filter(|line| !line.is_empty()).collect())
        .as_slice()
}
