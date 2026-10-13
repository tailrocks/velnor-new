//! Fixed jq contracts for remote-backend and saved-plan admission.

/// Fail-closed saved-plan review, implemented as a fixed jq filter.
pub(super) const PLAN_REVIEW_JQ: &str = r#"
def no_provisioners:
  type == "object"
  and ((.resources // []) | type == "array")
  and ((.module_calls // {}) | type == "object")
  and all((.resources // [])[];
    ((has("provisioners") | not) or (.provisioners | type == "array" and length == 0)))
  and all((.module_calls // {})[]; (.module | no_provisioners));
type == "object"
and (.format_version | type == "string" and startswith("1."))
and .errored == false
and ((has("resource_changes") | not) or (.resource_changes | type == "array"))
and (.configuration | type == "object")
and (.configuration.root_module | no_provisioners)
and (.planned_values | type == "object")
and ((.checks // []) | type == "array" and all(.[]; .status == "pass"))
and ((has("complete") | not) or .complete == true)
and ((has("applyable") | not) or .applyable == true)
and ((has("deferred_changes") | not) or .deferred_changes == [])
and ((has("action_invocations") | not) or .action_invocations == [])
and ((has("removed") | not) or .removed == [])
and all((.resource_changes // [])[];
  ((.mode == "managed") and (.change.actions == ["no-op"] or .change.actions == ["create"] or .change.actions == ["update"])
  or ((.mode == "data") and (.change.actions == ["no-op"] or .change.actions == ["read"]))))
"#;

/// Require the saved init metadata to prove that S3 is the active backend.
pub(super) const BACKEND_REVIEW_JQ: &str = r#"
.backend.type == "s3"
and .backend.config.bucket == env.VELNOR_BACKEND_BUCKET
and .backend.config.key == env.VELNOR_BACKEND_KEY
and .backend.config.region == env.VELNOR_BACKEND_REGION
and .backend.config.encrypt == true
and .backend.config.use_lockfile == true
"#;
