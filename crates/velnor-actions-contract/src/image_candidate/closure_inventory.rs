// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::sync::OnceLock;

use crate::ContractError;

use super::super::IMAGE_CANDIDATE_SOURCE_INPUTS;

pub const IMAGE_NATIVE_INVENTORY_PATH: &str =
    "crates/velnor-actions-contract/src/image_candidate/closure_inventory.txt";

#[derive(Debug, Clone)]
pub struct NativeImageSourceInventory {
    paths: Vec<String>,
}

impl NativeImageSourceInventory {
    pub fn parse(bytes: &[u8]) -> Result<Self, ContractError> {
        let invalid =
            || ContractError::CanonicalJson("image_candidate_source_inventory".to_owned());
        if bytes.len() > 1024 * 1024 || !bytes.ends_with(b"\n") || bytes.contains(&b'\r') {
            return Err(invalid());
        }
        let text = std::str::from_utf8(bytes).map_err(|_| invalid())?;
        let paths = text.lines().map(str::to_owned).collect::<Vec<_>>();
        if paths.len() > 10_000
            || paths.iter().any(|path| !valid_source_path(path))
            || paths.windows(2).any(|pair| pair[0] >= pair[1])
            || !std::iter::once(IMAGE_NATIVE_INVENTORY_PATH)
                .chain(IMAGE_CANDIDATE_SOURCE_INPUTS)
                .all(|path| {
                    paths
                        .binary_search_by(|entry| entry.as_str().cmp(path))
                        .is_ok()
                })
        {
            return Err(invalid());
        }
        Ok(Self { paths })
    }

    pub fn paths(&self) -> impl ExactSizeIterator<Item = &str> {
        self.paths.iter().map(String::as_str)
    }
}

pub(super) fn valid_source_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && path
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"/._+-".contains(&byte))
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

const INVENTORY: &str = include_str!("closure_inventory.txt");

static INPUTS: OnceLock<Vec<&str>> = OnceLock::new();

pub fn native_image_candidate_inputs() -> &'static [&'static str] {
    INPUTS
        .get_or_init(|| INVENTORY.lines().filter(|line| !line.is_empty()).collect())
        .as_slice()
}
