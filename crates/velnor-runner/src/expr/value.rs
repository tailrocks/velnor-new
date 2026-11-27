// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0
// SPDX-FileContributor: tailrocks/velnor-legacy@1b9fc0388925d21ff44ba1be5027684c38c8e4c9 crates/velnor-runner/src/expression/value.rs

use super::number::{format_number, parse_number};
use crate::context_value::{
    ordinal_ignore_case_cmp, ordinal_ignore_case_eq, ordinal_ignore_case_fold,
};
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ValueKind {
    Array,
    Boolean,
    Null,
    Number,
    Object,
    String,
}

impl ValueKind {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            ValueKind::Array => "Array",
            ValueKind::Boolean => "Boolean",
            ValueKind::Null => "Null",
            ValueKind::Number => "Number",
            ValueKind::Object => "Object",
            ValueKind::String => "String",
        }
    }
}

#[derive(Debug, Clone)]
pub struct ArrayValue {
    items: Arc<Vec<Value>>,
    filtered: bool,
}

impl ArrayValue {
    #[must_use]
    pub fn new(items: Vec<Value>) -> Self {
        Self {
            items: Arc::new(items),
            filtered: false,
        }
    }

    pub(crate) fn filtered(items: Vec<Value>) -> Self {
        Self {
            items: Arc::new(items),
            filtered: true,
        }
    }

    pub(crate) fn is_filtered(&self) -> bool {
        self.filtered
    }

    pub(crate) fn items(&self) -> &[Value] {
        &self.items
    }

    fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.items, &other.items)
    }
}

#[derive(Debug, Clone)]
pub struct ObjectValue {
    entries: Arc<Vec<(String, Value)>>,
    case_sensitive: bool,
}

impl ObjectValue {
    #[must_use]
    pub fn new(entries: Vec<(String, Value)>) -> Self {
        Self {
            entries: Arc::new(entries),
            case_sensitive: false,
        }
    }

    pub(crate) fn case_sensitive(entries: Vec<(String, Value)>) -> Self {
        Self {
            entries: Arc::new(entries),
            case_sensitive: true,
        }
    }

    pub(crate) fn entries(&self) -> &[(String, Value)] {
        &self.entries
    }

    pub(crate) fn get(&self, key: &str) -> Option<&Value> {
        self.entries
            .iter()
            .find(|(name, _)| {
                if self.case_sensitive {
                    name == key
                } else {
                    ordinal_ignore_case_eq(name, key)
                }
            })
            .map(|(_, value)| value)
    }

    fn same_instance(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.entries, &other.entries)
    }
}

#[derive(Debug, Clone)]
pub enum Value {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(ArrayValue),
    Object(ObjectValue),
}

impl Value {
    pub(crate) fn kind(&self) -> ValueKind {
        match self {
            Value::Null => ValueKind::Null,
            Value::Boolean(_) => ValueKind::Boolean,
            Value::Number(_) => ValueKind::Number,
            Value::String(_) => ValueKind::String,
            Value::Array(_) => ValueKind::Array,
            Value::Object(_) => ValueKind::Object,
        }
    }

    pub(crate) fn is_primitive(&self) -> bool {
        matches!(
            self,
            Value::Null | Value::Boolean(_) | Value::Number(_) | Value::String(_)
        )
    }

    #[must_use]
    pub fn is_falsy(&self) -> bool {
        match self {
            Value::Null => true,
            Value::Boolean(value) => !*value,
            Value::Number(value) => *value == 0.0 || value.is_nan(),
            Value::String(value) => value.is_empty(),
            Value::Array(_) | Value::Object(_) => false,
        }
    }

    #[must_use]
    pub fn is_truthy(&self) -> bool {
        !self.is_falsy()
    }

    #[must_use]
    pub fn convert_to_string(&self) -> String {
        match self {
            Value::Null => String::new(),
            Value::Boolean(value) => value.to_string(),
            Value::Number(value) => format_number(*value),
            Value::String(value) => value.clone(),
            other => other.kind().as_str().to_string(),
        }
    }

    pub(crate) fn convert_to_number(&self) -> f64 {
        match self {
            Value::Null => 0.0,
            Value::Boolean(value) => f64::from(u8::from(*value)),
            Value::Number(value) => *value,
            Value::String(value) => parse_number(value),
            Value::Array(_) | Value::Object(_) => f64::NAN,
        }
    }

