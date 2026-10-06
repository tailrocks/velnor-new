//! Exact report observations; compiled owner records bind their step identities.
use crate::RenderError;

pub(super) fn check(key: &str, value: &str, spans: &[&str]) -> Option<Result<(), RenderError>> {
    let suffix = match key {
        "VELNOR_SOURCE_OUTCOME"
        | "VELNOR_SOURCE_SAVE_OUTCOME"
        | "VELNOR_SOURCE_PUBLICATION_OUTCOME"
        | "VELNOR_SOURCE_RESTORE_OUTCOME" => ".outcome",
        "VELNOR_SOURCE_VERIFIED" => ".outputs.verified",
        "VELNOR_SOURCE_ERROR" => ".outputs.error",
        "VELNOR_SOURCE_RESTORE_KEY" | "VELNOR_SOURCE_PUBLICATION_MATCHED_KEY" => {
            ".outputs.cache-matched-key"
        }
        "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY" => return Some(publication_key(value, spans)),
        "VELNOR_SOURCE_SNAPSHOT_OUTCOME" | "VELNOR_SOURCE_SNAPSHOT_CHANGED" => "",
        _ => return None,
    };
    let allowed = match spans {
        [] => matches!(
            (key, value),
            ("VELNOR_SOURCE_SNAPSHOT_OUTCOME", "success")
                | ("VELNOR_SOURCE_SNAPSHOT_CHANGED", "true")
        ),
        [inner] if value == format!("${{{{ {inner} }}}}") => {
            if suffix.is_empty() {
                snapshot(key, inner)
            } else {
                step_binding(inner, suffix)
            }
        }
        _ => false,
    };
    Some(if allowed {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_env_expression:{key}")))
    })
}

pub(super) fn publication_key(value: &str, spans: &[&str]) -> Result<(), RenderError> {
    let literal = |key: &str| {
        !key.is_empty()
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    let allowed = if spans.is_empty() {
        literal(value)
    } else {
        [
            "VELNOR_NPM_DOWNLOADS_SNAPSHOT_DIGEST",
            "VELNOR_BUN_DOWNLOADS_SNAPSHOT_DIGEST",
            "VELNOR_SOURCES_SNAPSHOT_DIGEST",
        ]
        .iter()
        .any(|digest| {
            let suffix = format!(
                "-snapshot-${{{{env.{digest}}}}}-${{{{github.run_id}}}}-${{{{github.run_attempt}}}}"
            );
            value.strip_suffix(&suffix).is_some_and(literal)
                && spans
                    == [
                        format!("env.{digest}"),
                        "github.run_id".to_owned(),
                        "github.run_attempt".to_owned(),
                    ]
        })
    };
    if allowed {
        Ok(())
    } else {
        Err(RenderError::BadCommand(
            "bad_source_publication_key".to_owned(),
        ))
    }
}

fn step_binding(inner: &str, suffix: &str) -> bool {
    inner
        .strip_prefix("steps.")
        .and_then(|rest| rest.strip_suffix(suffix))
        .is_some_and(|id| velnor_actions_contract::StepId::new(id).is_ok())
}

fn snapshot(key: &str, inner: &str) -> bool {
    matches!(
        (key, inner),
        (
            "VELNOR_SOURCE_SNAPSHOT_OUTCOME",
            "steps.velnor-npm-source-after.outcome"
                | "steps.velnor-bun-source-after.outcome"
                | "steps.velnor-rust-source-after.outcome"
        ) | (
            "VELNOR_SOURCE_SNAPSHOT_CHANGED",
            "env.VELNOR_NPM_DOWNLOADS_SNAPSHOT_CHANGED"
                | "env.VELNOR_BUN_DOWNLOADS_SNAPSHOT_CHANGED"
                | "env.VELNOR_SOURCES_SNAPSHOT_CHANGED"
        )
    )
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn publication_key_accepts_only_closed_snapshot_template() {
        let key = "VELNOR_SOURCE_PUBLICATION_EXPECTED_KEY";
        let value = "source-key-snapshot-${{env.VELNOR_SOURCES_SNAPSHOT_DIGEST}}-${{github.run_id}}-${{github.run_attempt}}";
        let spans = [
            "env.VELNOR_SOURCES_SNAPSHOT_DIGEST",
            "github.run_id",
            "github.run_attempt",
        ];
        assert!(check(key, value, &spans).expect("report key").is_ok());
        assert!(check(key, "source-key", &[]).expect("report key").is_ok());
        for bad in [
            format!("{value}-foreign"),
            value.replace("source-key", "${{github.token}}"),
            value.replace("run_attempt", "token"),
        ] {
            assert!(check(key, &bad, &spans).expect("report key").is_err());
        }
    }

    #[test]
    fn source_bindings_are_exact_single_observations() {
        let key = "VELNOR_SOURCE_OUTCOME";
        assert!(
            check(
                key,
                "${{ steps.verify.outcome }}",
                &["steps.verify.outcome"]
            )
            .expect("report key")
            .is_ok()
        );
        for (value, inner) in [
            ("prefix${{ steps.verify.outcome }}", "steps.verify.outcome"),
            ("${{ github.token }}", "github.token"),
            ("${{ steps.bad.name.outcome }}", "steps.bad.name.outcome"),
        ] {
            assert!(check(key, value, &[inner]).expect("report key").is_err());
        }
        assert!(check(key, "success", &[]).expect("report key").is_err());
    }
}
