// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(crate) fn widen<N: Into<W>, W>(value: N) -> W {
    value.into()
}

#[cfg(test)]
pub(crate) fn narrow<N: TryInto<W>, W>(value: N) -> Option<W> {
    value.try_into().ok()
}
