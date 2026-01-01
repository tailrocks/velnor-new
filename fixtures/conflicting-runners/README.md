# conflicting-runners fixture
Intent: evidence for BOTH ordinary runners —
scripts/test-cargo.sh invokes `cargo test`,
scripts/test-nextest.sh invokes nextest.
Expected detector outcome: REJECTED with
ambiguous_test_runner listing both evidence items.
No guessing; never a mixed runner.
