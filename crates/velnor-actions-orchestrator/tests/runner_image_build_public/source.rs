// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;

#[test]
fn rejects_invalid_committed_or_dirty_sources_before_output() -> Result<(), Box<dyn Error>> {
    frozen_clock(|| {
        for case in ["missing", "oversized", "dirty"] {
            let mut fixture = Fixture::new()?;
            let path = fixture.root.join("scripts/check-runner-image-candidate.sh");
            if case == "missing" {
                fs::remove_file(path)?;
            } else if case == "dirty" {
                fs::write(path, b"uncommitted source")?;
            } else {
                fs::write(path, vec![b'x'; 8 * 1024 * 1024 + 1])?;
            }
            if case != "dirty" {
                git(&fixture.root, &["add", "--all"])?;
                git(
                    &fixture.root,
                    &[
                        "commit",
                        "-s",
                        "--quiet",
                        "-m",
                        "invalid source\n\nCo-authored-by: Codex <codex@openai.com>",
                    ],
                )?;
                fixture.sha = git(&fixture.root, &["rev-parse", "HEAD"])?
                    .trim()
                    .to_owned();
            }
            let error =
                build_runner_images_with_execution(&fixture.request()?, &fixture.execution())
                    .expect_err("accepted invalid committed source");
            assert!(
                error.to_string().contains(if case == "dirty" {
                    "image_candidate_tracked_changes"
                } else {
                    "runner_image_source_file"
                }),
                "{error}"
            );
            assert!(!fixture.output.exists());
        }
        Ok(())
    })
}
