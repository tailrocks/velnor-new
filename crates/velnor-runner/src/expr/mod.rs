// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod env;
mod error;
mod eval;
mod format;
mod glob;
mod hash_files;
mod index;
mod lexer;
mod memory;
mod node;
mod number;
mod parser;
mod pattern;
mod render;
mod template;
mod token;
mod unit_glob;
mod value;

pub use env::{ExprEnv, JobStatus, RootContext};
pub use error::{ExprError, ParseError, ParseErrorKind};
pub use eval::{evaluate_condition, evaluate_expression};
pub use template::evaluate_template;
pub use value::{ArrayValue, ObjectValue, Value};
