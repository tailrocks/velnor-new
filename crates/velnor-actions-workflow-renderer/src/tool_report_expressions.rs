//! Closed tool report observations; producer admission binds the exact metadata.
use crate::RenderError;

pub(super) fn check(key: &str, value: &str, spans: &[&str]) -> Option<Result<(), RenderError>> {
    let suffix = match key {
        "VELNOR_TOOL_VERIFIED" => ".outputs.verified",
        "VELNOR_TOOL_BEFORE_AVAILABLE" | "VELNOR_TOOL_AFTER_AVAILABLE" => ".outputs.available",
        "VELNOR_TOOL_BEFORE_DIGEST" | "VELNOR_TOOL_AFTER_DIGEST" => ".outputs.digest",
        "VELNOR_TOOL_CHANGED" => ".outputs.changed",
        "VELNOR_TOOL_MATCHED_KEY" | "VELNOR_TOOL_PUBLICATION_MATCHED_KEY" => {
            ".outputs.cache-matched-key"
        }
        "VELNOR_TOOL_PUBLICATION_EXACT_HIT" => ".outputs.cache-hit",
        "VELNOR_TOOL_BEFORE_OUTCOME"
        | "VELNOR_TOOL_AFTER_OUTCOME"
        | "VELNOR_TOOL_INSTALL_OUTCOME"
        | "VELNOR_TOOL_RESTORE_OUTCOME"
        | "VELNOR_TOOL_SAVE_OUTCOME"
        | "VELNOR_TOOL_PUBLICATION_OUTCOME" => ".outcome",
        "VELNOR_TOOL_IDENTITY" => return Some(identity(value, spans, false)),
        "VELNOR_TOOL_PUBLICATION_EXPECTED_KEY" => return Some(identity(value, spans, true)),
        _ => return None,
    };
    let allowed = matches!(spans, [inner]
        if value == format!("${{{{ {inner} }}}}")
        && inner.strip_prefix("steps.").and_then(|rest| rest.strip_suffix(suffix))
            .is_some_and(|id| velnor_actions_contract::StepId::new(id).is_ok()));
    Some(result(key, allowed))
}

fn identity(value: &str, spans: &[&str], publication: bool) -> Result<(), RenderError> {
    let suffix = if publication {
        "-${{ env.VELNOR_CACHE_IMAGE }}-snapshot-${{ steps.velnor-tool-after.outputs.digest }}-${{ github.run_id }}-${{ github.run_attempt }}"
    } else {
        "-${{ env.VELNOR_CACHE_IMAGE }}"
    };
    let expected = if publication {
        vec![
            "env.VELNOR_CACHE_IMAGE",
            "steps.velnor-tool-after.outputs.digest",
            "github.run_id",
            "github.run_attempt",
        ]
    } else {
        vec!["env.VELNOR_CACHE_IMAGE"]
    };
    let allowed = value.strip_suffix(suffix).is_some_and(|prefix| {
        !prefix.is_empty()
            && prefix
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    }) && spans == expected;
    result("tool_report_identity", allowed)
}

fn result(key: &str, allowed: bool) -> Result<(), RenderError> {
    if allowed {
        Ok(())
    } else {
        Err(RenderError::BadCommand(format!("bad_env_expression:{key}")))
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn observations_accept_only_one_exact_step_output() {
        for (key, suffix) in [
            ("VELNOR_TOOL_VERIFIED", ".outputs.verified"),
            ("VELNOR_TOOL_AFTER_AVAILABLE", ".outputs.available"),
            ("VELNOR_TOOL_AFTER_DIGEST", ".outputs.digest"),
            ("VELNOR_TOOL_PUBLICATION_EXACT_HIT", ".outputs.cache-hit"),
            (
                "VELNOR_TOOL_PUBLICATION_MATCHED_KEY",
                ".outputs.cache-matched-key",
            ),
            ("VELNOR_TOOL_PUBLICATION_OUTCOME", ".outcome"),
        ] {
            let inner = format!("steps.velnor-tool-publication{suffix}");
            let value = format!("${{{{ {inner} }}}}");
            assert!(check(key, &value, &[&inner]).expect("recognized").is_ok());
            for bad in [format!("prefix{value}"), "${{ github.token }}".to_owned()] {
                assert!(check(key, &bad, &[&inner]).expect("recognized").is_err());
            }
        }
    }

    #[test]
    fn identity_and_receipt_keys_accept_only_closed_templates() {
        let key = "VELNOR_TOOL_PUBLICATION_EXPECTED_KEY";
        let value = "mise-v3-key-${{ env.VELNOR_CACHE_IMAGE }}-snapshot-${{ steps.velnor-tool-after.outputs.digest }}-${{ github.run_id }}-${{ github.run_attempt }}";
        let spans = [
            "env.VELNOR_CACHE_IMAGE",
            "steps.velnor-tool-after.outputs.digest",
            "github.run_id",
            "github.run_attempt",
        ];
        assert!(check(key, value, &spans).expect("recognized").is_ok());
        for bad in [
            format!("{value}-foreign"),
            value.replace("mise-v3-key", "${{ github.token }}"),
            value.replace("velnor-tool-after", "foreign"),
        ] {
            assert!(check(key, &bad, &spans).expect("recognized").is_err());
        }
        assert!(
            check(
                "VELNOR_TOOL_IDENTITY",
                "mise-v3-key-${{ env.VELNOR_CACHE_IMAGE }}",
                &["env.VELNOR_CACHE_IMAGE"]
            )
            .expect("recognized")
            .is_ok()
        );
    }
}
