use super::dockerfile::{DockerfileProblem, analyze_dockerfile, is_digest_pinned};

const D: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn code(text: &str) -> String {
    analyze_dockerfile(text).expect_err("rejected").code()
}

