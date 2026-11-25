// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::cli_image::ImageReleaseVersionArgs;
use crate::error::CiError;
use crate::image_deploy::release_version::{Release, check};
use crate::io::manifests::{metadata_output, toolchain};

pub(super) fn check_tag(args: &ImageReleaseVersionArgs) -> Result<String, CiError> {
    let toolchain = toolchain()?;
    let root = args.root.canonicalize().map_err(|error| {
        CiError::new(format!("cannot resolve {}: {error}", args.root.display()))
    })?;
    let output = metadata_output(&root, "Cargo.toml", &toolchain)?;
    check(
        &output,
        &Release {
            version: &args.version,
            bins: &args.bins,
            package: args.package.as_deref(),
        },
    )
}
