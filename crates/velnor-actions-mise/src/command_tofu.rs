//! Tofu spawn constructor: pinned `mise exec` plus baked isolation env.
//!
//! Declared from `command.rs` (`#[path]`, no `lib.rs` edit). The
//! constructor bakes the reserved `TF_*` isolation pairs directly,
//! since `with_env` (reserved-key fail-loud) can never carry them.

use std::ffi::OsString;

use crate::error::MiseError;

use super::{
    EnvPolicy, NO_AUTO_INSTALL_ENV, TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV,
    TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF, mise_argv_tail, pairs_of,
};

impl super::IsolatedCommand {
    /// Build `mise <globals> exec <specs> -- <payload>` with baked tofu isolation.
    ///
    /// H1 allowlist constructor: the automation pair plus the isolated
    /// data/config paths arrive baked, since the reserved-key rule
    /// rejects them as caller extras. Verify policy plus the
    /// install-disable pair, exactly like [`Self::mise_exec`]:
    /// explicit `opentofu@exact` specs are the sole version authority
    /// and a missing tool fails as a preparation error. Ambient
    /// `TF_*`/`TOFU_*`/`CHECKPOINT_*` never reach the child (spawn
    /// strip); the baked values apply after it. Exits, signals, and
    /// timeouts flow through the shared typed result plus
    /// [`MiseError`] vocabulary; nonzero stays data via the opt-in
    /// `require_success`, never an automatic failure.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::EmptyCommand`] for an empty payload and
    /// [`MiseError::InvalidStepInput`] for an empty data dir or config
    /// file.
    pub fn tofu_exec(
        tool_specs: &[String],
        payload: &[OsString],
        data_dir: &str,
        config_file: &str,
    ) -> Result<Self, MiseError> {
        if payload.is_empty() {
            return Err(MiseError::EmptyCommand {
                program: "mise".to_owned(),
            });
        }
        for (field, value) in [
            ("tf_data_dir", data_dir),
            ("tf_cli_config_file", config_file),
        ] {
            if value.is_empty() {
                return Err(MiseError::InvalidStepInput {
                    field: field.to_owned(),
                    value: value.to_owned(),
                });
            }
        }
        let mut extra_env = pairs_of(&NO_AUTO_INSTALL_ENV);
        extra_env.extend(
            [
                (TF_IN_AUTOMATION_ENV, TF_IN_AUTOMATION_ON),
                (TF_INPUT_ENV, TF_INPUT_OFF),
                (TF_DATA_DIR_ENV, data_dir),
                (TF_CLI_CONFIG_FILE_ENV, config_file),
            ]
            .iter()
            .map(|(key, value)| (OsString::from(key), OsString::from(value))),
        );
        Ok(Self {
            program: OsString::from("mise"),
            args: mise_argv_tail("exec", tool_specs, payload),
            cwd: None,
            extra_env,
            policy: EnvPolicy::Verify,
        })
    }
}
