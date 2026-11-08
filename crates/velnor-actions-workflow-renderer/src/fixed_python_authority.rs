// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};

use crate::{RenderError, yaml::Yaml};

#[derive(Debug, Clone, Copy)]
pub(crate) struct FixedSource<'a> {
    pub(crate) path: &'a str,
    pub(crate) source: &'a str,
}

fn step(name: String, run: String) -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str(name)),
        ("shell".to_owned(), Yaml::str("bash")),
        ("run".to_owned(), Yaml::str(run)),
    ])
}

pub(crate) fn materialize_steps(
    files: &[FixedSource<'_>],
    prefix: &'static str,
    environment: &'static str,
    name: &'static str,
) -> Vec<Yaml> {
    let initialize = format!(
        "set -euo pipefail\numask 077\ndir=\"$(mktemp -d \"$RUNNER_TEMP/{prefix}.XXXXXXXX\")\"\nchmod 0700 \"$dir\"\nprintf '{environment}=%s\\n' \"$dir\" >> \"$GITHUB_ENV\"\n"
    );
    let mut result = vec![step(name.to_owned(), initialize)];
    for file in files {
        let chunks = file.source.as_bytes().chunks(12_000);
        let total = chunks.len();
        for (index, chunk) in chunks.enumerate() {
            let redirect = if index == 0 { ">" } else { ">>" };
            let encoded = STANDARD.encode(chunk);
            let mut run = format!(
                "set -euo pipefail\nset -C\n/usr/bin/python3 -I -S -c 'import base64,sys; sys.stdout.buffer.write(base64.b64decode(sys.argv[1], validate=True))' '{encoded}' {redirect} \"${environment}/{}\"\n",
                file.path
            );
            if index + 1 == total {
                run.push_str("chmod 0400 \"$");
                run.push_str(environment);
                run.push('/');
                run.push_str(file.path);
                run.push_str("\"\n");
            }
            result.push(step(
                format!(
                    "Write fixed authority source {} ({}/{total})",
                    file.path,
                    index + 1
                ),
                run,
            ));
        }
    }
    result
}

pub(crate) fn bound_workflow(document: &mut Yaml) -> Result<(), RenderError> {
    let Yaml::Map(fields) = document else {
        return Ok(());
    };
    let workflow_defaults = fields.iter().any(|(key, _)| key == "defaults");
    let Some((_, Yaml::Map(jobs))) = fields.iter_mut().find(|(key, _)| key == "jobs") else {
        return Ok(());
    };
    for (_, job) in jobs {
        let Yaml::Map(fields) = job else {
            continue;
        };
        let supported = !workflow_defaults
            && !fields.iter().any(|(key, _)| key == "defaults" || key == "container")
            && fields.iter().any(|(key, value)| {
                key == "runs-on" && matches!(value, Yaml::Str(label) if label.starts_with("ubuntu-") || label.starts_with("macos-"))
            });
        let Some((_, Yaml::Seq(steps))) = fields.iter_mut().find(|(key, _)| key == "steps") else {
            continue;
        };
        *steps = bound_steps(std::mem::take(steps), supported)?;
    }
    Ok(())
}

fn bound_steps(steps: Vec<Yaml>, supported: bool) -> Result<Vec<Yaml>, RenderError> {
    let mut bounded = Vec::new();
    for mut original in steps {
        let Yaml::Map(fields) = &mut original else {
            bounded.push(original);
            continue;
        };
        let Some(source) = fields
            .iter()
            .find_map(|(key, value)| match (key.as_str(), value) {
                ("run", Yaml::Str(source)) if source.chars().count() > 21_000 => {
                    Some(source.clone())
                }
                _ => None,
            })
        else {
            bounded.push(original);
            continue;
        };
        if !supported {
            return Err(RenderError::InvalidWorkflow(
                "oversized_run_unsupported_runner".to_owned(),
            ));
        }
        if source.contains("${{") {
            return Err(RenderError::InvalidWorkflow(
                "oversized_run_contains_expression".to_owned(),
            ));
        }
        if fields.iter().any(|(key, value)| {
            key == "shell" && !matches!(value, Yaml::Str(shell) if shell == "bash")
        }) {
            return Err(RenderError::InvalidWorkflow(
                "oversized_run_unsupported_shell".to_owned(),
            ));
        }
        let condition = fields.iter().find(|(key, _)| key == "if").cloned();
        let pipefail = fields.iter().any(|(key, value)| {
            key == "shell" && matches!(value, Yaml::Str(shell) if shell == "bash")
        });
        let shell_options = if pipefail { "-e -o pipefail" } else { "-e" };
        let digest = Sha256::digest(source.as_bytes())
            .iter()
            .flat_map(|byte| {
                [
                    b"0123456789abcdef"[usize::from(byte >> 4)],
                    b"0123456789abcdef"[usize::from(byte & 15)],
                ]
            })
            .map(char::from)
            .collect::<String>();
        let mut materializers = materialize_steps(
            &[FixedSource {
                path: "script.sh",
                source: &source,
            }],
            "velnor-fixed-script",
            "VELNOR_FIXED_SCRIPT_DIR",
            "Materialize fixed script",
        );
        if let Some(condition) = condition {
            for materializer in &mut materializers {
                if let Yaml::Map(fields) = materializer {
                    fields.push(condition.clone());
                }
            }
        }
        if let Some((_, run)) = fields.iter_mut().find(|(key, _)| key == "run") {
            *run = Yaml::str(format!(
                "/usr/bin/python3 -I -S -c 'import hashlib,sys; sys.exit(hashlib.sha256(open(sys.argv[1], \"rb\").read()).hexdigest() != sys.argv[2])' \"$VELNOR_FIXED_SCRIPT_DIR/script.sh\" '{digest}'\nexec \"$BASH\" --noprofile --norc {shell_options} \"$VELNOR_FIXED_SCRIPT_DIR/script.sh\""
            ));
        }
        bounded.extend(materializers);
        bounded.push(original);
    }
    Ok(bounded)
}
