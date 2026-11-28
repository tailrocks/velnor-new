// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fmt;
use std::path::Path;
use std::sync::Arc;

type Hook = Arc<dyn Fn(&Path) + Send + Sync>;

#[derive(Clone, Default)]
pub(crate) struct Seam {
    hook: Option<Hook>,
}

impl Seam {
    #[cfg(feature = "test-support")]
    pub(crate) fn new(hook: impl Fn(&Path) + Send + Sync + 'static) -> Self {
        Self {
            hook: Some(Arc::new(hook)),
        }
    }

    pub(super) const fn unhooked() -> Self {
        Self { hook: None }
    }

    pub(super) fn following() -> Self {
        #[cfg(feature = "test-support")]
        if let Some(target) = std::env::var_os("VELNOR_TEST_REMOVE_BEFORE_OPEN") {
            let target = std::path::PathBuf::from(target);
            return Self::new(move |path| {
                if path == target {
                    std::fs::remove_file(path).ok();
                }
            });
        }
        Self::unhooked()
    }

    pub(crate) fn before_open(&self, path: &Path) {
        if let Some(hook) = &self.hook {
            hook(path);
        }
    }
}

impl fmt::Debug for Seam {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Seam")
            .field("armed", &self.hook.is_some())
            .finish()
    }
}
