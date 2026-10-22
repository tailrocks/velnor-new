//! Report and retention bounds of a sharded suite's `[playwright]` table.

use super::super::BunStackConfig;
use serde_json::{Value, json};

fn parse(wire: Value) -> BunStackConfig {
    serde_json::from_value(wire).expect("bun stack")
}

fn problem(wire: Value) -> String {
    parse(wire)
        .validate("config.toml")
        .expect_err("invalid")
        .to_string()
}

#[expect(
    clippy::needless_pass_by_value,
    reason = "json! literals read best passed by value"
)]
fn playwright_suite(playwright: Value) -> Value {
    json!({ "roots": ["."], "members": { "ui": { "suites": {
        "browser": { "scripts": ["test:browser"], "browsers": ["chromium", "webkit"], "playwright": playwright }
    } } } })
}

#[test]
fn report_and_retention_are_bounded_and_omitted_at_default() {
    let wire = playwright_suite(json!({
        "shards": 2,
        "report": "merge",
        "report_retention_days": 30,
        "results_retention_days": 1
    }));
    let config = parse(wire.clone());
    config.validate("config.toml").expect("valid");
    assert_eq!(serde_json::to_value(&config).expect("serialize"), wire);
    let defaults = parse(playwright_suite(json!({
        "shards": 2,
        "report": "none",
        "report_retention_days": 14,
        "results_retention_days": 7
    })));
    assert_eq!(
        serde_json::to_value(&defaults).expect("serialize"),
        playwright_suite(json!({ "shards": 2 }))
    );
    let cases = [
        (
            json!({ "report": "merge" }),
            "report_requires_shards_or_projects:browser",
        ),
        (
            json!({ "shards": 2, "report_retention_days": 0 }),
            "bad_retention_days:0:expected_1..=90",
        ),
        (
            json!({ "shards": 2, "results_retention_days": 91 }),
            "bad_retention_days:91:expected_1..=90",
        ),
    ];
    for (table, code) in cases {
        let got = problem(playwright_suite(table));
        assert!(got.contains(code), "{code}: {got}");
    }
    assert!(
        serde_json::from_value::<BunStackConfig>(playwright_suite(json!({ "report": "html" })))
            .is_err()
    );
    parse(playwright_suite(
        json!({ "projects": ["a", "b"], "report": "merge" }),
    ))
    .validate("config.toml")
    .expect("two legs from projects");
}
