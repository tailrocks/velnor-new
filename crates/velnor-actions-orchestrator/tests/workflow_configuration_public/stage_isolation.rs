// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "stage_isolation/filesystem.rs"]
mod filesystem;
#[path = "stage_isolation/legacy.rs"]
mod legacy;
#[path = "stage_isolation/post_create.rs"]
mod post_create;
#[path = "stage_isolation/replacement.rs"]
mod replacement;
#[path = "stage_isolation/support.rs"]
mod support;
