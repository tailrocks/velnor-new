// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::rules::RenderedStep;

pub(crate) fn mbx_out_dirs_step() -> RenderedStep {
    RenderedStep {
        name: "Keep mbx build-script outputs in target".to_owned(),
        run: Some("echo \"MBX_SHARE_OUT_DIR=0\" >> \"$GITHUB_ENV\"".to_owned()),
        ..RenderedStep::default()
    }
}
