// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Owned, Scope, read, write};
use crate::config::ConnectionId;

#[test]
fn writing_a_connection_keeps_the_record_of_a_connection_named_like_its_staging_file()
-> Result<(), Box<dyn std::error::Error>> {
    let state = tempfile::tempdir()?;
    let dotted = ConnectionId::parse(".a.tmp")?;
    let plain = ConnectionId::parse("a")?;
    let first = Owned {
        name: "velnor-a".to_owned(),
        runner_group_id: 1,
        scope: Scope::Repository { repository_id: 1 },
        server: "https://example.invalid/".to_owned(),
        scale_set_id: Some(7),
    };
    let second = Owned {
        scale_set_id: Some(8),
        ..first.clone()
    };
    write(state.path(), &dotted, &first)?;
    write(state.path(), &plain, &second)?;
    assert_eq!(read(state.path(), &dotted)?, Some(first));
    assert_eq!(read(state.path(), &plain)?, Some(second));
    Ok(())
}
