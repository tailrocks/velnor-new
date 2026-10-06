//! Closed declaration for a hosted job that builds a native-platform image.

use super::workflow_task::is_valid_workflow_task_id;
use crate::errors::ContractError;
use serde::{Deserialize, Serialize};

/// One image build attached to the shared workflow task graph.
///
/// The enclosing `WorkflowTask` owns the `kind = "native-image"`
/// discriminator and the workflow owns ordering and uniqueness across every
/// task variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeImageTask {
    /// Stable identifier shared with all other workflow task variants.
    pub id: String,
    /// Native OS and architecture to build.
    pub platform: NativeImagePlatform,
    /// Repository-owned validation script; filesystem checks belong to the resolver.
    pub script: String,
    /// Required per-job timeout in minutes.
    pub timeout_minutes: u16,
    /// Cache lifetime and owner for the native image builder.
    pub cache: NativeImageCachePolicy,
}

/// Closed native image platform catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeImagePlatform {
    /// Native Linux ARM64 image built on a hosted ARM64 runner.
    LinuxArm64,
}

/// Closed policy for native image task cache ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeImageCachePolicy {
    /// The task creates a run/attempt/task-specific Buildx builder and removes
    /// that exact builder after success, failure, or cancellation.
    TaskOwnedBuilder,
}

impl NativeImagePlatform {
    /// Exact GitHub-hosted runner label.
    #[must_use]
    pub const fn runs_on(self) -> &'static str {
        match self {
            Self::LinuxArm64 => "ubuntu-26.04-arm",
        }
    }

    /// OCI platform string passed to Buildx.
    #[must_use]
    pub const fn oci_platform(self) -> &'static str {
        match self {
            Self::LinuxArm64 => "linux/arm64",
        }
    }
}

impl NativeImageTask {
    /// Validate shared task identity and image-specific declaration fields.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        if !is_valid_workflow_task_id(&self.id) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.id",
                format!("bad_workflow_task_id:{}", self.id),
            ));
        }
        if !is_safe_native_image_script_path(&self.script) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.script",
                format!("bad_native_image_script:{}", self.script),
            ));
        }
        if !(1..=60).contains(&self.timeout_minutes) {
            return Err(ContractError::config(
                file,
                "workflow.tasks.timeout_minutes",
                format!("bad_native_image_timeout:{}", self.timeout_minutes),
            ));
        }
        Ok(())
    }
}

/// Accept an argv-safe shell script path lexically contained below the owned directory.
///
/// This deliberately does not inspect the filesystem. The resolver must reject
/// symlinks in every path component and require the final tracked file to exist.
#[must_use]
fn is_safe_native_image_script_path(path: &str) -> bool {
    const PREFIX: &str = "maintained-image-build/";
    let Some(relative) = path.strip_prefix(PREFIX) else {
        return false;
    };
    let is_shell = std::path::Path::new(relative)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("sh"));
    if !is_shell {
        return false;
    }
    relative.split('/').all(|component| {
        !component.is_empty()
            && component != "."
            && component != ".."
            && component
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    })
}

#[cfg(test)]
mod tests {
    use super::{
        NativeImageCachePolicy, NativeImagePlatform, NativeImageTask,
        is_safe_native_image_script_path,
    };

    fn task(id: &str, script: &str, timeout_minutes: u16) -> NativeImageTask {
        NativeImageTask {
            id: id.to_owned(),
            platform: NativeImagePlatform::LinuxArm64,
            script: script.to_owned(),
            timeout_minutes,
            cache: NativeImageCachePolicy::TaskOwnedBuilder,
        }
    }

    #[test]
    fn platform_has_exact_runner_and_oci_target() {
        let platform = NativeImagePlatform::LinuxArm64;
        assert_eq!(platform.runs_on(), "ubuntu-26.04-arm");
        assert_eq!(platform.oci_platform(), "linux/arm64");
    }

    #[test]
    fn script_paths_are_lexically_confined_to_owned_shell_scripts() {
        for path in [
            "maintained-image-build/arm64-image-validation.sh",
            "maintained-image-build/checks/image.sh",
            "maintained-image-build/v2_check-1.sh",
        ] {
            assert!(is_safe_native_image_script_path(path), "{path}");
        }
        for path in [
            "",
            "/maintained-image-build/image.sh",
            "maintained-image-build/../image.sh",
            "maintained-image-build/a/../../image.sh",
            "maintained-image-build//image.sh",
            "maintained-image-build/./image.sh",
            "maintained-image-build/image.sh/",
            "maintained-image-build/image.sh/../outside.sh",
            "maintained-image-build\\image.sh",
            "maintained-image-build/image.py",
            "maintained-image-build/${{ github.sha }}.sh",
        ] {
            assert!(!is_safe_native_image_script_path(path), "{path:?}");
        }
    }

    #[test]
    fn declaration_rejects_bad_shared_id_and_unbounded_timeout() {
        for timeout in [1, 60] {
            assert!(
                task(
                    "architect-arm64-image",
                    "maintained-image-build/arm64-image-validation.sh",
                    timeout
                )
                .validate("config.toml")
                .is_ok()
            );
        }
        for id in ["", "Upper", "bad/id", "required"] {
            assert!(
                task(id, "maintained-image-build/arm64-image-validation.sh", 30)
                    .validate("config.toml")
                    .is_err(),
                "{id:?}"
            );
        }
        for timeout in [0, 61, u16::MAX] {
            assert!(
                task(
                    "architect-arm64-image",
                    "maintained-image-build/arm64-image-validation.sh",
                    timeout
                )
                .validate("config.toml")
                .is_err(),
                "{timeout}"
            );
        }
    }

    #[test]
    fn serde_shape_has_no_local_kind_or_duplicate_workflow_fields() {
        let task = task(
            "architect-arm64-image",
            "maintained-image-build/arm64-image-validation.sh",
            60,
        );
        let value = serde_json::to_value(task).expect("native image task serializes");
        let table = value.as_object().expect("task is a map");
        assert_eq!(table.len(), 5);
        assert_eq!(
            table.get("id").and_then(serde_json::Value::as_str),
            Some("architect-arm64-image")
        );
        assert_eq!(
            table.get("platform").and_then(serde_json::Value::as_str),
            Some("linux-arm64")
        );
        assert!(table.get("kind").is_none());
        assert!(table.get("mise_task").is_none());
        assert_eq!(
            table.get("cache").and_then(serde_json::Value::as_str),
            Some("task-owned-builder")
        );
    }

    #[test]
    fn serde_rejects_unknown_cache_policy_and_leaf_fields() {
        let valid = serde_json::json!({
            "id": "architect-arm64-image",
            "platform": "linux-arm64",
            "script": "maintained-image-build/arm64-image-validation.sh",
            "timeout_minutes": 60,
            "cache": "task-owned-builder"
        });
        let mut unknown_cache = valid.clone();
        unknown_cache["cache"] = serde_json::json!("shared");
        assert!(serde_json::from_value::<NativeImageTask>(unknown_cache).is_err());

        let mut unknown_field = valid;
        unknown_field["runner"] = serde_json::json!("ubuntu-26.04-arm");
        assert!(serde_json::from_value::<NativeImageTask>(unknown_field).is_err());
    }
}
