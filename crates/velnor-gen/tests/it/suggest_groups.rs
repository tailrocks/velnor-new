// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::cli::Command;
use velnor_gen::io::parse_cli;
use velnor_gen::suggest_groups;
use velnor_model::{GroupRoot, RepoView};
use velnor_rust::{TIMINGS_PATH, detect};

use crate::facts::TestResult;
use crate::pipeline::view;

fn timings(entries: &[(&str, &[(&str, u64)])]) -> String {
    let roots: Vec<String> = entries
        .iter()
        .map(|(root, binaries)| {
            let binaries: Vec<String> = binaries
                .iter()
                .map(|(id, seconds)| format!("\"{id}\": {seconds}"))
                .collect();
            format!(
                "\"{root}\": {{\"lint_build_seconds\": 1, \"test_build_seconds\": 1, \"test_seconds\": 1, \"test_binary_seconds\": {{{}}}}}",
                binaries.join(", ")
            )
        })
        .collect();
    format!("{{{}}}", roots.join(", "))
}

fn timed(
    roots: &[&str],
    config: &str,
    timings: &str,
) -> Result<RepoView, Box<dyn std::error::Error>> {
    let mut view = view(roots, Some(config.as_bytes()))?;
    view.tracked_paths.insert(TIMINGS_PATH);
    view.files.insert(TIMINGS_PATH, timings.as_bytes().to_vec());
    Ok(view)
}

fn suggested(
    roots: &[&str],
    config: &str,
    timings: &str,
) -> Result<(String, Vec<String>), Box<dyn std::error::Error>> {
    Ok(suggest_groups(&timed(roots, config, timings)?)?)
}

#[test]
fn a_binary_over_a_third_of_the_root_gets_one_block_and_a_smaller_one_gets_none() -> TestResult {
    let (text, notes) = suggested(
        &["Cargo.toml"],
        "",
        &timings(&[(
            "Cargo.toml",
            &[("app::db", 40), ("app", 30), ("app::web", 20)],
        )]),
    )?;
    assert_eq!(
        text,
        "# app::db takes 40 of 90 s of the tests in Cargo.toml\n[[test_groups]]\nname = \"app-db\"\nfilter = \"binary_id(=app::db)\"\n"
    );
    assert!(notes.is_empty(), "{notes:?}");
    let (text, _) = suggested(
        &["Cargo.toml"],
        "",
        &timings(&[(
            "Cargo.toml",
            &[
                ("app::a", 20),
                ("app::b", 20),
                ("app::c", 20),
                ("app::d", 15),
                ("app::e", 15),
            ],
        )]),
    )?;
    assert_eq!(text, "");
    Ok(())
}

#[test]
fn without_times_or_with_a_malformed_file_nothing_is_suggested_and_the_file_is_named() -> TestResult
{
    let (text, notes) = suggest_groups(&view(&["Cargo.toml"], Some(b""))?)?;
    assert_eq!((text.as_str(), notes.len()), ("", 0));
    let (text, notes) = suggested(&["Cargo.toml"], "", "{")?;
    assert_eq!(text, "");
    assert!(
        notes.len() == 1 && notes[0].starts_with(".velnor/rust-ci-timings.json: "),
        "{notes:?}"
    );
    let (text, _) = suggested(&["Cargo.toml"], "", &timings(&[("Cargo.toml", &[])]))?;
    assert_eq!(text, "");
    Ok(())
}

#[test]
fn the_output_is_the_same_for_every_run_and_for_every_key_order_and_ties_go_by_name() -> TestResult
{
    let ordered = timings(&[("Cargo.toml", &[("a::x", 40), ("a::y", 40), ("a::z", 20)])]);
    let shuffled = timings(&[("Cargo.toml", &[("a::z", 20), ("a::y", 40), ("a::x", 40)])]);
    let (first, _) = suggested(&["Cargo.toml"], "", &ordered)?;
    for _ in 0..5 {
        assert_eq!(suggested(&["Cargo.toml"], "", &shuffled)?.0, first);
        assert_eq!(suggested(&["Cargo.toml"], "", &ordered)?.0, first);
    }
    let names: Vec<&str> = first.lines().filter(|l| l.starts_with("name")).collect();
    assert_eq!(names, ["name = \"a-x\"", "name = \"a-y\""]);
    Ok(())
}

