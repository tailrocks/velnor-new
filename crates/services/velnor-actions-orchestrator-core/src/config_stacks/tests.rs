use super::*;
use crate::config::{CONFIG_REL, load_config};

/// Write `body` as `.velnor/config.toml` under a fresh temp root.
fn rooted(body: &str) -> tempfile::TempDir {
    let root = tempfile::tempdir().expect("temp root");
    let dir = root.path().join(".velnor");
    std::fs::create_dir_all(&dir).expect("velnor dir");
    std::fs::write(dir.join("config.toml"), body).expect("config write");
    root
}

mod config_stacks_tests;
