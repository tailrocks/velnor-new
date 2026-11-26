// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::commands;
use super::issues::Issues;
use super::mask::{self, Masker};
use super::sink::LogSink;
use super::{Annotation, AnnotationLevel};

pub(super) const LINE_LIMIT: usize = 1 << 20;

pub(super) struct Output<'a> {
    pub(super) number: u32,
    pub(super) sink: &'a mut dyn LogSink,
    pub(super) masker: &'a mut Masker,
    pub(super) issues: &'a mut Issues,
}

impl Output<'_> {
    pub(super) fn enter(&mut self, number: u32) {
        self.number = number;
        self.issues.begin_step();
    }

    pub(super) fn line(&mut self, text: &str) {
        commands::apply(text, self);
        self.message(text);
    }

    pub(super) fn bounded_line(&mut self, text: &str, overflow: bool) {
        commands::apply(text, self);
        if !overflow && text.len() <= LINE_LIMIT {
            self.message(text);
            return;
        }
        let ranges = self.masker.ranges(text);
        let mut end = LINE_LIMIT.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        let kept = mask::redact(text, &ranges, end);
        self.sink.line(
            self.number,
            &format!("{kept} [line truncated at {LINE_LIMIT} bytes]"),
        );
    }

    pub(super) fn message(&mut self, text: &str) {
        let masked = self.masker.mask(text);
        self.sink.line(self.number, &masked);
    }

    pub(super) fn push(&mut self, mut annotation: Annotation) {
        annotation.step = (self.number != 0).then_some(self.number);
        if let std::borrow::Cow::Owned(masked) = self.masker.mask(&annotation.message) {
            annotation.message = masked;
        }
        self.issues.push(annotation);
    }

    pub(super) fn annotate(&mut self, level: AnnotationLevel, message: impl Into<String>) {
        self.push(Annotation::new(level, message));
    }
}
