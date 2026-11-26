// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Annotation, AnnotationLevel};

const MAX_PER_LEVEL: usize = 10;
const MAX_MESSAGE_CHARS: usize = 4096;

#[derive(Debug, Default)]
pub(super) struct Issues {
    items: Vec<Annotation>,
    counts: [usize; 3],
    stop_token: Option<String>,
    command_failed: bool,
}

const fn slot(level: AnnotationLevel) -> usize {
    match level {
        AnnotationLevel::Notice => 0,
        AnnotationLevel::Warning => 1,
        AnnotationLevel::Error => 2,
    }
}

impl Issues {
    pub(super) fn begin_step(&mut self) {
        self.counts = [0; 3];
        self.stop_token = None;
        self.command_failed = false;
    }

    pub(super) fn push(&mut self, mut annotation: Annotation) {
        let count = &mut self.counts[slot(annotation.level)];
        if *count >= MAX_PER_LEVEL {
            return;
        }
        *count += 1;
        if let Some((end, _)) = annotation.message.char_indices().nth(MAX_MESSAGE_CHARS) {
            annotation.message.truncate(end);
        }
        self.items.push(annotation);
    }

    pub(super) fn stop_token(&self) -> Option<&str> {
        self.stop_token.as_deref()
    }

    pub(super) fn stop(&mut self, token: String) {
        self.stop_token = Some(token);
    }

    pub(super) fn resume(&mut self) {
        self.stop_token = None;
    }

    pub(super) fn fail_command(&mut self) {
        self.command_failed = true;
    }

    pub(super) fn take_command_failure(&mut self) -> bool {
        std::mem::take(&mut self.command_failed)
    }

    pub(super) fn into_items(self) -> Vec<Annotation> {
        self.items
    }
}
