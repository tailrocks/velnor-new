// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{RenderError, ambient_shell_step};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::collections::BTreeMap;
use velnor_actions_contract::{Step, StepId, StepRole};

pub fn capture_step() -> Result<Step, RenderError> {
    let script = STANDARD.encode(include_bytes!("release_budget.py"));
    let command = format!(
        "set -o pipefail; printf %s {script} | /usr/bin/base64 --decode | /usr/bin/python3 -I -"
    );
    let mut step = ambient_shell_step(
        "Capture release budget",
        vec!["bash".to_owned(), "-c".to_owned(), command],
        BTreeMap::from([
            ("GH_TOKEN".to_owned(), "${{ github.token }}".to_owned()),
            ("LD_PRELOAD".to_owned(), String::new()),
            ("LD_AUDIT".to_owned(), String::new()),
            ("GLIBC_TUNABLES".to_owned(), String::new()),
            (
                "SSL_CERT_FILE".to_owned(),
                "/etc/ssl/certs/ca-certificates.crt".to_owned(),
            ),
            ("SSL_CERT_DIR".to_owned(), "/etc/ssl/certs".to_owned()),
            ("LD_LIBRARY_PATH".to_owned(), String::new()),
            ("BASH_ENV".to_owned(), String::new()),
            ("ENV".to_owned(), String::new()),
        ]),
    )?;
    step.id = Some(StepId::ReleaseBudget);
    step.role = Some(StepRole::ReleaseBudget);
    Ok(step)
}
