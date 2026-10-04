//! Local composite actions for duplicated verification lanes.
//!
//! GitHub rejects a workflow file over 500 KB. Shared lane bodies live here
//! so each lane job keeps only its id, `runs-on`, `needs`, and one call.

use crate::yaml::Yaml;
use crate::{RenderError, steps};

/// One local composite call. The path is renderer-owned, not a remote pin.
pub(crate) fn shared_call(uses: &str, id: Option<&str>) -> Result<Yaml, RenderError> {
    if !uses.starts_with("$/.github/actions/")
        || uses.contains("..")
        || uses.contains('\\')
        || uses.contains('@')
    {
        return Err(RenderError::UnsafePath(uses.to_owned()));
    }
    let mut entries = vec![
        ("name".to_owned(), Yaml::str("Run shared steps".to_owned())),
        ("uses".to_owned(), Yaml::str(uses.to_owned())),
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
