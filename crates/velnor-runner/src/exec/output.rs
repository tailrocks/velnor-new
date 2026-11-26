// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::commands;
use super::issues::Issues;
use super::mask::Masker;
use super::sink::LogSink;
use super::{Annotation, AnnotationLevel};

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

    pub(super) fn message(&mut self, text: &str) {
        let masked = self.masker.mask(text);
        self.sink.line(self.number, &masked);
    }

    pub(super) fn push(&mut self, mut annotation: Annotation) {
        if let std::borrow::Cow::Owned(masked) = self.masker.mask(&annotation.message) {
            annotation.message = masked;
        }
        self.issues.push(annotation);
    }

    pub(super) fn annotate(&mut self, level: AnnotationLevel, message: impl Into<String>) {
        self.push(Annotation::new(level, message));
    }
}
