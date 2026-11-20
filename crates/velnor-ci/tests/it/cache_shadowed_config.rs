// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cache_private_repo::{GIT_LOCK, decide_with, private_checkout};

const MIRROR: &str = "[source.mirror]\nregistry = \"https://registry.example.invalid/\"\n";
const HARMLESS: &str = "[build]\njobs = 2\n";

fn decide_with_configs(name: &str, configs: &[(&str, &str)]) -> String {
    let repo = private_checkout(name, GIT_LOCK);
    for (file, text) in configs {
        repo.write_unstaged(&format!(".cargo/{file}"), text);
    }
    let (output, written) = decide_with(&repo, Some("true"));
    assert!(output.status.success(), "{name}: {output:?}");
    written
}

#[test]
fn a_config_toml_that_cargo_ignores_beside_a_config_does_not_refuse_the_save() {
    assert_eq!(
        decide_with_configs(
            "decide-shadowed-toml",
            &[("config", HARMLESS), ("config.toml", MIRROR)]
        ),
        "save=true\nsources=true\n"
    );
}

#[test]
fn the_config_cargo_reads_still_refuses_the_save() {
    for (label, configs) in [
        ("toml-alone", vec![("config.toml", MIRROR)]),
        ("plain-alone", vec![("config", MIRROR)]),
        (
            "plain-wins",
            vec![("config", MIRROR), ("config.toml", HARMLESS)],
        ),
    ] {
        assert_eq!(
            decide_with_configs(&format!("decide-read-{label}"), &configs),
            "save=false\nsources=false\n",
            "{label}"
        );
    }
}
