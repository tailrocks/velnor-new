// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/mod.rs

use super::env::{Function, RootContext};
use super::value::Value;

#[derive(Debug, Clone)]
pub(crate) enum Node {
    Literal(Value),
    Wildcard,
    NamedValue(RootContext),
    Index(Box<Node>, Box<Node>),
    Not(Box<Node>),
    And(Vec<Node>),
    Or(Vec<Node>),
    Binary(BinaryOperator, Box<Node>, Box<Node>),
    Function {
        function: Function,
        name: String,
        args: Vec<Node>,
    },
}

impl Node {
    pub(crate) fn depth(&self) -> usize {
        1 + self
            .children()
            .into_iter()
            .map(Node::depth)
            .max()
            .unwrap_or(0)
    }

    pub(crate) fn children(&self) -> Vec<&Node> {
        match self {
            Node::Literal(_) | Node::Wildcard | Node::NamedValue(_) => Vec::new(),
            Node::Not(inner) => vec![inner.as_ref()],
            Node::Index(left, right) | Node::Binary(_, left, right) => {
                vec![left.as_ref(), right.as_ref()]
            }
            Node::And(nodes) | Node::Or(nodes) => nodes.iter().collect(),
            Node::Function { args, .. } => args.iter().collect(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinaryOperator {
    Equal,
    NotEqual,
    GreaterThan,
    GreaterThanOrEqual,
    LessThan,
    LessThanOrEqual,
}
