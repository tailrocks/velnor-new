// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{HelperRelease, PrivateSources};
use velnor_release::BinaryPlan;

use crate::rules::RenderedStep;

pub(crate) struct Private<'a> {
    pub(crate) helper: &'a HelperRelease,
    pub(crate) sources: &'a PrivateSources,
}

pub(crate) struct BuildContext<'a> {
    pub(crate) source: &'a str,
    pub(crate) same_commit: RenderedStep,
    pub(crate) tag: &'a str,
    pub(crate) private: Option<Private<'a>>,
    pub(crate) mbx: bool,
}

pub(crate) fn package_tag(binary: &BinaryPlan) -> String {
    format!("${{{{ needs.tag.outputs.{} }}}}", binary.package)
}
