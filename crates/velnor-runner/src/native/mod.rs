// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod checkout;
mod depth;

use crate::broker::StoreWriter;
use crate::exec::AdapterRegistry;

pub(crate) fn registry(store: StoreWriter) -> AdapterRegistry {
    let mut registry = AdapterRegistry::empty();
    registry.register(Box::new(checkout::Checkout { store }));
    registry
}