    pub(crate) fn abstract_equal(&self, right: &Value) -> bool {
        let (left, right) = coerce_types(self.clone(), right.clone());
        match (&left, &right) {
            (Value::Null, Value::Null) => true,
            (Value::Number(left), Value::Number(right)) => {
                left.partial_cmp(right) == Some(Ordering::Equal)
            }
            (Value::String(left), Value::String(right)) => ordinal_ignore_case_eq(left, right),
            (Value::Boolean(left), Value::Boolean(right)) => left == right,
            (Value::Object(left), Value::Object(right)) => left.same_instance(right),
            (Value::Array(left), Value::Array(right)) => left.same_instance(right),
            _ => false,
        }
    }

    pub(crate) fn abstract_not_equal(&self, right: &Value) -> bool {
        !self.abstract_equal(right)
    }

    fn compare(&self, right: &Value) -> Option<Ordering> {
        let (left, right) = coerce_types(self.clone(), right.clone());
        match (&left, &right) {
            (Value::Number(left), Value::Number(right)) => left.partial_cmp(right),
            (Value::String(left), Value::String(right)) => {
                Some(ordinal_ignore_case_cmp(left, right))
            }
            (Value::Boolean(left), Value::Boolean(right)) => Some(left.cmp(right)),
            _ => None,
        }
    }

    pub(crate) fn abstract_greater_than(&self, right: &Value) -> bool {
        self.compare(right) == Some(Ordering::Greater)
    }

    pub(crate) fn abstract_less_than(&self, right: &Value) -> bool {
        self.compare(right) == Some(Ordering::Less)
    }

    pub(crate) fn abstract_greater_than_or_equal(&self, right: &Value) -> bool {
        self.abstract_equal(right) || self.abstract_greater_than(right)
    }

    pub(crate) fn abstract_less_than_or_equal(&self, right: &Value) -> bool {
        self.abstract_equal(right) || self.abstract_less_than(right)
    }
}

fn coerce_types(left: Value, right: Value) -> (Value, Value) {
    let left_kind = left.kind();
    let right_kind = right.kind();

    if left_kind == right_kind {
        return (left, right);
    }

    match (left_kind, right_kind) {
        (ValueKind::Number, ValueKind::String) => {
            let right = Value::Number(right.convert_to_number());
            (left, right)
        }
        (ValueKind::String, ValueKind::Number) => {
            let left = Value::Number(left.convert_to_number());
            (left, right)
        }
        _ if matches!(left_kind, ValueKind::Boolean | ValueKind::Null) => {
            let left = Value::Number(left.convert_to_number());
            coerce_types(left, right)
        }
        _ if matches!(right_kind, ValueKind::Boolean | ValueKind::Null) => {
            let right = Value::Number(right.convert_to_number());
            coerce_types(left, right)
        }
        _ => (left, right),
    }
}

pub(crate) fn ordinal_ignore_case_starts_with(value: &str, prefix: &str) -> bool {
    let prefix_chars = prefix.chars().count();
    let end = value
        .char_indices()
        .nth(prefix_chars)
        .map_or(value.len(), |(index, _)| index);
    ordinal_ignore_case_eq(&value[..end], prefix)
}

pub(crate) fn ordinal_ignore_case_ends_with(value: &str, suffix: &str) -> bool {
    let start = match suffix.chars().count() {
        0 => value.len(),
        count => value
            .char_indices()
            .rev()
            .nth(count - 1)
            .map_or(0, |(index, _)| index),
    };
    ordinal_ignore_case_eq(&value[start..], suffix)
}

fn folded_bytes(value: &str) -> impl Iterator<Item = u8> + '_ {
    ordinal_ignore_case_fold(value).flat_map(|character| {
        let mut encoded = [0_u8; 4];
        let length = character.encode_utf8(&mut encoded).len();
        encoded.into_iter().take(length)
    })
}

fn widen(value: u32) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

fn narrow(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

pub(crate) fn ordinal_ignore_case_contains(value: &str, needle: &str) -> bool {
    let needle: Vec<u8> = folded_bytes(needle).collect();
    if needle.is_empty() {
        return true;
    }
    if u32::try_from(needle.len()).is_err() {
        return false;
    }
    let mut fallback = vec![0_u32; needle.len()];
    let mut matched = 0_usize;
    for index in 1..needle.len() {
        while matched > 0 && needle[index] != needle[matched] {
            matched = widen(fallback[matched - 1]);
        }
        if needle[index] == needle[matched] {
            matched += 1;
        }
        fallback[index] = narrow(matched);
    }
    matched = 0;
    for byte in folded_bytes(value) {
        while matched > 0 && byte != needle[matched] {
            matched = widen(fallback[matched - 1]);
        }
        if byte == needle[matched] {
            matched += 1;
        }
        if matched == needle.len() {
            return true;
        }
    }
    false
}
