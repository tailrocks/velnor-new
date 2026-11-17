// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::CiError;
use crate::inputs::{SavedInput, Tracked, format_inputs, parse_inputs};

const VERSION: &str = "velnor-inputs-v1";

pub(super) struct InputSnapshot {
    pub source_identity: String,
    pub inputs: Vec<SavedInput>,
}

impl InputSnapshot {
    fn matches(&self, tracked: &Tracked) -> bool {
        let mut saved: Vec<_> = self.inputs.iter().map(|input| &input.file).collect();
        let mut current: Vec<_> = tracked.files.iter().chain(&tracked.links).collect();
        saved.sort();
        current.sort();
        saved == current
    }
}

pub(super) struct VerifiedInputs {
    pub digest: [u8; 32],
}

pub(super) fn verify(root: &Path, path: &Path) -> Result<VerifiedInputs, CiError> {
    let invalid = || CiError::new("saved inputs do not match the built source records");
    let expected = std::env::var("EXPECTED_SOURCE_IDENTITY").map_err(|_| invalid())?;
    let captured = super::inputs::capture_inputs(root, true)?;
    let text = fs::read_to_string(path).map_err(|error| super::delta::failure(path, &error))?;
    let saved = decode(&text)?;
    if expected.len() != 64
        || captured.source_identity != expected
        || saved.source_identity != expected
        || !saved.matches(&captured.tracked)
    {
        return Err(invalid());
    }
    Ok(VerifiedInputs {
        digest: Sha256::digest(text.as_bytes()).into(),
    })
}

pub(super) fn encode(source_identity: &str, inputs: &[SavedInput]) -> String {
    let body = format_inputs(inputs);
    let digest = binding(source_identity, &body);
    format!("{VERSION}\t{source_identity}\t{digest}\n{body}")
}

pub(super) fn decode(text: &str) -> Result<InputSnapshot, CiError> {
    let invalid = || CiError::new("saved inputs.tsv source binding is missing or inconsistent");
    let (header, body) = text.split_once('\n').ok_or_else(invalid)?;
    let fields: Vec<_> = header.split('\t').collect();
    let [VERSION, source_identity, digest] = fields.as_slice() else {
        return Err(invalid());
    };
    if (!source_identity.is_empty()
        && (source_identity.len() != 64
            || !source_identity.bytes().all(|byte| byte.is_ascii_hexdigit())))
        || *digest != binding(source_identity, body)
    {
        return Err(invalid());
    }
    Ok(InputSnapshot {
        source_identity: (*source_identity).to_owned(),
        inputs: parse_inputs(body)?,
    })
}

fn binding(source_identity: &str, body: &str) -> String {
    velnor_rust::sha256_hex(format!("{source_identity}\n{body}").as_bytes())
}
