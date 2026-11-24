// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::{Arc, Mutex, PoisonError, Weak};

use crate::resolve_world::{Shas, make_repo};
use crate::support::fresh_dir;

pub(crate) struct Template {
    pub(crate) root: velnor_test_support::ScratchDir,
    pub(crate) shas: Shas,
}

static TEMPLATE: Mutex<Weak<Template>> = Mutex::new(Weak::new());

pub(crate) fn shared_template() -> Arc<Template> {
    let mut cached = TEMPLATE.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(live) = cached.upgrade() {
        return live;
    }
    let root = fresh_dir("velnor-ci-resolve", "template");
    let shas = make_repo(&root.join("repo"));
    let built = Arc::new(Template { root, shas });
    *cached = Arc::downgrade(&built);
    built
}
