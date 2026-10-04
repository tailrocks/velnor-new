//! Local composite actions for duplicated verification lanes.
//!
//! GitHub rejects a workflow file over 500 KB. Shared lane bodies live here
//! so each lane job keeps only its id, `runs-on`, `needs`, and one call.

use crate::yaml::Yaml;
use crate::{RenderError, steps};

/// One local composite call. The path is renderer-owned, not a remote pin.
/// Zizmor's self-repository advice uses `$/'`, which resolves at the workflow
/// SHA rather than this already checked-out event tree. Keep the exception
/// on this fixed workspace-relative reference only.
pub(crate) fn shared_call(uses: &str, id: Option<&str>) -> Result<Yaml, RenderError> {
    let Some(logical) = uses.strip_prefix("./.github/actions/") else {
        return Err(RenderError::UnsafePath(uses.to_owned()));
    };
    if logical.is_empty()
        || !logical
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(RenderError::UnsafePath(uses.to_owned()));
    }
    let mut entries = vec![
        ("name".to_owned(), Yaml::str("Run shared steps".to_owned())),
        (
            "uses".to_owned(),
            Yaml::annotated(uses, "zizmor: ignore[self-repository]"),
        ),
    ];
    if let Some(id) = id {
        entries.push(("id".to_owned(), Yaml::str(id.to_owned())));
    }
    Ok(Yaml::Map(entries))
}

/// Composite action document. Steps are already rendered.
pub(crate) fn composite_yaml(
    name: &str,
    steps: Vec<Yaml>,
    outputs: Option<Vec<(String, Yaml)>>,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(name)?;
    let mut entries = vec![
        ("name".to_owned(), Yaml::str(name.to_owned())),
        (
            "description".to_owned(),
            Yaml::str(format!("Shared steps for {name}")),
        ),
    ];
    if let Some(outputs) = outputs {
        entries.push(("outputs".to_owned(), Yaml::Map(outputs)));
    }
    entries.push((
        "runs".to_owned(),
        Yaml::Map(vec![
            ("using".to_owned(), Yaml::str("composite".to_owned())),
            ("steps".to_owned(), Yaml::Seq(steps)),
        ]),
    ));
    Ok(Yaml::Map(entries))
}

/// Composite `run` steps require an explicit shell. Workflow jobs do not.
pub(crate) fn push_composite_shell(entries: &mut Vec<(String, Yaml)>, composite: bool) {
    if composite {
        entries.push(("shell".to_owned(), Yaml::str("bash".to_owned())));
    }
}

#[cfg(test)]
mod tests {
    use super::shared_call;

    #[test]
    fn shared_calls_use_only_canonical_repository_local_actions() {
        let yaml = crate::yaml::render_yaml(
            &shared_call("./.github/actions/rust-0", None).expect("canonical local action"),
        );
        assert!(
            yaml.contains("uses: ./.github/actions/rust-0 # zizmor: ignore[self-repository]"),
            "{yaml}"
        );
        for uses in [
            "./.github/actions/",
            "./.github/actions/../rust-0",
            "./.github/actions/a/b",
            "./.github/actions/rust.0",
            "./.github/actions/rust-0@deadbeef",
            "actions/checkout@0000000000000000000000000000000000000000",
        ] {
            assert!(shared_call(uses, None).is_err(), "accepted {uses}");
        }
    }
}
