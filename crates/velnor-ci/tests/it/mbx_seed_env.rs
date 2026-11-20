// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::PathBuf;
use std::time::Duration;

use velnor_ci::mbx_seed::{Outcome, env_lines};
use velnor_ci::mbx_seed_copy::Copied;

#[test]
fn a_private_store_path_with_a_line_break_cannot_add_an_environment_variable() {
    let seeded = |store: &str| Outcome::Seeded {
        store: PathBuf::from(store),
        copied: Copied::default(),
        elapsed: Duration::ZERO,
    };
    assert_eq!(
        env_lines(&seeded("/runner/mbx")).as_deref().ok(),
        Some("MBX_CACHE_DIR=/runner/mbx\n")
    );
    assert!(env_lines(&seeded("/runner/mbx\nGITHUB_PATH=/evil")).is_err());
}
