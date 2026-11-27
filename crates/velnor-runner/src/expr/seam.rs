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
