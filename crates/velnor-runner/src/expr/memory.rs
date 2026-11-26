// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/eval.rs

use super::error::ExprError;
use super::node::Node;
use super::parser::MAX_LENGTH;
use super::render::template_expression;

const MAX_MEMORY_BYTES: usize = 10 * 1024 * 1024;
const STRING_BASE_OVERHEAD: usize = 26;
const BYTES_PER_UTF16_UNIT: usize = 2;

#[derive(Clone, Copy)]
pub(crate) enum Subject<'a> {
    Node(&'a Node),
    Template(&'a str),
}

impl Subject<'_> {
    fn expression(self) -> String {
        match self {
            Subject::Node(node) => node.to_expression(),
            Subject::Template(source) => {
                let expression = template_expression(source);
                match expression.char_indices().nth(MAX_LENGTH) {
                    Some((end, _)) => format!("{}...", &expression[..end]),
                    None => expression,
                }
            }
        }
    }
}

pub(crate) struct MemoryCounter<'a> {
    subject: Subject<'a>,
    bytes: usize,
}

impl<'a> MemoryCounter<'a> {
    pub(crate) fn new(subject: Subject<'a>) -> Self {
        Self { subject, bytes: 0 }
    }

    pub(crate) fn add(&mut self, value: &str) -> Result<(), ExprError> {
        if value.is_empty() {
            return Ok(());
        }
        let units = value.encode_utf16().count();
        self.add_bytes(
            STRING_BASE_OVERHEAD.saturating_add(units.saturating_mul(BYTES_PER_UTF16_UNIT)),
        )
    }

    pub(crate) fn add_bytes(&mut self, bytes: usize) -> Result<(), ExprError> {
        self.bytes = self.bytes.saturating_add(bytes);
        if self.bytes > MAX_MEMORY_BYTES {
            return Err(ExprError::evaluation(format!(
                "The maximum allowed memory size was exceeded while evaluating the following expression: {}",
                self.subject.expression()
            )));
        }
        Ok(())
    }
}
