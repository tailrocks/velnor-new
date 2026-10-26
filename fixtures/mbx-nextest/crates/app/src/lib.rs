// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub fn ok() -> bool {
    true
}

#[cfg(test)]
mod tests {
    #[test]
    fn is_ok() {
        assert!(super::ok());
    }
}
