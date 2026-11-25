// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_gen::{GenerateError, ImageError, expected_files};
use velnor_model::RepoPath;

use crate::facts::TestResult;
use crate::image_workflow::sample;

const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";
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
        format!("FROM debian@sha256:{DIGEST}\nADD https://example.com/tool.tar.gz /opt/\n"),
        format!(
            "FROM debian@sha256:{DIGEST}\nADD --checksum=sha256:abc https://example.com/tool.tar.gz /opt/\n"
        ),
        format!("FROM debian@sha256:{DIGEST}\nADD https://github.com/acme/repo.git#main /src\n"),
        format!("FROM debian@sha256:{DIGEST}\nADD git@github.com:acme/repo.git /src\n"),
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
        format!(
            "FROM debian@sha256:{DIGEST}\nADD --checksum=sha256:{DIGEST} https://example.com/tool.tar.gz /opt/\nADD https://github.com/acme/repo.git#{COMMIT} /src\nADD local.tar /opt/\n"
        ),
    ] {
        assert!(with_dockerfile(&text)?.is_ok(), "{text}");
    }
    Ok(())
}

#[test]
fn a_reference_on_a_continuation_line_is_seen_with_either_escape_character() -> TestResult {
    for text in [
        format!("FROM debian@sha256:{DIGEST}\nCOPY \\\n  --from=alpine:3 /a /a\n"),
        format!("# escape=`\nFROM debian@sha256:{DIGEST}\nCOPY `\n  --from=alpine:3 /a /a\n"),
        format!("FROM debian@sha256:{DIGEST}\nADD \\\n  https://example.com/tool.tar.gz /opt/\n"),
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
fn the_body_of_a_heredoc_is_not_read_as_instructions() -> TestResult {
    let ok = format!(
        "FROM debian@sha256:{DIGEST}\nRUN <<EOF\nFROM alpine:3\nADD https://example.com/x /x\nEOF\nCOPY a /a\n"
    );
    assert!(with_dockerfile(&ok)?.is_ok());
    let after = format!(
        "FROM debian@sha256:{DIGEST}\nRUN <<-EOF\n\tFROM alpine:3\n\tEOF\nCOPY --from=alpine:3 /a /a\n"
    );
    assert!(matches!(
        with_dockerfile(&after)?,
        Err(GenerateError::Image(ImageError::MutableBase { .. }))
    ));
    Ok(())
}

#[test]
fn a_git_source_needs_a_full_commit_and_a_local_add_is_unaffected() -> TestResult {
    let pinned = format!(
        "FROM debian@sha256:{DIGEST}\nADD https://github.com/acme/repo.git?ref={COMMIT} /src\nADD https://github.com/acme/repo.git#{COMMIT}:sub /src2\nADD ./local.tar /opt/\n"
    );
    assert!(with_dockerfile(&pinned)?.is_ok());
    for text in [
        format!("FROM debian@sha256:{DIGEST}\nADD https://github.com/acme/repo.git#v1.0 /src\n"),
        format!(
            "FROM debian@sha256:{DIGEST}\nADD --keep-git-dir=true https://github.com/acme/repo.git /src\n"
        ),
    ] {
        assert!(with_dockerfile(&text)?.is_err(), "{text}");
    }
    Ok(())
}
