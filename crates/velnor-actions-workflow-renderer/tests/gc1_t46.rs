// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;
use std::path::Path;
use std::process::Command;

use sha2::{Digest, Sha256};

const IDENTITY_SOURCE: &str = include_str!("../src/job_image_authority_identity.py");
const REGISTRY_SOURCE: &str = include_str!("../src/job_image_authority_registry.py");
const PUBLISH_SOURCE: &str = include_str!("../src/job_image_authority_publish.py");

const IDENTITY_FILE: &str = "job_image_authority_identity.py";
const REGISTRY_FILE: &str = "job_image_authority_registry.py";
const PUBLISH_FILE: &str = "job_image_authority_publish.py";

fn fail(message: &str) -> Box<dyn Error> {
    io::Error::other(message).into()
}

fn require(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Ok(())
    } else {
        Err(fail(&format!("{label} lost:{needle}")))
    }
}

fn forbid(haystack: &str, needle: &str, label: &str) -> Result<(), Box<dyn Error>> {
    if haystack.contains(needle) {
        Err(fail(&format!("{label} kept:{needle}")))
    } else {
        Ok(())
    }
}

#[path = "gc1_t46_accept_a.rs"]
mod gc1_t46_accept_a;
#[path = "gc1_t46_accept_b.rs"]
mod gc1_t46_accept_b;
#[path = "gc1_t46_driver_a.rs"]
mod gc1_t46_driver_a;
#[path = "gc1_t46_driver_b.rs"]
mod gc1_t46_driver_b;
#[path = "gc1_t46_envelopes.rs"]
mod gc1_t46_envelopes;
#[path = "gc1_t46_inputs.rs"]
mod gc1_t46_inputs;
#[path = "gc1_t46_reject_a.rs"]
mod gc1_t46_reject_a;
#[path = "gc1_t46_reject_b.rs"]
mod gc1_t46_reject_b;
#[path = "gc1_t46_reject_c.rs"]
mod gc1_t46_reject_c;
#[path = "gc1_t46_source.rs"]
mod gc1_t46_source;
pub(crate) use gc1_t46_accept_a::*;
pub(crate) use gc1_t46_accept_b::*;
pub(crate) use gc1_t46_driver_a::*;
pub(crate) use gc1_t46_driver_b::*;
pub(crate) use gc1_t46_envelopes::*;
pub(crate) use gc1_t46_inputs::*;
pub(crate) use gc1_t46_reject_a::*;
pub(crate) use gc1_t46_reject_b::*;
pub(crate) use gc1_t46_reject_c::*;
pub(crate) use gc1_t46_source::*;
