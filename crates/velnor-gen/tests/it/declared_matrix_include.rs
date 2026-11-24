// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, job, refusal, rendered, workflow};
use crate::yaml_tree::Node;

fn matrix_job(matrix: &str, env: &str) -> String {
    workflow(
        "",
        "",
        &format!(
            "\n[[workflow.job]]\nid = \"build\"\n\n[workflow.job.matrix]\n{matrix}\n[[workflow.job.step]]\nname = \"Build\"\nrun = \"echo ok\"\nenv = {{ {env} }}\n"
        ),
    )
}

fn pairs(entry: &Node) -> Vec<(&str, &str)> {
    entry
        .keys()
        .into_iter()
        .filter_map(|key| Some((key, entry.get(key)?.text()?)))
        .collect()
}

#[test]
fn an_include_only_matrix_lists_paired_values() -> TestResult {
    let config = matrix_job(
        "fail_fast = false\ninclude = [\n  { target = \"x86_64-unknown-linux-musl\", suffix = \"x64\" },\n  { target = \"aarch64-unknown-linux-musl\", suffix = \"arm64\" },\n]\n",
        "T = \"${{ matrix.target }}\", S = \"${{ matrix.suffix }}\"",
    );
    let tree = rendered(&config)?;
    let strategy = job(&tree, "build")?.get("strategy").ok_or("no strategy")?;
    assert_eq!(
        strategy.get("fail-fast").and_then(Node::text),
        Some("false")
    );
    let include = strategy.at(&["matrix", "include"]).ok_or("no include")?;
    let entries: Vec<_> = include.items().iter().map(pairs).collect();
    assert_eq!(
        entries,
        [
            vec![("suffix", "x64"), ("target", "x86_64-unknown-linux-musl")],
            vec![
                ("suffix", "arm64"),
                ("target", "aarch64-unknown-linux-musl")
            ],
        ]
    );
    assert_eq!(
        strategy.at(&["matrix"]).map(|m| m.keys()),
        Some(vec!["include"])
    );
    Ok(())
}

#[test]
fn include_entries_add_to_axes_with_declared_or_new_keys() -> TestResult {
    let config = matrix_job(
        "axes = { os = [\"ubuntu-24.04\", \"macos-14\"] }\ninclude = [{ os = \"ubuntu-24.04\", flavor = \"musl\" }, { os = \"windows-2022\", flavor = \"msvc\" }]\n",
        "F = \"${{ matrix.flavor }}\", O = \"${{ matrix.os }}\"",
    );
    let tree = rendered(&config)?;
    let matrix = job(&tree, "build")?
        .at(&["strategy", "matrix"])
        .ok_or("no matrix")?;
    assert_eq!(matrix.keys(), ["os", "include"]);
    assert_eq!(matrix.get("include").map(|i| i.items().len()), Some(2));
    Ok(())
}

#[test]
fn an_invalid_include_is_refused() -> TestResult {
    let cases = [
        ("include = []\n", "exactly one source"),
        (
            "from_output = \"x.y\"\ninclude = [{ a = \"1\" }]\n",
            "exactly one source",
        ),
        ("include = [{}]\n", "at least one key"),
        ("include = [{ a = \"1\" }, { a = \"1\" }]\n", "repeats"),
        (
            "include = [{ a = \"${{ github.sha }}\" }]\n",
            "without expressions",
        ),
        ("include = [{ a = \"\" }]\n", "non-empty"),
        ("include = [{ a = \"1\" }, { b = \"2\" }]\n", "same keys"),
        ("include = [{ include = \"1\" }]\n", "identifier"),
        ("include = [{ \"a b\" = \"1\" }]\n", "identifier"),
        ("include = [{ a = \"1\" }, { A = \"2\" }]\n", "spelled like"),
        (
            "axes = { os = [\"a\"] }\ninclude = [{ OS = \"b\" }]\n",
            "spelled like",
        ),
        ("include = [{ a = 1 }]\n", "a"),
    ];
    for (matrix, needle) in cases {
        let message = refusal(&matrix_job(matrix, "X = \"${{ matrix.a }}\""))
            .map_err(|error| format!("{matrix:?}: {error}"))?;
        assert!(message.contains(needle), "{matrix:?}: {message}");
    }
    let message = refusal(&matrix_job(
        "include = [{ a = \"1\" }]\n",
        "X = \"${{ matrix.b }}\"",
    ))?;
    assert!(message.contains("matrix.b"), "{message}");
    Ok(())
}

#[test]
fn the_combinations_of_axes_and_include_stay_within_256() -> TestResult {
    let many = |count: usize| {
        let values: Vec<String> = (0..count).map(|index| format!("\"v{index}\"")).collect();
        let axes = format!("axes = {{ a = [{}] }}\n", values.join(", "));
        matrix_job(
            &format!("{axes}include = [{{ b = \"x\" }}, {{ b = \"y\" }}]\n"),
            "X = \"${{ matrix.a }}\"",
        )
    };
    rendered(&many(254))?;
    let message = refusal(&many(255))?;
    assert!(message.contains("256"), "{message}");
    Ok(())
}
