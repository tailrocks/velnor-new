use super::dockerfile::{DockerfileProblem, analyze_dockerfile, is_digest_pinned};

const D: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn code(text: &str) -> String {
    analyze_dockerfile(text).expect_err("rejected").code()
}

#[test]
fn digest_pinning_requires_a_full_lowercase_sha256() {
    assert!(is_digest_pinned(&format!("debian:12@{D}")));
    assert!(is_digest_pinned(&format!("ghcr.io/o/a@{D}")));
    for bad in [
        "debian:12",
        "debian@sha256:abc",
        "@sha256:00",
        "debian@sha512:ab",
    ] {
        assert!(!is_digest_pinned(bad), "{bad}");
    }
    assert!(!is_digest_pinned(&format!("debian@{}", D.to_uppercase())));
}

#[test]
fn accepts_runtime_only_dockerfile_and_reports_context_sources() {
    let text = format!(
        "# syntax=docker/dockerfile:1@{D}\nFROM --platform=linux/amd64 debian:12@{D} AS run\n\
         RUN apt-get update \\\n && apt-get install -y ca-certificates\n\
         COPY --chown=1000:1000 dist/app /usr/local/bin/app\nCOPY [\"conf/a.toml\", \"/etc/\"]\n"
    );
    let facts = analyze_dockerfile(&text).expect("accepted");
    assert_eq!(facts.external_images.len(), 1);
    let sources = facts.context_sources.into_iter().collect::<Vec<_>>();
    assert_eq!(sources, ["conf/a.toml", "dist/app"]);
}

#[test]
fn unpinned_from_is_rejected_with_the_reference() {
    assert_eq!(code("FROM rust:1.99\n"), "image_base_unpinned:rust:1.99");
    assert!(
        code(&format!("FROM debian@{D}\nFROM node:22 AS b\n")).starts_with("image_base_unpinned")
    );
}

#[test]
fn scratch_and_earlier_stages_need_no_pin() {
    let text = format!("FROM debian@{D} AS build\nFROM scratch AS x\nFROM build\n");
    assert!(analyze_dockerfile(&text).is_ok());
}

#[test]
fn arg_defaults_resolve_and_must_pin() {
    let ok = format!("ARG BASE=debian:12@{D}\nFROM ${{BASE}}\n");
    assert!(analyze_dockerfile(&ok).is_ok());
    assert!(code("ARG BASE=debian:12\nFROM $BASE\n").starts_with("image_base_unpinned"));
    assert!(code("ARG BASE\nFROM ${BASE}\n").starts_with("image_base_unresolved"));
    assert!(code("FROM ${NOPE}\n").starts_with("image_base_unresolved"));
}

#[test]
fn copy_from_external_image_must_be_pinned() {
    let text = format!("FROM debian@{D} AS a\nCOPY --from=a /x /y\nCOPY --from=0 /x /y\n");
    assert!(analyze_dockerfile(&text).is_ok());
    let bad = format!("FROM debian@{D}\nCOPY --from=busybox:1 /bin/sh /sh\n");
    assert_eq!(code(&bad), "image_base_unpinned:busybox:1");
}

#[test]
fn secret_and_ssh_mounts_are_rejected() {
    let base = format!("FROM debian@{D}\n");
    for mount in ["type=secret,id=x", "type=ssh", "id=x,type=secret"] {
        let text = format!("{base}RUN --mount={mount} true\n");
        assert_eq!(
            analyze_dockerfile(&text),
            Err(DockerfileProblem::SecretMount)
        );
    }
    let cache = format!("{base}RUN --mount=type=cache,target=/var/cache/apt apt-get update\n");
    assert!(analyze_dockerfile(&cache).is_ok());
}

#[test]
fn remote_add_context_escape_and_directives_are_rejected() {
    let base = format!("FROM debian@{D}\n");
    assert!(
        code(&format!("{base}ADD https://example.com/a.tgz /a\n")).starts_with("image_add_remote")
    );
    assert!(code(&format!("{base}COPY ../x /x\n")).starts_with("image_context_escape"));
    assert_eq!(
        code(&format!("# escape=`\n{base}")),
        "image_escape_directive"
    );
    assert!(
        code(&format!("# syntax=docker/dockerfile:1\n{base}")).starts_with("image_syntax_unpinned")
    );
}

#[test]
fn heredoc_bodies_are_not_parsed_as_instructions() {
    let text = format!(
        "FROM debian@{D}\nRUN <<EOF\nFROM evil:latest\nCOPY ../x /x\nEOF\nCOPY <<EOT /etc/a\nhello\nEOT\n"
    );
    let facts = analyze_dockerfile(&text).expect("accepted");
    assert!(facts.context_sources.is_empty());
}
