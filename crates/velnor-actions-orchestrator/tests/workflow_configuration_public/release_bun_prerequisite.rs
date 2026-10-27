// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::fs;

use velnor_actions_orchestrator::{prepare, render_staged_tree};

const QUALIFICATION_ACTIONS: [(&str, &str); 4] = [
    (
        "generator-release-qualify-linux",
        "Qualify downloaded candidate",
    ),
    (
        "generator-release-qualify-macos",
        "Qualify downloaded candidate",
    ),
    (
        "generator-release-qualify-macos-intel",
        "Qualify downloaded candidate",
    ),
    (
        "generator-release-qualify-homebrew",
        "Verify candidate release identity without credentials",
    ),
];

#[test]
fn generator_release_qualifiers_resolve_and_install_project_bun_before_candidate_checks()
-> Result<(), Box<dyn Error>> {
    let repo = super::workflow_configuration_fixture::make_repo(
        "schema = 2\n[workflow]\npolicy = \"consumer-v1\"\ndefault_branch = \"testmain\"\n\n[execution]\ndefault_profile = \"hosted\"\nhosted_profile = \"hosted\"\nscale_set_profile = \"local\"\nworkflows = [\"generator_release\"]\n[execution.profiles.hosted]\nkind = \"github-hosted\"\nlabel = \"ubuntu-26.04\"\nplatform = \"linux/amd64\"\n[execution.profiles.local]\nkind = \"github-scale-set\"\nname = \"ubuntu-26.04-scale-set\"\nlabels = [\"velnor\", \"ubuntu-26.04-scale-set\"]\nplatform = \"linux/amd64\"\n",
    )?;
    let mise = include_str!("../../../../mise.toml").replace("bun = \"1.4.4\"", "bun = \"1.5.7\"");
    fs::write(repo.path().join("mise.toml"), mise)?;
    fs::write(
        repo.path().join("mise.lock"),
        include_str!("../../../../mise.lock"),
    )?;

    let tree = render_staged_tree(&prepare(repo.path())?)?;
    assert!(tree.get(".github/workflows/product-release.yml").is_some());
    for (action, candidate_step) in QUALIFICATION_ACTIONS {
        let path = format!(".github/actions/{action}/action.yml");
        let rendered = tree
            .get(&path)
            .ok_or_else(|| format!("{path} is missing"))?;
        let setup = rendered.find("Setup Mise").ok_or("Mise setup is missing")?;
        let resolve = rendered
            .find("Resolve project tool versions")
            .ok_or_else(|| format!("{action} does not resolve the project Bun pin"))?;
        let resolver_end = rendered[resolve..]
            .find("\n        - name: ")
            .map_or(rendered.len(), |end| resolve + end);
        assert!(
            rendered[resolve..resolver_end].contains("VELNOR_TOOL_BUN"),
            "{action} resolver does not include the project Bun pin"
        );
        let install = rendered
            .find(r#"bun@'\"${VELNOR_TOOL_BUN}\""#)
            .ok_or_else(|| format!("{action} does not explicitly install Bun"))?;
        assert!(
            setup < resolve && resolve < install,
            "{action} must resolve Bun after Mise setup and before installation"
        );
        let candidate = rendered
            .find(candidate_step)
            .ok_or_else(|| format!("{action} candidate qualification is missing"))?;
        assert!(
            install < candidate,
            "{action} must install Bun before candidate qualification"
        );
        assert!(!rendered.contains("bun@1.4.4"));
        assert!(!rendered.contains("bun@1.5.7"));
    }
    Ok(())
}
