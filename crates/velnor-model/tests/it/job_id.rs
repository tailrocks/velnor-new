// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::JobId;

#[test]
fn a_job_name_that_could_break_a_printed_line_is_refused() {
    for name in [
        "verify\nhosted",
        "verify\rhosted",
        "::warning::x",
        "a:b",
        "a%0A",
        "a#[b",
        "a b",
    ] {
        assert!(JobId::new(name).is_err(), "{name:?} must not be a job id");
    }
    assert!(JobId::new("verify-hosted_1").is_ok());
}
