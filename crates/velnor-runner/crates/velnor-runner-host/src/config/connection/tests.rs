// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::ConnectionId;
use crate::error::HostError;

#[test]
fn dot_only_connection_ids_are_rejected() {
    for value in [".", ".."] {
        assert!(matches!(ConnectionId::parse(value), Err(HostError::Config)));
    }
}

#[test]
fn ordinary_connection_ids_stay_accepted() {
    for value in ["lab", "ci.1"] {
        assert_eq!(
            ConnectionId::parse(value)
                .as_ref()
                .map(ConnectionId::as_str),
            Ok(value)
        );
    }
}
