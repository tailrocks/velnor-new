// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_local::prune::prune;
use velnor_local::sparse::excluded;

const MARKER: &str = "- name: velnor-local prune sparse checkout";
const HEREDOC: &str = "<<'VELNOR_PRUNE'";
const END: &str = "VELNOR_PRUNE";
const CHECKOUT: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

fn workflow(with: &str) -> String {
    format!(
        "name: ci\njobs:\n  build:\n    runs-on: ubuntu-26.04\n    steps:\n      - name: Check out\n        uses: {CHECKOUT}\n        with:\n{with}          persist-credentials: false\n      - name: Build\n        run: cargo build\n"
    )
}

fn pruned_paths(text: &str) -> Vec<String> {
    let Some((body, _)) = text
        .split_once(HEREDOC)
        .and_then(|(_, rest)| rest.split_once('\n'))
        .and_then(|(_, rest)| rest.split_once(END))
    else {
        unreachable!("no prune heredoc in:\n{text}")
    };
    body.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_sparse_checkout_gains_a_prune_step_that_removes_the_paths_it_excludes() {
    let text = workflow(concat!(
        "          sparse-checkout: |\n",
        "            crates/a/\n",
        "            Cargo.toml\n",
        "          sparse-checkout-cone-mode: false\n",
    ));
    let tracked = [
        "Cargo.toml",
        "crates/a/src/lib.rs",
        "crates/b/src/lib.rs",
        "docs/x.md",
    ];
    let (rewritten, count) = prune(&text, &tracked);
    assert_eq!(count, 1);
    let marker = rewritten
        .find(MARKER)
        .unwrap_or_else(|| unreachable!("no prune step in:\n{rewritten}"));
    let persist = rewritten.find("persist-credentials").unwrap_or_default();
    let build = rewritten.find("- name: Build").unwrap_or_default();
    assert!(persist < marker && marker < build, "{rewritten}");
    assert_eq!(
        pruned_paths(&rewritten),
        ["crates/b/src/lib.rs", "docs/x.md"]
    );
}

#[test]
fn a_checkout_without_sparse_checkout_is_left_unchanged() {
    let text = workflow("          fetch-depth: 1\n");
    assert_eq!(prune(&text, &["a.txt"]), (text, 0));
}

#[test]
fn pruning_a_pruned_workflow_adds_nothing() {
    let text = workflow("          sparse-checkout: qualification\n");
    let tracked = ["qualification/ports.sh", "src/main.rs"];
    let (once, count) = prune(&text, &tracked);
    assert_eq!(count, 1);
    assert_eq!(prune(&once, &tracked), (once, 0));
}

#[test]
fn cone_mode_keeps_root_files_and_everything_under_the_listed_directory() {
    let removed = excluded(
        &["qualification"],
        true,
        &[
            "qualification/ports.sh",
            "qualification/sub/x.sh",
            "README.md",
            "src/main.rs",
        ],
    );
    assert_eq!(removed, ["src/main.rs"]);
}

#[test]
fn non_cone_patterns_keep_directory_contents_and_apply_the_last_match() {
    let excluded_paths = excluded(
        &["/*", "!/docs/**"],
        false,
        &[
            "Cargo.toml",
            "crates/a/lib.rs",
            "docs/x.md",
            "docs/sub/y.md",
        ],
    );
    assert_eq!(excluded_paths, ["docs/x.md", "docs/sub/y.md"]);
}

#[test]
fn an_unanchored_non_cone_pattern_matches_at_any_depth() {
    let excluded_paths = excluded(&["*.md"], false, &["README.md", "src/a.md", "src/a.rs"]);
    assert_eq!(excluded_paths, ["src/a.rs"]);
}
