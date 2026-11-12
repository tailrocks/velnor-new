// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::composite::{composite_yaml, shared_call_named};
use crate::{RenderError, yaml::Yaml};

pub(super) fn source_build_action(
    logical: &str,
    name: &str,
    steps: Vec<Yaml>,
    actions: &mut Vec<(String, Yaml)>,
) -> Result<Yaml, RenderError> {
    let mut action = composite_yaml(name, steps)?;
    if let Yaml::Map(fields) = &mut action {
        fields.push((
            "outputs".to_owned(),
            Yaml::Map(
                [
                    (
                        "sources_id",
                        "${{ steps.upload-sources.outputs.artifact-id }}",
                    ),
                    (
                        "sources_digest",
                        "${{ steps.upload-sources.outputs.artifact-digest }}",
                    ),
                    ("sources_tree", "${{ steps.sources-identity.outputs.tree }}"),
                    (
                        "sources_root_lock",
                        "${{ steps.sources-identity.outputs.root_lock }}",
                    ),
                    (
                        "sources_runner_lock",
                        "${{ steps.sources-identity.outputs.runner_lock }}",
                    ),
                    (
                        "sources_attempt",
                        "${{ steps.sources-identity.outputs.attempt }}",
                    ),
                ]
                .into_iter()
                .map(|(name, value)| {
                    (
                        name.to_owned(),
                        Yaml::Map(vec![
                            (
                                "description".to_owned(),
                                Yaml::str("Same-run Cargo source artifact"),
                            ),
                            ("value".to_owned(), Yaml::str(value)),
                        ]),
                    )
                })
                .collect(),
            ),
        ));
    }
    actions.push((format!(".github/actions/{logical}/action.yml"), action));
    let mut call = shared_call_named(&format!("./.github/actions/{logical}"), name)?;
    if let Yaml::Map(fields) = &mut call {
        fields.push(("id".to_owned(), Yaml::str("build")));
    }
    Ok(call)
}
