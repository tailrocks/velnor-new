// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn a_package_that_gains_or_loses_a_dependency_is_changed_even_when_no_crate_is_new() {
    let base = workspace();
    let before = render(4, &base, "");
    let rows = [
        (
            replaced(&base, member("lib", &["core"])),
            Some(found(&[("lib", "lib"), ("app", "lib")])),
        ),
        (
            replaced(&base, member("tool", &["fnv", "hex", "serde"])),
            Some(found(&[("tool", "tool")])),
        ),
        (
            replaced(&base, krate("hex", "0.4.3", &["fnv"])),
            Some(found(&[("lib", "hex"), ("app", "hex"), ("tool", "hex")])),
        ),
    ];
    for (head, expected) in rows {
        assert_eq!(affected(&before, &render(4, &head, "")), expected);
    }
}

#[test]
fn two_versions_of_one_crate_are_told_apart() {
    let packages = |syn1: &str, syn2: &str| -> Vec<Pkg> {
        let mut packages = replaced(
            &replaced(
                &workspace(),
                krate("serde_derive", "1.0.1", &[&format!("syn {syn2}")]),
            ),
            member("tool", &["fnv", "hex", &format!("syn {syn1}")]),
        );
        packages.push(krate("syn", syn1, &[]));
        packages.push(krate("syn", syn2, &[]));
        packages
    };
    let before = render(4, &packages("1.0.1", "2.0.1"), "");
    assert_eq!(
        affected(&before, &render(4, &packages("1.0.2", "2.0.1"), "")),
        Some(found(&[("tool", "syn")]))
    );
    assert_eq!(
        affected(&before, &render(4, &packages("1.0.1", "2.0.2"), "")),
        Some(found(&[("core", "syn"), ("lib", "syn"), ("app", "syn")]))
    );
}

#[test]
fn a_members_own_version_with_an_edited_manifest_names_nothing_and_with_a_changed_dependency_names_that_dependency()
 {
    let base = workspace();
    let before = render(4, &base, "");
    let bumped = replaced(&base, member("lib", &["core", "hex"]).version("0.1.0"));
    assert_eq!(
        affected_with(&before, &render(4, &bumped, ""), &["lib"]),
        Some(found(&[]))
    );
    let both = replaced(&bumped, krate("fnv", "1.0.6", &[]));
    assert_eq!(
        affected_with(&before, &render(4, &both, ""), &["lib"]),
        Some(found(&[("tool", "fnv")]))
    );
}

const GIT: &str = "git+https://example.com/repo?rev=abc#abc";

fn with(packages: &[Pkg]) -> String {
    render(4, packages, "")
}

fn heads_to_refuse(base: &[Pkg]) -> Vec<(&'static str, String)> {
    let before = with(base);
    let mut added_member = base.to_vec();
    added_member.push(member("extra", &[]));
    let mut added_path = base.to_vec();
    added_path.push(member("local", &[]));
    let renamed: Vec<Pkg> = base
        .iter()
        .map(|package| match package.name {
            "tool" => member("tool2", &["fnv", "hex"]),
            _ => package.clone(),
        })
        .collect();
    let mut twice = base.to_vec();
    twice.push(krate("unused", "1.0.0", &[]));
    twice.push(krate("unused", "1.0.0", &[]));
    let mut ambiguous = base.to_vec();
    ambiguous.push(krate("fnv", "1.0.6", &[]));
    vec![
        ("unparsable head", "not toml [".to_owned()),
        ("empty head", String::new()),
        ("format change", render(3, base, "")),
        ("unknown format", render(5, base, "")),
        (
            "git source change",
            with(&replaced(base, krate("fnv", "1.0.5", &[]).source(GIT))),
        ),
        ("added member", with(&added_member)),
        ("added path package", with(&added_path)),
        ("renamed member", with(&renamed)),
        ("duplicate entry", with(&twice)),
        ("ambiguous reference", with(&ambiguous)),
        (
            "patch change",
            render(
                4,
                base,
                "\n[[patch.unused]]\nname = \"fnv\"\nversion = \"1.0.9\"\n",
            ),
        ),
        (
            "unknown package field",
            before.replacen(
                "name = \"fnv\"\nversion = \"1.0.5\"\n",
                "name = \"fnv\"\nversion = \"1.0.5\"\nreplace = \"fnv 1.0.5\"\n",
                1,
            ),
        ),
        (
            "unknown top-level key",
            before.replacen("version = 4\n", "version = 4\nextra = 1\n", 1),
        ),
        (
            "metadata table",
            render(4, base, "\n[metadata]\n\"checksum fnv 1.0.5\" = \"x\"\n"),
        ),
    ]
}

#[test]
fn a_head_the_function_cannot_vouch_for_is_an_error_so_that_everything_runs() {
    let base = workspace();
    let before = with(&base);
    let heads = heads_to_refuse(&base);
    for (label, head) in heads {
        assert_eq!(affected(&before, &head), None, "{label}");
    }
    let git_before = with(&replaced(&base, krate("fnv", "1.0.5", &[]).source(GIT)));
    assert_eq!(
        affected(&git_before, &git_before),
        Some(found(&[])),
        "unchanged git"
    );
    assert_eq!(
        affected(&git_before, &before),
        None,
        "a source change away from git"
    );
    let ghosts = [names(), vec![name("ghost")]].concat();
    assert!(
        lock_affected(&before, &before, &ghosts, &BTreeSet::new()).is_err(),
        "member in no lockfile"
    );
    let removed: Vec<Pkg> = base
        .iter()
        .filter(|package| package.name != "tool")
        .cloned()
        .collect();
    assert_eq!(affected(&before, &with(&removed)), None, "member left out");
}

#[test]
fn a_lockfile_text_or_format_the_function_cannot_read_is_an_error_so_that_everything_runs() {
    let base = workspace();
    let before = render(4, &base, "");
    assert_eq!(affected("not toml [", &before), None, "unparsable base");
    assert_eq!(affected("", &before), None, "empty base");
    assert_eq!(
        affected(&render(3, &base, ""), &render(3, &base, "")),
        Some(found(&[])),
        "same format 3"
    );
    for format in [1, 2, 5] {
        let same = render(format, &base, "");
        assert_eq!(affected(&same, &same), None, "format {format}");
    }
    let unversioned = before.replacen("version = 4\n", "", 1);
    assert_eq!(
        affected(&unversioned, &unversioned),
        None,
        "no format version"
    );
}
