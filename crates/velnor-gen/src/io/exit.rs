// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{Error, Exit};
use crate::check_tag;

pub(super) fn tag_verdict(tag: &str) -> Exit {
    match check_tag(env!("CARGO_PKG_VERSION"), tag) {
        Ok(message) => {
            println!("{message}");
            Exit::SUCCESS
        }
        Err(message) => {
            eprintln!("velnor-gen: {message}");
            Exit::from(1)
        }
    }
}

pub(super) fn finish(result: Result<String, Error>) -> Exit {
    match result {
        Ok(message) => {
            println!("{message}");
            Exit::SUCCESS
        }
        Err(error) => {
            eprintln!("velnor-gen: {error}");
            Exit::from(2)
        }
    }
}
