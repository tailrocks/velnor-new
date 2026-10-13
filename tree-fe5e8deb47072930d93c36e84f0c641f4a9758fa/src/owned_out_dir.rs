//! Generated-output ownership requirements in native rustc predictions.
use crate::RustcInputPrediction;

impl RustcInputPrediction {
    /// Whether native workspace restoration needs successful OUT_DIR ownership
    /// evidence. Reading the environment alone can embed paths into artifacts,
    /// even when no generated file appears in dep-info.
    pub fn requires_owned_out_dir(&self) -> bool {
        self.environment.iter().any(|name| name == "OUT_DIR")
            || self
                .inputs
                .iter()
                .any(|path| path == "${out_dir}" || path.starts_with("${out_dir}/"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prediction(inputs: &[&str], environment: &[&str]) -> RustcInputPrediction {
        RustcInputPrediction {
            version: 4,
            inputs: inputs.iter().map(|value| (*value).to_owned()).collect(),
            environment: environment
                .iter()
                .map(|value| (*value).to_owned())
                .collect(),
            compiler_duration_ns: 0,
            crate_name: "fixture".into(),
        }
    }

    #[test]
    fn environment_only_generated_paths_need_owner_evidence() {
        assert!(prediction(&["${workspace}/src/main.rs"], &["OUT_DIR"]).requires_owned_out_dir());
    }

    #[test]
    fn normalized_generated_inputs_need_owner_evidence() {
        assert!(prediction(&["${out_dir}/generated.rs"], &[]).requires_owned_out_dir());
        assert!(!prediction(&["${out_dir_other}/generated.rs"], &[]).requires_owned_out_dir());
        assert!(
            !prediction(&["${workspace}/src/main.rs"], &["OUT_DIR_OTHER"]).requires_owned_out_dir()
        );
    }
}
