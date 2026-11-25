// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::image_workflow::sample;

const DIGEST: &str = "0f4f2d5c1a9b7e38d6c24a1b9e5f70c3a8d2b46e91f0c7a35b8d4e6f21a09c73";

fn with_dockerfile(text: &str) -> Result<Result<(), GenerateError>, Box<dyn std::error::Error>> {
    let mut view = sample("image-plain")?;
    view.files
        .insert(RepoPath::new("Dockerfile")?, text.as_bytes().to_vec());
    Ok(expected_files(&view).map(|_| ()))
}

#[test]
fn a_base_image_or_copy_source_that_is_not_pinned_by_digest_is_refused() -> TestResult {
    for text in [
        "FROM debian:trixie-slim\n".to_owned(),
        format!("FROM debian@sha256:{}\n", &DIGEST[..40]),
        "ARG BASE=debian\nFROM ${BASE}\n".to_owned(),
        format!("FROM debian@sha256:{DIGEST}\nCOPY --from=alpine:3 /bin/sh /sh\n"),
        format!("FROM debian@sha256:{DIGEST}\nRUN --mount=type=bind,from=alpine:3,target=/x ls\n"),
        format!("FROM debian@sha256:{DIGEST} AS build\nFROM alpine:3\nCOPY --from=build /a /a\n"),
    ] {
        assert!(
            matches!(
                with_dockerfile(&text)?,
                Err(GenerateError::Image(ImageError::MutableBase { .. }))
            ),
            "{text}"
        );
    }
    Ok(())
}

#[test]
fn pinned_references_stage_names_numbers_scratch_comments_and_continuations_are_accepted()
-> TestResult {
    for text in [
        format!("FROM debian@sha256:{DIGEST}\n"),
        format!(
            "# base\nFROM --platform=linux/amd64 debian:trixie@sha256:{DIGEST} AS Build\nFROM scratch\nCOPY --from=build /a /a\nCOPY --from=0 /b /b\n"
        ),
        format!("FROM \\\n  debian@sha256:{DIGEST}\nRUN echo from=nothing\n"),
        format!("FROM debian@sha256:{DIGEST}\nCOPY dist/plain-svc /usr/local/bin/plain-svc\n"),
    ] {
        assert!(with_dockerfile(&text)?.is_ok(), "{text}");
    }
    Ok(())
}
