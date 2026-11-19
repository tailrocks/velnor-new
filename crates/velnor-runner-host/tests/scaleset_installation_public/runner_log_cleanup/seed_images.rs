// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::super::super::TestResult;
use super::{DIND_IMAGE_ID, RUNNER_IMAGE_ID};

pub(super) async fn seed_worker_images(
    conn: &turso::Connection,
    root_digest: &str,
) -> TestResult<()> {
    for (role, sha_ch, arch_ch, idx_ch, man_ch, cfg_id, iidx_ch, lsha_ch, lidx_ch) in [
        (
            "velnor-runner",
            "a",
            "1",
            "2",
            "3",
            RUNNER_IMAGE_ID,
            "5",
            "6",
            "7",
        ),
        (
            "velnor-dind",
            "a",
            "9",
            "a",
            "b",
            DIND_IMAGE_ID,
            "c",
            "d",
            "e",
        ),
        (
            "velnor-runner",
            "b",
            "7",
            "2",
            "3",
            RUNNER_IMAGE_ID,
            "5",
            "6",
            "7",
        ),
    ] {
        let (sha, arch_sha) = (sha_ch.repeat(40), arch_ch.repeat(64));
        let image_ref = format!("{role}:v{sha}-{arch_sha}");
        let receipt =
            format!(r#"{{"store":"classic","image_id":"{cfg_id}","repo_tags":["{image_ref}"]}}"#);
        conn.execute(
            "INSERT INTO image_imports (engine_id, docker_root_digest, daemon_architecture, source_sha, archive_sha256, index_digest, platform, manifest_digest, config_digest, attempt, state, image_id, image_index_digest, image_index_size, manifest_size, load_archive_sha256, load_archive_index_digest, image_reference, receipt) VALUES ('engine-1', ?1, 'aarch64', ?2, ?3, ?4, 'linux/amd64', ?5, ?6, 1, 'completed', ?7, ?8, 256, 512, ?9, ?10, ?11, ?12)",
            (
                root_digest,
                sha.as_str(),
                arch_sha.as_str(),
                idx_ch.repeat(64).as_str(),
                format!("sha256:{}", man_ch.repeat(64)).as_str(),
                cfg_id,
                cfg_id,
                format!("sha256:{}", iidx_ch.repeat(64)).as_str(),
                lsha_ch.repeat(64).as_str(),
                lidx_ch.repeat(64).as_str(),
                image_ref.as_str(),
                receipt.as_str(),
            ),
        )
        .await?;
    }
    Ok(())
}
