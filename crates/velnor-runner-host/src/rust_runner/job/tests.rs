// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{BreachRecord, JobBreach};

#[test]
fn every_kind_the_sandbox_records_is_a_breach_and_an_unknown_one_is_not() {
    for (kind, expected) in [
        ("disk", JobBreach::Disk),
        ("foreign_device", JobBreach::ForeignDevice),
        ("machine_full", JobBreach::MachineFull),
        ("processes", JobBreach::Processes),
        ("memory", JobBreach::Memory),
    ] {
        let record = format!("{{\"breach\":\"{kind}\"}}");
        assert_eq!(
            serde_json::from_str::<BreachRecord>(&record)
                .ok()
                .map(|record| record.breach),
            Some(expected),
            "{kind}"
        );
    }
    for broken in [r#"{"breach":"unheard_of"}"#, "{}", ""] {
        assert!(
            serde_json::from_str::<BreachRecord>(broken).is_err(),
            "{broken}"
        );
    }
}
