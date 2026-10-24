use std::os::unix::fs::MetadataExt;
use std::path::Path;

use super::*;

fn test_scratch(label: &str) -> Result<super::PrivateDirectory, String> {
    super::PrivateDirectory::create_test_root(&std::env::temp_dir(), label)
        .map_err(|error| error.to_string())
}

include!("plist.rs");
include!("install.rs");
include!("publication.rs");
include!("uninstall.rs");
