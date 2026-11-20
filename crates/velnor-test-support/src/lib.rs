// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

mod real_cargo;

pub use real_cargo::{
    clean_cargo_env, is_mirror_wrapper, mirror_cargo, real_cargo, real_cargo_str,
};
