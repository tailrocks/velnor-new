// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::process::Command;

use velnor_ci::io::cache_delete_entry::{Candidate, delete_by_id, resolved_report};

use crate::cache_delete_entry::{
    KEY, MAIN, MERGE, VERSION_A, VERSION_B, api, entry, refs, repo, resolve_entry,
};

const BASE: &str = "refs/heads/release";

#[test]
fn the_refusal_names_every_earlier_ref_and_the_order_checked() {
    let api = api(vec![entry(11, KEY, MAIN, VERSION_A)]);
    let error = resolve_entry(&api, &repo(), KEY, &refs(&[MERGE, BASE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a later ref must be refused"));
    let text = error.to_string();
    assert!(
        text.contains(&format!(
            "is no longer on {MERGE} or {BASE} (refs checked, in order: {MERGE}, {BASE}, {MAIN};"
        )),
        "{text}"
    );
}

#[test]
fn a_second_job_printing_the_same_notice_is_refused_after_the_first_deleted_its_entry() {
    let api = api(vec![
        entry(11, KEY, MAIN, VERSION_A),
        entry(12, KEY, MERGE, VERSION_A),
    ]);
    let asked = refs(&[MERGE, MAIN]);
    let first = resolve_entry(&api, &repo(), KEY, &asked).unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(first.id, 12);
    delete_by_id(&api, &repo(), KEY, first.id).unwrap_or_else(|e| unreachable!("{e}"));
    let error = resolve_entry(&api, &repo(), KEY, &asked)
        .err()
        .unwrap_or_else(|| unreachable!("the second job's step 1 must refuse"));
    let text = error.to_string();
    assert!(
        text.contains("is no longer on")
            && text.contains("already deleted")
            && !text.contains("--id"),
        "{text}"
    );
    assert_eq!(*api.deleted.borrow(), [12]);
}

#[test]
fn naming_the_later_ref_first_resolves_its_entry() {
    let api = api(vec![entry(11, KEY, MAIN, VERSION_A)]);
    let found = resolve_entry(&api, &repo(), KEY, &refs(&[MAIN, MERGE]))
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert_eq!(found.id, 11);
    assert!(api.deleted.borrow().is_empty());
}

fn printed_arguments(key: &str) -> Vec<String> {
    let found = Candidate {
        id: 12,
        version: VERSION_A.to_owned(),
        reference: MERGE.to_owned(),
    };
    let report = resolved_report(&repo(), key, &found, "https://api.github.com");
    let start = report
        .find("velnor-ci cache delete-entry")
        .unwrap_or_else(|| unreachable!("{report}"));
    let script = format!(
        "args() {{ for a in \"$@\"; do printf '%s\\0' \"$a\"; done; }}\nargs{}",
        &report[start + "velnor-ci".len()..]
    );
    let output = Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .output()
        .unwrap_or_else(|e| unreachable!("{e}"));
    assert!(output.status.success(), "{output:?}");
    let mut pieces: Vec<String> = output
        .stdout
        .split(|byte| *byte == 0)
        .map(|piece| String::from_utf8_lossy(piece).into_owned())
        .collect();
    pieces.pop();
    pieces
}

#[test]
fn the_printed_id_command_passes_awkward_keys_to_the_shell_unchanged() {
    for key in [
        "velnor-a:b",
        "velnor-a%b",
        "'$(id)'",
        "velnor a'b",
        "velnor-$(id)`id`",
        "velnor-a\nb",
    ] {
        assert_eq!(
            printed_arguments(key),
            [
                "cache",
                "delete-entry",
                "--repo",
                "tailrocks/velnor-new",
                "--key",
                key,
                "--id",
                "12"
            ],
            "{key:?}"
        );
    }
}

#[test]
fn the_version_split_refusal_suggests_a_shell_quoted_raw_key() {
    let key = "velnor a'b:c%d";
    let api = api(vec![
        entry(11, key, MAIN, VERSION_A),
        entry(12, key, MERGE, VERSION_B),
    ]);
    let error = resolve_entry(&api, &repo(), key, &refs(&[MERGE, MAIN]))
        .err()
        .unwrap_or_else(|| unreachable!("a version split must be refused"));
    let text = error.to_string();
    assert!(
        text.contains("--id <ID> --key 'velnor a'\\''b:c%d'"),
        "{text}"
    );
    assert!(api.deleted.borrow().is_empty());
}

#[test]
fn a_printed_id_command_names_a_non_default_api_and_never_a_token() {
    let found = Candidate {
        id: 12,
        version: VERSION_A.to_owned(),
        reference: MERGE.to_owned(),
    };
    let default = resolved_report(&repo(), KEY, &found, "https://api.github.com");
    assert!(
        !default.contains("GITHUB_API_URL=") && !default.contains("ghp_"),
        "{default}"
    );
    let enterprise = resolved_report(&repo(), KEY, &found, "https://ghe.example.com/api/v3");
    assert!(
        enterprise.contains(
            "GITHUB_API_URL='https://ghe.example.com/api/v3' velnor-ci cache delete-entry --repo"
        ),
        "{enterprise}"
    );
    assert!(
        enterprise.contains("GITHUB_TOKEN taken from the same source"),
        "{enterprise}"
    );
}
