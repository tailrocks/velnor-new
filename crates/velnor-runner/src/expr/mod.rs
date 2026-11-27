// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod anchor;
mod env;
mod error;
mod eval;
mod format;
mod from_json;
mod from_json_depth;
mod from_json_number;
mod glob;
mod hash_files;
mod index;
mod js_class;
mod js_regex;
mod lexer;
mod memory;
mod node;
mod number;
mod parser;
mod pattern;
mod render;
mod seam;
mod segment;
mod template;
mod token;
mod translate;
mod value;
mod walk;

pub use env::{ExprEnv, JobStatus, RootContext};
pub use error::{ExprError, ParseError, ParseErrorKind};
pub use eval::{evaluate_condition, evaluate_expression};
pub use template::evaluate_template;
pub use value::{ArrayValue, ObjectValue, Value};

#[cfg(feature = "test-support")]
pub fn segment_matches(pattern: &str, name: &str) -> Result<bool, String> {
    pattern::Seg::compile(pattern)
        .and_then(|segment| segment.is_match(name))
        .map_err(|error| error.to_string())
}
