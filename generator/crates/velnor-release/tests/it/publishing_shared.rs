// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_release::{ReleasePlan, root_publishes_to_crates_io};

use crate::fixture::{PLZ, TestResult, edit, files, metadata, with_metadata};

#[test]
fn runtime_and_generation_classify_a_root_whose_members_all_opt_out_the_same_way() -> TestResult {
    let plz = edit(PLZ, "name = \"app\"\n", "name = \"app\"\ngit_only = true\n")?;
    let members: &[(&str, &[(&str, &str)])] = &[("Cargo.toml", &[("app", "Cargo.toml")])];
    let mut generated = files();
    generated
        .release_plz_toml
        .insert(".".to_owned(), plz.clone());
    with_metadata(&mut generated, members);
    assert!(!ReleasePlan::read(&generated)?.roots[0].publishes_to_crates_io);
    assert!(!root_publishes_to_crates_io(
        ".",
        &plz,
        Some(&metadata(members))
    )?);
    Ok(())
}

#[test]
fn an_unlisted_inheriting_member_makes_runtime_and_generation_agree_on_publishing() -> TestResult {
    let plz = edit(PLZ, "name = \"app\"\n", "name = \"app\"\ngit_only = true\n")?;
    let members: &[(&str, &[(&str, &str)])] = &[(
        "Cargo.toml",
        &[("app", "Cargo.toml"), ("lib", "lib/Cargo.toml")],
    )];
    let mut generated = files();
    generated
        .release_plz_toml
        .insert(".".to_owned(), plz.clone());
    with_metadata(&mut generated, members);
    assert!(ReleasePlan::read(&generated)?.roots[0].publishes_to_crates_io);
    assert!(root_publishes_to_crates_io(
        ".",
        &plz,
        Some(&metadata(members))
    )?);
    assert!(root_publishes_to_crates_io(".", &plz, None)?);
    Ok(())
}
