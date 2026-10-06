//! Exact metadata and immutable artifact handles for compiled release helpers.
use crate::RenderError;

pub(super) fn check(key: &str, value: &str, spans: &[&str]) -> Option<Result<(), RenderError>> {
    let expected = match key {
        "GITHUB_WORKSPACE" => "github.workspace".to_owned(),
        "GITHUB_RUN_ID" => "github.run_id".to_owned(),
        "GITHUB_RUN_ATTEMPT" => "github.run_attempt".to_owned(),
        "GITHUB_ACTOR_ID" => "github.actor_id".to_owned(),
        _ => artifact(key)?,
    };
    Some(
        if spans == [expected.as_str()] && value == format!("${{{{ {expected} }}}}") {
            Ok(())
        } else {
            Err(RenderError::BadCommand(format!("bad_env_expression:{key}")))
        },
    )
}

fn artifact(key: &str) -> Option<String> {
    let (prefix, suffix) = key.rsplit_once('_')?;
    if !matches!(suffix, "ID" | "DIGEST") {
        return None;
    }
    let (producer, output) = match prefix {
        "RELEASE_PACKAGE_ARTIFACT" => ("release-package", "package-artifact"),
        "RELEASE_PREFLIGHT_ARTIFACT" => ("release-preflight", "preflight-artifact"),
        "RELEASE_REGISTRY_RECEIPT_ARTIFACT" => {
            ("release-registry-publish", "registry-receipt-artifact")
        }
        "RELEASE_FORGE_RECEIPT_ARTIFACT" => ("release-forge-publish", "forge-receipt-artifact"),
        "RELEASE_PREPARE_ARTIFACT" => ("release-preparation-source", "artifact"),
        _ => return None,
    };
    Some(format!(
        "needs.{producer}.outputs.{output}-{}",
        suffix.to_ascii_lowercase()
    ))
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn release_bindings_reject_foreign_producers_and_composed_values() {
        for (key, inner) in [
            ("GITHUB_ACTOR_ID", "github.actor_id"),
            ("GITHUB_WORKSPACE", "github.workspace"),
            (
                "RELEASE_PREPARE_ARTIFACT_ID",
                "needs.release-preparation-source.outputs.artifact-id",
            ),
            (
                "RELEASE_REGISTRY_RECEIPT_ARTIFACT_DIGEST",
                "needs.release-registry-publish.outputs.registry-receipt-artifact-digest",
            ),
        ] {
            let value = format!("${{{{ {inner} }}}}");
            assert!(
                check(key, &value, &[inner])
                    .expect("recognized binding")
                    .is_ok()
            );
            assert!(
                check(key, &format!("prefix{value}"), &[inner])
                    .expect("recognized binding")
                    .is_err()
            );
            assert!(
                check(key, "${{ github.token }}", &["github.token"])
                    .expect("recognized binding")
                    .is_err()
            );
        }
        assert!(check("OTHER", "${{ github.actor_id }}", &["github.actor_id"]).is_none());
    }
}
