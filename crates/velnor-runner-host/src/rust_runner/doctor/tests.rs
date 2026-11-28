// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::is_private;

#[test]
fn a_jit_directory_is_private_only_when_the_caller_owns_it_with_mode_0700() {
    assert!(is_private(0o040_700, 501, 501));
    assert!(!is_private(0o040_700, 0, 501), "owned by another user");
    assert!(!is_private(0o040_755, 501, 501), "readable by others");
    assert!(!is_private(0o040_750, 501, 501), "readable by the group");
    assert!(
        !is_private(0o040_500, 501, 501),
        "not writable by the owner"
    );
}
