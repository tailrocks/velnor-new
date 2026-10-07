//! Job-entry bricks: base fields, step attachment, canonical checkout pin.

use crate::yaml::Yaml;

/// Pinned `actions/checkout` used by qualification jobs.
pub const CHECKOUT_USES: &str = "actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1";

/// Initial job fields: name, runs-on, timeout.
#[must_use]
pub fn base(name: &str, runs_on: Yaml, timeout: i64) -> Vec<(String, Yaml)> {
    base_fields(name, runs_on, timeout)
}

fn base_fields(name: &str, runs_on: Yaml, timeout: i64) -> Vec<(String, Yaml)> {
    vec![
        ("name".to_owned(), Yaml::str(name)),
        ("runs-on".to_owned(), runs_on),
        ("timeout-minutes".to_owned(), Yaml::Int(timeout)),
    ]
}

/// Append steps and return one job entry.
#[must_use]
pub fn finish(id: &str, mut fields: Vec<(String, Yaml)>, steps: Vec<Yaml>) -> (String, Yaml) {
    fields.push(("steps".to_owned(), Yaml::Seq(steps)));
    (id.to_owned(), Yaml::Map(fields))
}
