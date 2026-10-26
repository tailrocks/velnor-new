// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::yaml::Yaml;
use crate::{RenderError, steps};

pub(crate) fn shared_call(uses: &str) -> Result<Yaml, RenderError> {
    shared_call_named(uses, "Run shared steps")
}

pub(crate) fn shared_call_named(uses: &str, name: &str) -> Result<Yaml, RenderError> {
    shared_call_named_with_inputs(uses, name, Vec::new())
}

pub(crate) fn shared_call_named_with_inputs(
    uses: &str,
    name: &str,
    inputs: Vec<(String, Yaml)>,
) -> Result<Yaml, RenderError> {
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
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name.to_owned())),
        (
            "uses".to_owned(),
            Yaml::annotated(uses, "zizmor: ignore[self-repository]"),
        ),
    ];
    if !inputs.is_empty() {
        fields.push(("with".to_owned(), Yaml::Map(inputs)));
    }
    Ok(Yaml::Map(fields))
}

pub(crate) fn composite_yaml(name: &str, steps: Vec<Yaml>) -> Result<Yaml, RenderError> {
    composite_yaml_with_inputs(name, Vec::new(), steps)
}

pub(crate) fn composite_yaml_with_inputs(
    name: &str,
    inputs: Vec<(String, Yaml)>,
    steps: Vec<Yaml>,
) -> Result<Yaml, RenderError> {
    steps::scan_for_private_subcommands(name)?;
    let mut fields = vec![
        ("name".to_owned(), Yaml::str(name.to_owned())),
        (
            "description".to_owned(),
            Yaml::str(format!("Shared steps for {name}")),
        ),
    ];
    if !inputs.is_empty() {
        fields.push(("inputs".to_owned(), Yaml::Map(inputs)));
    }
    fields.push((
        "runs".to_owned(),
        Yaml::Map(vec![
            ("using".to_owned(), Yaml::str("composite".to_owned())),
            ("steps".to_owned(), Yaml::Seq(steps)),
        ]),
    ));
    let mut document = Yaml::Map(fields);
    crate::project_tool_resolver::bind_workflow(&mut document)?;
    Ok(document)
}

pub(crate) fn push_composite_shell(entries: &mut Vec<(String, Yaml)>, composite: bool) {
    if composite {
        entries.push(("shell".to_owned(), Yaml::str("bash".to_owned())));
    }
}
