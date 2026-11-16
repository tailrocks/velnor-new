// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::os::unix::fs::PermissionsExt;

use super::{COMPACT_SLACK, MINTED_FILE, MintedAgents};

fn lines(state: &std::path::Path) -> usize {
    std::fs::read_to_string(state.join(MINTED_FILE)).map_or(0, |text| text.lines().count())
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
async fn a_torn_or_malformed_line_never_names_an_agent_to_delete() {
    let state = tempfile::tempdir().expect("state");
    std::fs::write(
        state.path().join(MINTED_FILE),
        "+ trusted whole\n+ bogus other\n+ trusted bad/name\n+ trusted a b\n+ trusted cut",
    )
    .expect("seed");
    let journal = MintedAgents::new(state.path());
    let open = journal.load().await.expect("load");
    assert_eq!(open, [("trusted".to_owned(), "whole".to_owned())]);
    journal.intent("trusted", "next").await.expect("intent");
    let reloaded = MintedAgents::new(state.path()).load().await.expect("load");
    assert_eq!(reloaded.len(), 2, "{reloaded:?}");
}
