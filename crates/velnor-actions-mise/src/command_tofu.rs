use std::ffi::OsString;

use crate::error::MiseError;

use super::{
    EnvPolicy, NO_AUTO_INSTALL_ENV, TF_CLI_CONFIG_FILE_ENV, TF_DATA_DIR_ENV, TF_IN_AUTOMATION_ENV,
    TF_IN_AUTOMATION_ON, TF_INPUT_ENV, TF_INPUT_OFF, TF_PLUGIN_CACHE_DIR_ENV, mise_argv_tail,
    pairs_of,
};

impl super::IsolatedCommand {
    pub fn tofu_exec(
        tool_specs: &[String],
        payload: &[OsString],
        data_dir: &str,
        config_file: &str,
        cache_dir: &str,
    ) -> Result<Self, MiseError> {
        if payload.is_empty() {
            return Err(MiseError::EmptyCommand {
                program: "mise".to_owned(),
            });
        }
        for (field, value) in [
            ("tf_data_dir", data_dir),
            ("tf_cli_config_file", config_file),
            ("tf_plugin_cache_dir", cache_dir),
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
                (TF_PLUGIN_CACHE_DIR_ENV, cache_dir),
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