#[test]
fn with_several_roots_each_block_names_its_root_and_each_root_counts_alone() -> TestResult {
    let (text, _) = suggested(
        &["Cargo.toml", "lab/Cargo.toml"],
        "",
        &timings(&[
            ("lab/Cargo.toml", &[("lab::bin/lab", 40), ("lab", 50)]),
            ("Cargo.toml", &[("app::db", 7), ("app", 3)]),
        ]),
    )?;
    let blocks: Vec<&str> = text.split("\n\n").collect();
    assert_eq!(blocks.len(), 3, "{text}");
    assert!(blocks[0].contains("filter = \"binary_id(=app::db)\"\nroot = \".\""));
    assert!(blocks[1].contains("name = \"lab\"\nfilter = \"binary_id(=lab)\"\nroot = \"lab\""));
    assert!(blocks[2].contains("name = \"lab-bin-lab\""));
    assert!(blocks[2].ends_with("root = \"lab\"\n"));
    Ok(())
}

#[test]
fn names_that_collide_or_are_reserved_get_a_number_and_every_name_is_a_valid_group_name()
-> TestResult {
    for (binaries, expected) in [
        (
            &[("a::b/c", 40), ("a::b-c", 40), ("x", 10)][..],
            &["name = \"a-b-c\"", "name = \"a-b-c-2\""][..],
        ),
        (
            &[("default", 50), ("x", 10)][..],
            &["name = \"default-2\""][..],
        ),
    ] {
        let (text, _) = suggested(&["Cargo.toml"], "", &timings(&[("Cargo.toml", binaries)]))?;
        let names: Vec<&str> = text.lines().filter(|l| l.starts_with("name")).collect();
        assert_eq!(names, expected);
        let facts = detect(&timed(&["Cargo.toml"], &text, "")?)?;
        assert_eq!(facts.config.test_groups.len(), expected.len());
    }
    Ok(())
}

#[test]
fn names_collide_without_regard_to_case_and_ids_without_a_usable_slug_still_get_a_valid_name()
-> TestResult {
    for (config, binaries, expected) in [
        (
            "[[test_groups]]\nname = \"App-DB\"\nfilter = \"binary_id(=other)\"\n",
            &[("app::db", 50), ("x", 10)][..],
            &["name = \"app-db-2\""][..],
        ),
        (
            "",
            &[("App", 40), ("app", 40), ("x", 10)][..],
            &["name = \"App\"", "name = \"app-2\""][..],
        ),
        (
            "",
            &[("\u{65e5}\u{672c}", 40), ("\u{4e2d}\u{6587}", 40), ("x", 10)][..],
            &["name = \"binary\"", "name = \"binary-2\""][..],
        ),
        ("", &[("2fa", 50), ("x", 10)][..], &["name = \"binary-2fa\""][..]),
    ] {
        let (text, _) = suggested(&["Cargo.toml"], config, &timings(&[("Cargo.toml", binaries)]))?;
        let names: Vec<&str> = text.lines().filter(|l| l.starts_with("name")).collect();
        assert_eq!(names, expected, "{text}");
        let facts = detect(&timed(&["Cargo.toml"], &format!("{config}{text}"), "")?)?;
        assert_eq!(
            facts.config.test_groups.len(),
            expected.len() + usize::from(!config.is_empty())
        );
    }
    Ok(())
}

#[test]
fn a_suggestion_is_valid_config_and_is_not_suggested_again_once_it_is_in_the_config() -> TestResult
{
    let measured = timings(&[(
        "Cargo.toml",
        &[("app::db", 40), ("app", 20), ("app::web", 30)],
    )]);
    let (text, _) = suggested(&["Cargo.toml"], "", &measured)?;
    let facts = detect(&timed(&["Cargo.toml"], &text, &measured)?)?;
    let groups = &facts.config.test_groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].filter, "binary_id(=app::db)");
    assert_eq!(groups[0].root, GroupRoot::OnlyRoot);
    assert_eq!(suggested(&["Cargo.toml"], &text, &measured)?.0, "");
    Ok(())
}

#[test]
fn the_command_takes_no_arguments_and_needs_no_pinned_helper() -> TestResult {
    let cli = parse_cli(["velnor-gen", "suggest-groups"])?;
    assert_eq!(cli.command, Command::SuggestGroups);
    assert!(parse_cli(["velnor-gen", "suggest-groups", "extra"]).is_err());
    Ok(())
}
