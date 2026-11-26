// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod checkout;
mod git_mirror;

use crate::exec::AdapterRegistry;

pub(crate) fn registry() -> AdapterRegistry {
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(checkout::Checkout));
    registry
}
