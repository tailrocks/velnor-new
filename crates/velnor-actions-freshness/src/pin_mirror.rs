// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;

use crate::context::FreshnessContext;
use crate::inventory;
use crate::pin_diff::{self, BaseRef};

pub(crate) fn run(root: PathBuf, base: Option<&BaseRef>) -> i32 {
    let mut ctx = FreshnessContext::new(root, false, false);
    if !inventory::load_inventory(&mut ctx) {
        return ctx.finish();
    }
    inventory::check_pin_consistency(&mut ctx);
    if let Some(base) = base {
        pin_diff::report(&mut ctx, base);
    }
    ctx.finish()
}
