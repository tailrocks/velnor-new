// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::time::Duration;

use zeroize::Zeroizing;

use super::github_env;
use crate::cli::ImagePublishArgs;
use crate::error::CiError;

mod flow;
mod gh;
mod parallel;
mod push;
mod registry;
mod transport;

pub use push::OpenBlob;
pub use registry::Registry;
pub use transport::{Body, Https, Request, Response, Transport};

const TOKEN_VARIABLE: &str = "GH_TOKEN";
const ARTIFACT_VARIABLE: &str = "VELNOR_CANDIDATE_ARTIFACT_ID";

pub struct Run<'a> {
    pub vars: HashMap<String, String>,
    pub token: Zeroizing<String>,
    pub gh: &'a Path,
    pub transport: &'a dyn Transport,
    pub pause: &'a (dyn Fn(Duration) + Sync),
}

impl fmt::Debug for Run<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Run").finish_non_exhaustive()
    }
}

pub fn publish_with(args: &ImagePublishArgs, run: &Run<'_>) -> Result<String, CiError> {
    flow::publish(args, run)
}

pub(in crate::io) fn publish(args: &ImagePublishArgs) -> Result<String, CiError> {
    let mut vars = github_env();
    if let Ok(value) = std::env::var(ARTIFACT_VARIABLE) {
        vars.insert(ARTIFACT_VARIABLE.to_owned(), value);
    }
    let token = Zeroizing::new(std::env::var(TOKEN_VARIABLE).unwrap_or_default());
    let transport = Https::new();
    publish_with(
        args,
        &Run {
            vars,
            token,
            gh: Path::new("gh"),
            transport: &transport,
            pause: &std::thread::sleep,
        },
    )
}
