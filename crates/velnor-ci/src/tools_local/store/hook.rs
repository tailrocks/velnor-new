// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::cell::RefCell;
use std::rc::Rc;

use super::Step;

type Hook = Rc<dyn Fn(Step)>;

thread_local! {
    static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
}

pub(super) struct Installed;

impl Drop for Installed {
    fn drop(&mut self) {
        HOOK.with(|hook| *hook.borrow_mut() = None);
    }
}

pub(super) fn install(hook: impl Fn(Step) + 'static) -> Installed {
    HOOK.with(|slot| *slot.borrow_mut() = Some(Rc::new(hook)));
    Installed
}

pub(super) fn reached(step: Step) {
    let hook = HOOK.with(|slot| slot.borrow().clone());
    if let Some(hook) = hook {
        hook(step);
    }
}
