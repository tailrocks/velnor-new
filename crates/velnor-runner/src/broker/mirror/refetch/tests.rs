// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::path::{Path, PathBuf};

use super::adopt_packs;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("velnor-adopt-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("stage/objects/pack")).expect("stage");
    std::fs::create_dir_all(dir.join("mirror/objects/pack")).expect("mirror");
    dir
}

fn put(dir: &Path, name: &str, body: &str) {
    std::fs::write(dir.join(name), body).expect("write");
}

fn read(dir: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(dir.join(name)).ok()
}

#[tokio::test]
async fn an_orphan_index_is_replaced_by_the_staged_pair() {
    let root = scratch("orphan");
    let stage = root.join("stage/objects/pack");
    let mirror = root.join("mirror/objects/pack");
    put(&stage, "pack-a.idx", "staged idx");
    put(&stage, "pack-a.pack", "staged pack");
    put(&mirror, "pack-a.idx", "old idx");
    adopt_packs(&root.join("stage"), &root.join("mirror"))
        .await
        .expect("adopt");
    assert_eq!(read(&mirror, "pack-a.idx").as_deref(), Some("staged idx"));
    assert_eq!(read(&mirror, "pack-a.pack").as_deref(), Some("staged pack"));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_complete_pair_is_left_alone() {
    let root = scratch("complete");
    let stage = root.join("stage/objects/pack");
    let mirror = root.join("mirror/objects/pack");
    put(&stage, "pack-a.idx", "staged idx");
    put(&stage, "pack-a.pack", "staged pack");
    put(&mirror, "pack-a.idx", "old idx");
    put(&mirror, "pack-a.pack", "old pack");
    adopt_packs(&root.join("stage"), &root.join("mirror"))
        .await
        .expect("adopt");
    assert_eq!(read(&mirror, "pack-a.idx").as_deref(), Some("old idx"));
    assert_eq!(read(&mirror, "pack-a.pack").as_deref(), Some("old pack"));
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test]
async fn a_directory_posing_as_the_index_is_replaced() {
    let root = scratch("posing");
    let stage = root.join("stage/objects/pack");
    let mirror = root.join("mirror/objects/pack");
    put(&stage, "pack-a.idx", "staged idx");
    put(&stage, "pack-a.pack", "staged pack");
    std::fs::create_dir_all(mirror.join("pack-a.idx/inner")).expect("posing directory");
    put(&mirror, "pack-a.pack", "old pack");
    adopt_packs(&root.join("stage"), &root.join("mirror"))
        .await
        .expect("adopt");
    assert_eq!(read(&mirror, "pack-a.idx").as_deref(), Some("staged idx"));
    assert_eq!(read(&mirror, "pack-a.pack").as_deref(), Some("staged pack"));
    let _ = std::fs::remove_dir_all(&root);
}
