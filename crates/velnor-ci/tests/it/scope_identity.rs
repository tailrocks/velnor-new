// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::{BTreeMap, BTreeSet};

use velnor_ci::scope::Scope;
use velnor_model::{
    CompileInputs, InputOwner, PackageName, PolicyTools, RepoFacts, RepoPath, RustCiConfig,
    RustMember, RustRoot, ToolPins,
};

fn path(text: &str) -> RepoPath {
    RepoPath::new(text).unwrap_or_else(|e| unreachable!("{e}"))
}

fn util(directory: &str) -> RustRoot {
    RustRoot {
        manifest: path(&format!("{directory}/Cargo.toml")),
        target_dir: path(&format!("{directory}/target")),
        nextest_ci_profile: false,
        members: vec![RustMember {
            name: PackageName::new("util").unwrap_or_else(|e| unreachable!("{e}")),
            manifest: path(&format!("{directory}/util/Cargo.toml")),
            directory: Some(path(&format!("{directory}/util"))),
            dependencies: Vec::new(),
            targets: Vec::new(),
            custom_harnesses: Vec::new(),
        }],
        depends_on: Vec::new(),
        external_dependencies: Vec::new(),
    }
}

fn facts() -> RepoFacts {
    RepoFacts {
        repository: String::new(),
        rust_roots: vec![util("a"), util("b")],
        toolchain: None,
        tools: ToolPins {
            configs: Vec::new(),
            locked_file: true,
            tools: BTreeMap::new(),
        },
        config: RustCiConfig::default(),
        helper: None,
        compile_inputs: CompileInputs::new(),
        inputs_lock_identified: false,
        timings: BTreeMap::new(),
        timings_notes: Vec::new(),
        private_sources: None,
        private_base_caches: false,
        private_suggestion: None,
        unowned: BTreeMap::new(),
        policy: PolicyTools::default(),
        deny_configs: BTreeMap::new(),
        weakenings: Vec::new(),
    }
}

fn listed(owner: InputOwner) -> CompileInputs {
    BTreeMap::from([(Some(path("docs/a.md")), BTreeSet::from([owner]))])
}

#[test]
fn a_root_reads_the_lock_paths_of_its_own_package_and_not_those_of_a_namesake_in_another_root() {
    let facts = facts();
    let name = || PackageName::new("util").unwrap_or_else(|e| unreachable!("{e}"));
    let identified = listed(InputOwner::at(name(), path("a/util/Cargo.toml")));
    let of_a = Scope::new("a/", &identified, Some(&facts));
    let of_b = Scope::new("b/", &identified, Some(&facts));
    assert!(of_a.contains_bytes(b"docs/a.md"));
    assert!(!of_b.contains_bytes(b"docs/a.md"));

    let name_only = listed(InputOwner::named(name()));
    assert!(Scope::new("b/", &name_only, Some(&facts)).contains_bytes(b"docs/a.md"));
}
