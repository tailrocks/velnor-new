//! Closed typed selectors for one Nextest test obligation.

use crate::errors::ContractError;
use crate::ids::is_component_byte;
use serde::{Deserialize, Serialize};

/// Supported Nextest selection mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NextestTestSelectionMode {
    /// Run only the listed ignored tests.
    IgnoredOnly,
}

/// Exact test allowlist for one Cargo package and Nextest binary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NextestTestSelection {
    /// Cargo package name.
    pub package: String,
    /// Cargo test target name as reported to Nextest.
    pub binary: String,
    /// Closed execution mode.
    pub mode: NextestTestSelectionMode,
    /// Sorted, duplicate-free exact test identifiers.
    pub test_ids: Vec<String>,
}

impl NextestTestSelection {
    /// Validate safe package/binary components and a sorted exact-test allowlist.
    /// # Errors
    pub fn validate(&self, file: &str, key: &str) -> Result<(), ContractError> {
        for (field, value) in [("package", &self.package), ("binary", &self.binary)] {
            if value.is_empty() || !value.bytes().all(is_component_byte) {
                return Err(ContractError::config(
                    file,
                    format!("{key}.{field}"),
                    format!("bad_component:{value}"),
                ));
            }
        }
        if self.test_ids.is_empty() {
            return Err(ContractError::config(
                file,
                format!("{key}.test_ids"),
                "empty_test_ids",
            ));
        }
        if self.test_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(ContractError::config(
                file,
                format!("{key}.test_ids"),
                "must_be_sorted_unique",
            ));
        }
        if self.test_ids.iter().any(|id| !is_safe_test_id(id)) {
            return Err(ContractError::config(
                file,
                format!("{key}.test_ids"),
                "bad_test_id",
            ));
        }
        Ok(())
    }
}

/// Filter-expression atom grammar excludes DSL operators and quoting.
fn is_safe_test_id(value: &str) -> bool {
    !value.is_empty()
        && !value.starts_with('-')
        && value.bytes().all(|byte| {
            byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'-' | b'.' | b':')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selector(test_ids: Vec<&str>) -> NextestTestSelection {
        NextestTestSelection {
            package: "termrock-tests".to_owned(),
            binary: "control_states".to_owned(),
            mode: NextestTestSelectionMode::IgnoredOnly,
            test_ids: test_ids.into_iter().map(str::to_owned).collect(),
        }
    }

    #[test]
    fn selector_accepts_sorted_exact_ids_and_rejects_unbounded_shapes() {
        assert!(
            selector(vec!["a_test", "b_test"])
                .validate("c", "s")
                .is_ok()
        );
        assert!(selector(vec![]).validate("c", "s").is_err());
        assert!(
            selector(vec!["b_test", "a_test"])
                .validate("c", "s")
                .is_err()
        );
        assert!(
            selector(vec!["a_test", "a_test"])
                .validate("c", "s")
                .is_err()
        );
        assert!(
            selector(vec!["a_test | test(=other)"])
                .validate("c", "s")
                .is_err()
        );
    }

    #[test]
    fn selector_rejects_hostile_components_and_unknown_fields() {
        let mut item = selector(vec!["a_test"]);
        item.package = "${{ secrets.X }}".to_owned();
        assert!(item.validate("c", "s").is_err());
        assert!(serde_json::from_str::<NextestTestSelection>(
            r#"{"package":"p","binary":"b","mode":"ignored_only","test_ids":["t"],"filter":"*"}"#
        )
        .is_err());
    }

    #[test]
    fn rust_stack_requires_sorted_unique_selector_packages() {
        let mut stack = super::super::stacks::RustStackConfig::default_config();
        let mut first = selector(vec!["a_test"]);
        let mut second = selector(vec!["b_test"]);
        first.package = "alpha".to_owned();
        second.package = "alpha".to_owned();
        stack.nextest_test_selectors = vec![first, second];
        let error = stack
            .validate(".velnor/config.toml")
            .expect_err("duplicate package selector fails");
        assert!(
            error
                .to_string()
                .contains("must_be_sorted_unique_by_package")
        );
    }
}
