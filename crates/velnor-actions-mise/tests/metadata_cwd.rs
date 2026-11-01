// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::{error::Error, path::PathBuf};

use velnor_actions_mise::{
    IsolatedCommand, MetadataDiscovery, MetadataQualification, MiseError, ToolCatalog, ToolHomes,
    VerifyPreparedInputs,
};

const RELATIVE_MANIFEST: &str = "tools/probe/Cargo.toml";

fn resolved_manifest(command: &IsolatedCommand) -> Result<PathBuf, Box<dyn Error>> {
    let manifest = PathBuf::from(command.argv().last().ok_or("empty argv")?);
    Ok(match command.cwd() {
        Some(cwd) => cwd.join(manifest),
        None => std::env::current_dir()?.join(manifest),
    })
}

#[test]
fn relative_nested_manifest_resolves_once_from_the_metadata_cwd() -> Result<(), Box<dyn Error>> {
    let catalog = ToolCatalog::pinned();
    let expected = std::env::current_dir()?.join(RELATIVE_MANIFEST);
    let commands = [
        MetadataDiscovery::new(PathBuf::from(RELATIVE_MANIFEST))?.command(&catalog)?,
        MetadataQualification::new(PathBuf::from(RELATIVE_MANIFEST))?.command(&catalog)?,
        VerifyPreparedInputs::new(PathBuf::from(RELATIVE_MANIFEST), ToolHomes::runner_temp())?
            .command(&catalog)?,
    ];
    for command in commands {
        assert_eq!(resolved_manifest(&command)?, expected, "{command:?}");
        assert_eq!(command.cwd(), expected.parent().map(PathBuf::from).as_ref());
    }
    Ok(())
}

#[test]
fn empty_manifest_path_is_rejected_by_every_metadata_constructor() {
    let rejected = MiseError::InvalidManifestPath {
        path: String::new(),
    };
    assert_eq!(
        MetadataDiscovery::new(PathBuf::new()),
        Err(rejected.clone())
    );
    assert_eq!(
        MetadataQualification::new(PathBuf::new()),
        Err(rejected.clone())
    );
    assert_eq!(
        VerifyPreparedInputs::new(PathBuf::new(), ToolHomes::runner_temp()),
        Err(rejected)
    );
}
