// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;

use super::{ArgValue, State, arg_value, arg_words, substitute_with_unknown, valid_name};

impl State {
    pub(super) fn record_env(&mut self, args: &str) {
        let Some(words) = arg_words(args) else {
            self.unknown_environment = true;
            self.stage_env.clear();
            return;
        };
        let mut updates = Vec::new();
        for word in words {
            let Some((name, raw)) = word.split_once('=') else {
                self.unknown_environment = true;
                self.stage_env.clear();
                return;
            };
            if !valid_name(name) {
                self.unknown_environment = true;
                self.stage_env.clear();
                return;
            }
            let value = arg_value(raw)
                .and_then(|value| {
                    substitute_with_unknown(&value, &self.variables(), self.unknown_environment)
                })
                .map_or(ArgValue::Unknown, ArgValue::Known);
            updates.push((name.to_owned(), value));
        }
        self.stage_env.extend(updates);
    }

    pub(super) fn variables(&self) -> BTreeMap<String, ArgValue> {
        let mut variables = self.stage_args.clone();
        if self.unknown_environment {
            for (name, value) in &mut variables {
                if !self.stage_env.contains_key(name) {
                    *value = ArgValue::Unknown;
                }
            }
        }
        variables.extend(self.stage_env.clone());
        variables
    }
}
