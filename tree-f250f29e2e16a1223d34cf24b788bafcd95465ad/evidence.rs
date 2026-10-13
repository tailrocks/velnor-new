//! Recorded freshness evidence, bounded exceptions, and advisory policy.

mod advisories;
mod exceptions;
mod freshness;

use serde_json::Value;

const BLESSED_STANDING: &str = "asamarts/alint";

pub(crate) use advisories::check_advisories;
pub(crate) use exceptions::check_exceptions;
pub(crate) use freshness::check_recorded_freshness;

fn display(value: Option<&Value>) -> String {
    value.map_or_else(|| "null".to_owned(), Value::to_string)
}
