//! One closed tagged union for every explicitly declared workflow task.

use crate::errors::ContractError;

use super::{BuildTask, NativeImageTask, VerificationTask};

/// Generated job-key prefix shared by every workflow task variant.
pub const WORKFLOW_TASK_JOB_PREFIX: &str = "task-";

/// One typed task in the single workflow task inventory.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum WorkflowTask {
    /// Compile-free platform and repository verification task.
    Verification(VerificationTask),
    /// Native task allowed to compile repository code under explicit bounds.
    Build(BuildTask),
    /// Hosted native-platform image validation task.
    NativeImage(NativeImageTask),
}

impl WorkflowTask {
    /// Stable ID shared across all variants.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Verification(task) => &task.id,
            Self::Build(task) => &task.id,
            Self::NativeImage(task) => &task.id,
        }
    }

    /// Validate the selected variant's closed fields and shared task identity.
    /// # Errors
    pub fn validate(&self, file: &str) -> Result<(), ContractError> {
        match self {
            Self::Verification(task) => task.validate(file),
            Self::Build(task) => task.validate(file),
            Self::NativeImage(task) => task.validate(file),
        }
    }
}

/// True for a bounded task ID in the workflow-wide namespace.
#[must_use]
pub fn is_valid_workflow_task_id(id: &str) -> bool {
    const RESERVED: [&str; 7] = [
        "actionlint",
        "candidate",
        "plan",
        "publish-baseline",
        "required",
        "task",
        "velnor-task",
    ];
    !id.is_empty()
        && id.len() <= 48
        && !RESERVED.contains(&id)
        && id.as_bytes()[0].is_ascii_lowercase()
        && (id.as_bytes()[id.len() - 1].is_ascii_lowercase()
            || id.as_bytes()[id.len() - 1].is_ascii_digit())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !id.contains("--")
}

#[cfg(test)]
mod tests {
    use super::{WorkflowTask, is_valid_workflow_task_id};
    use crate::config::{
        BuildTask, BuildTaskRunner, NativeImageCachePolicy, NativeImagePlatform, NativeImageTask,
        VerificationRunner, VerificationTask,
    };

    fn verification(id: &str) -> WorkflowTask {
        WorkflowTask::Verification(VerificationTask {
            id: id.to_owned(),
            mise_task: "desktop-format-check".to_owned(),
            runner: VerificationRunner::MacosArm64,
            timeout_minutes: 10,
        })
    }

    fn build(id: &str) -> WorkflowTask {
        WorkflowTask::Build(BuildTask {
            id: id.to_owned(),
            mise_task: "desktop-ci".to_owned(),
            tools: vec!["mr-boxington".to_owned(), "rust".to_owned()],
            runner: BuildTaskRunner::Macos26Arm64,
            timeout_minutes: 120,
            cargo_build_jobs: 2,
            nextest_test_threads: 2,
        })
    }

    fn native_image(id: &str) -> WorkflowTask {
        WorkflowTask::NativeImage(NativeImageTask {
            id: id.to_owned(),
            platform: NativeImagePlatform::LinuxArm64,
            script: "maintained-image-build/arm64-image-validation.sh".to_owned(),
            timeout_minutes: 60,
            cache: NativeImageCachePolicy::TaskOwnedBuilder,
        })
    }

    #[test]
    fn shared_task_id_namespace_rejects_reserved_and_malformed_values() {
        for id in ["desktop-format", "native-build2"] {
            assert!(is_valid_workflow_task_id(id), "{id}");
        }
        for id in ["", "Upper", "-start", "end-", "a--b", "required", "x/y"] {
            assert!(!is_valid_workflow_task_id(id), "{id:?}");
        }
        assert!(verification("same-id").validate("config.toml").is_ok());
        assert!(build("same-id").validate("config.toml").is_ok());
    }

    #[test]
    fn tagged_union_serializes_direct_task_fields_and_rejects_unknown_variants() {
        let task = build("native-desktop");
        let value = serde_json::to_value(&task).expect("serialize");
        assert_eq!(value["kind"], "build");
        assert_eq!(value["id"], "native-desktop");
        assert!(value.get("task").is_none(), "variant stays a direct table");
        let decoded: WorkflowTask = serde_json::from_value(value).expect("decode");
        assert_eq!(decoded, task);
        assert!(serde_json::from_str::<WorkflowTask>(r#"{"kind":"unknown","id":"x"}"#).is_err());
        assert!(serde_json::from_str::<WorkflowTask>(
            r#"{"kind":"verification","id":"x","mise_task":"check","runner":"linux-x64","timeout_minutes":10,"extra":true}"#
        )
        .is_err());
        assert!(serde_json::from_str::<WorkflowTask>(
            r#"{"id":"native-desktop","mise_task":"desktop-ci","tools":["mr-boxington","rust"],"runner":"macos-26-arm64","timeout_minutes":120,"cargo_build_jobs":2,"nextest_test_threads":2}"#
        )
        .is_err(), "the discriminator is required");
        let verification = verification("native-format");
        let value = serde_json::to_value(&verification).expect("serialize verification");
        assert_eq!(value["kind"], "verification");
        assert_eq!(value["id"], "native-format");
        assert!(value.get("verification").is_none());
        assert_eq!(
            serde_json::from_value::<WorkflowTask>(value).expect("decode verification"),
            verification
        );
    }

    #[test]
    fn native_image_is_a_direct_tagged_variant_in_the_shared_id_namespace() {
        let task = native_image("architect-arm64-image");
        assert!(task.validate("config.toml").is_ok());
        let value = serde_json::to_value(&task).expect("serialize native image");
        assert_eq!(value["kind"], "native-image");
        assert_eq!(value["id"], "architect-arm64-image");
        assert_eq!(value["platform"], "linux-arm64");
        assert_eq!(
            value["script"],
            "maintained-image-build/arm64-image-validation.sh"
        );
        assert_eq!(value["cache"], "task-owned-builder");
        assert!(value.get("native_image").is_none());
        assert_eq!(
            serde_json::from_value::<WorkflowTask>(value).expect("decode native image"),
            task
        );
        assert!(serde_json::from_str::<WorkflowTask>(
            r#"{"kind":"native-image","id":"x","platform":"linux-arm64","script":"maintained-image-build/x.sh","timeout_minutes":60,"cache":"global"}"#
        )
        .is_err());
    }
}
