// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod cache;
mod checkout;
mod composite;
mod depth;
mod git;
mod mise;
mod mise_lock;
mod mise_policy;
mod umask;

pub(crate) use umask::Umask;

use crate::broker::StoreWriter;
use crate::exec::AdapterRegistry;

pub(crate) fn registry(store: StoreWriter, umask: Umask) -> AdapterRegistry {
    let store = std::sync::Arc::new(store);
    let mut registry = AdapterRegistry::empty();
    mise::register(&mut registry, store.facts().cloned());
    registry.register(Box::new(checkout::Checkout {
        store: store.clone(),
    }));
    registry.register(Box::new(cache::CacheAdapter::new(store, umask)));
    registry
}
