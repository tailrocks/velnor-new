// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{DeveloperImages, validate_developer_images};
use crate::error::HostError;

fn images() -> DeveloperImages {
    DeveloperImages {
        runner: format!("sha256:{}", "a".repeat(64)),
        dind: format!("sha256:{}", "b".repeat(64)),
        resource_probe: format!("sha256:{}", "c".repeat(64)),
    }
}

#[test]
fn a_release_build_refuses_developer_images() {
    assert_eq!(
        validate_developer_images(Some(&images()), false, true),
        Err(HostError::DeveloperImages)
    );
    assert_eq!(
        validate_developer_images(Some(&images()), false, false),
        Ok(())
    );
    assert_eq!(validate_developer_images(None, true, true), Ok(()));
}
