// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use super::{COMPACT_SLACK, MINTED_FILE, MintedAgents, encode};

fn lines(state: &std::path::Path) -> usize {
    std::fs::read_to_string(state.join(MINTED_FILE)).map_or(0, |text| text.lines().filter(|line| !line.is_empty()).count())
}

#[tokio::test]
async fn the_journal_is_created_private_and_stays_bounded_while_agents_come_and_go() {
    let state = tempfile::tempdir().expect("state");
    let journal = MintedAgents::new(state.path());
    journal.intent("trusted", "kept").await.expect("intent");
    let mode = std::fs::metadata(state.path().join(MINTED_FILE))
        .expect("file")
        .permissions()
        .mode()
        & 0o777;
    assert_eq!(mode, 0o600);
    let mut longest = 0;
    for index in 0..500 {
        let name = format!("agent-{index}");
        journal.intent("trusted", &name).await.expect("intent");
        journal.retired("trusted", &name).await.expect("retired");
        longest = longest.max(lines(state.path()));
    }
    assert!(longest <= 2 + COMPACT_SLACK + 2, "{longest}");
    let open = journal.load().await.expect("load");
    assert_eq!(open, [("trusted".to_owned(), "kept".to_owned())]);
}

#[tokio::test]
async fn an_intent_is_refused_when_the_state_directory_entry_cannot_be_synced() {
    let state = tempfile::tempdir().expect("state");
    let journal = MintedAgents::new(state.path());
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o300)).expect("chmod");
    let refused = journal.intent("trusted", "needs-a-synced-entry").await;
    std::fs::set_permissions(state.path(), std::fs::Permissions::from_mode(0o700)).expect("chmod");
    assert!(refused.is_err());
}

#[tokio::test]
async fn a_torn_or_malformed_line_never_names_an_agent_to_delete() {
    let state = tempfile::tempdir().expect("state");
    let whole = encode("+", "trusted", "whole");
    std::fs::write(
        state.path().join(MINTED_FILE),
        format!("{whole}+ bogus other\n+ trusted bad/name\n+ trusted a b\n+ trusted whole\n+ trusted cut"),
    )
    .expect("seed");
    let journal = MintedAgents::new(state.path());
    let open = journal.load().await.expect("load");
    assert_eq!(open, [("trusted".to_owned(), "whole".to_owned())]);
    journal.intent("trusted", "next").await.expect("intent");
    let reloaded = MintedAgents::new(state.path()).load().await.expect("load");
    assert_eq!(reloaded.len(), 2, "{reloaded:?}");
}

#[tokio::test]
async fn a_torn_record_followed_by_a_whole_one_loses_only_the_torn_one() {
    let state = tempfile::tempdir().expect("state");
    let whole = encode("+", "trusted", "kept");
    let torn = &encode("+", "trusted", "cut")[..14];
    std::fs::write(state.path().join(MINTED_FILE), format!("{whole}{torn}")).expect("seed");
    let journal = MintedAgents::new(state.path());
    journal.intent("untrusted", "second").await.expect("intent");
    let open = MintedAgents::new(state.path()).load().await.expect("load");
    assert_eq!(
        open,
        [
            ("trusted".to_owned(), "kept".to_owned()),
            ("untrusted".to_owned(), "second".to_owned())
        ]
    );
}

#[tokio::test]
async fn a_torn_retirement_never_reads_as_a_retirement() {
    let state = tempfile::tempdir().expect("state");
    let intent = encode("+", "trusted", "a");
    let retired = encode("-", "trusted", "a");
    let torn = &retired[..retired.len() - 4];
    std::fs::write(state.path().join(MINTED_FILE), format!("{intent}{torn}")).expect("seed");
    let open = MintedAgents::new(state.path()).load().await.expect("load");
    assert_eq!(open, [("trusted".to_owned(), "a".to_owned())]);
}

#[tokio::test]
async fn a_record_whose_name_was_cut_but_whose_line_ended_is_not_a_record() {
    let state = tempfile::tempdir().expect("state");
    let full = encode("+", "trusted", "abcdef");
    let cut = format!("{}\n", full.replacen("abcdef", "abc", 1).trim_end());
    std::fs::write(state.path().join(MINTED_FILE), cut).expect("seed");
    let open = MintedAgents::new(state.path()).load().await.expect("load");
    assert!(open.is_empty(), "{open:?}");
}
