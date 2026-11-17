// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

const PREFIX: &str = "env -u ACTIONS_ID_TOKEN_REQUEST_TOKEN -u ACTIONS_ID_TOKEN_REQUEST_URL -u ACTIONS_RUNTIME_TOKEN -u GITHUB_TOKEN -u MISE_GITHUB_TOKEN -u GH_TOKEN -u GH_HOST -u GH_CONFIG_DIR ";
const VARIABLES: [&str; 8] = [
    "ACTIONS_ID_TOKEN_REQUEST_TOKEN",
    "ACTIONS_ID_TOKEN_REQUEST_URL",
    "ACTIONS_RUNTIME_TOKEN",
    "GITHUB_TOKEN",
    "MISE_GITHUB_TOKEN",
    "GH_TOKEN",
    "GH_HOST",
    "GH_CONFIG_DIR",
];

fn commands(run: &str) -> impl Iterator<Item = &str> {
    run.split(['\n', ';', '|', '&'])
        .flat_map(|line| line.split("$("))
        .map(str::trim)
        .filter(|command| !command.is_empty())
}

fn repository_command(command: &str) -> bool {
    let mut words = command.split_whitespace().peekable();
    while words.peek().is_some_and(|word| {
        *word == "env" || *word == "-u" || VARIABLES.contains(word) || word.contains('=')
    }) {
        words.next();
    }
    let Some(executable) = words.next().map(|word| word.trim_matches(['\'', '"'])) else {
        return false;
    };
    let arguments = words.collect::<Vec<_>>().join(" ");
    match executable.rsplit('/').next() {
        Some("cargo" | "mbx" | "mise") => repository_build(command),
        Some("bash" | "python3" | "bun") => {
            arguments.contains("scripts/")
                || arguments.contains("images/")
                || arguments.starts_with("-s --")
        }
        Some("docker") => arguments.starts_with("build "),
        _ => {
            executable.starts_with("scripts/")
                || executable.starts_with("./")
                || [
                    "linux-assets/",
                    "macos-assets/",
                    "macos-intel-assets/",
                    "homebrew-assets/",
                    "source-preview/",
                ]
                .iter()
                .any(|directory| executable.starts_with(directory))
        }
    }
}

fn unstripped(run: &str) -> Vec<&str> {
    commands(run)
        .filter(|command| repository_command(command) && !command.starts_with(PREFIX))
        .collect()
}

fn runs<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::Object(fields) => {
            if let Some(run) = fields.get("run").and_then(Value::as_str) {
                found.push(run);
            }
            for child in fields.values() {
                runs(child, found);
            }
        }
        Value::Array(values) => {
            for child in values {
                runs(child, found);
            }
        }
        _ => {}
    }
}

fn release_documents() -> Result<Vec<(String, Value)>, Box<dyn Error>> {
    let mut files = release_files()?;
    let mut qualification = request()?;
    qualification.workflows = BTreeSet::from([
        RoutingWorkflow::Qualification,
        RoutingWorkflow::GeneratorRelease,
    ]);
    qualification.mbx_qualification = Some(crate::gc1_t50::mbx_pins());
    qualification.mise_pin_qualification = Some(crate::gc1_t50::mise_pin_pins());
    files.extend(
        render_schema2_workflows(&qualification)?
            .into_iter()
            .filter(|file| file.path == ".github/workflows/qualification.yml"),
    );
    files
        .into_iter()
        .filter(|file| {
            file.path.starts_with(".github/workflows/product-release")
                || file.path.starts_with(".github/actions/generator-release-")
                || file.path == ".github/workflows/qualification.yml"
        })
        .map(|file| {
            let mut document: Value = serde_saphyr::from_str(&file.bytes)?;
            if file.path == ".github/workflows/qualification.yml" {
                let jobs = document["jobs"]
                    .as_object_mut()
                    .ok_or("missing qualification jobs")?;
                jobs.retain(|name, _| name.starts_with("stable-sources-"));
                assert_eq!(jobs.len(), 9);
            }
            Ok((file.path, document))
        })
        .collect()
}

#[test]
fn release_repository_commands_strip_every_credential() -> Result<(), Box<dyn Error>> {
    let mut failures = Vec::new();
    for (path, document) in release_documents()? {
        let mut scripts = Vec::new();
        runs(&document, &mut scripts);
        let boundaries = scripts
            .iter()
            .flat_map(|run| commands(run))
            .filter(|command| repository_command(command))
            .count();
        println!("{path}: {boundaries} repository commands");
        for run in scripts {
            failures.extend(
                unstripped(run)
                    .into_iter()
                    .map(|command| format!("{path}: {command}")),
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    Ok(())
}

#[test]
fn release_stripping_rejects_each_rendered_boundary_mutation() -> Result<(), Box<dyn Error>> {
    let mut boundaries = 0;
    let mut mutations = 0;
    for (path, document) in release_documents()? {
        let mut found = Vec::new();
        runs(&document, &mut found);
        for run in found {
            for command in commands(run).filter(|command| repository_command(command)) {
                boundaries += 1;
                assert!(command.starts_with(PREFIX), "{path}: {command}");
                let executable = command
                    .strip_prefix(PREFIX)
                    .ok_or("missing stripping prefix")?;
                assert_eq!(unstripped(executable), [executable], "{path}");
                mutations += 1;
                for variable in VARIABLES {
                    let mutated = command.replacen(&format!("-u {variable} "), "", 1);
                    assert_eq!(
                        unstripped(&mutated),
                        [mutated.as_str()],
                        "{path}: {variable}"
                    );
                    mutations += 1;
                }
            }
        }
    }
    assert!(boundaries > 0);
    assert_eq!(mutations, boundaries * (VARIABLES.len() + 1));
    println!("validated {boundaries} repository commands with {mutations} stripping mutations");
    Ok(())
}

#[test]
fn release_stripping_checks_each_execution_including_substitutions() {
    for run in [
        format!("{PREFIX}mbx build\nbash scripts/build.sh"),
        format!("bash scripts/build.sh\n{PREFIX}mbx build"),
        format!("{PREFIX}test \"$(./linux-assets/candidate --version)\" = version"),
        format!("echo '{PREFIX}'; mbx build"),
    ] {
        assert_eq!(unstripped(&run).len(), 1, "{run}");
    }
    for run in [
        format!("test \"$({PREFIX}./linux-assets/candidate --version)\" = version"),
        format!("test \"$({PREFIX}/usr/bin/python3 -I -S scripts/tree.py cache)\" = tree"),
    ] {
        assert!(unstripped(&run).is_empty(), "{run}");
    }
}
