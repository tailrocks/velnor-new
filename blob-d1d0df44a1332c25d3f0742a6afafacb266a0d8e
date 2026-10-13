//! Expected `.alint.yml` legacy policy snapshot (exact rule set plus per-rule pins).

use super::ExpectedRule;

/// Current policy snapshot: exact rule set plus per-rule pins.
pub(crate) const EXPECTED: [ExpectedRule; 7] = [
    ExpectedRule {
        id: "required-files",
        kind: "file_exists",
        paths: &[(
            "paths",
            &[
                "Cargo.toml",
                "Cargo.lock",
                "clippy.toml",
                "deny.toml",
                "rustfmt.toml",
                "CODEOWNERS",
                ".alint.yml",
                ".config/nextest.toml",
                "AGENTS.md",
            ],
        )],
        pairs: &[],
    },
    ExpectedRule {
        id: "claude-is-agents-pointer",
        kind: "command",
        paths: &[
            ("paths.include", &["**/AGENTS.md"]),
            ("paths.exclude", &[".github/**"]),
        ],
        pairs: &[("command", "{path}")],
    },
    ExpectedRule {
        id: "agent-instructions-max-lines",
        kind: "file_max_lines",
        paths: &[("paths", &["**/AGENTS.md"])],
        pairs: &[("max_lines", "100")],
    },
    ExpectedRule {
        id: "agent-instructions-max-size",
        kind: "file_max_size",
        paths: &[("paths", &["**/AGENTS.md"])],
        pairs: &[("max_bytes", "16384")],
    },
    ExpectedRule {
        id: "crates-only",
        kind: "file_absent",
        paths: &[
            ("paths.include", &["**/*.rs"]),
            (
                "paths.exclude",
                &[
                    "crates/*/src/**",
                    "crates/*/tests/**",
                    "crates/*/benches/**",
                    "crates/*/examples/**",
                    "crates/*/fixtures/**",
                    "crates/*/testdata/**",
                    "crates/*/build.rs",
                    "crates/*/build_support/**",
                    "crates/test_support/**",
                    "crates/velnor-runner/crates/*/src/**",
                    "crates/velnor-runner/crates/*/tests/**",
                    "crates/velnor-runner/crates/*/benches/**",
                    "crates/velnor-runner/crates/*/examples/**",
                    "crates/velnor-runner/crates/*/build.rs",
                ],
            ),
        ],
        pairs: &[],
    },
    ExpectedRule {
        id: "rust-max-lines",
        kind: "file_max_lines",
        paths: &[
            (
                "paths.include",
                &[
                    "crates/velnor-actions-*/src/**/*.rs",
                    "crates/velnor-actions-*/tests/**/*.rs",
                    "crates/velnor-archive-guard/src/**/*.rs",
                    "crates/velnor-archive-guard/tests/**/*.rs",
                    "crates/velnor-runner/crates/*/src/**/*.rs",
                    "crates/velnor-runner/crates/*/tests/**/*.rs",
                ],
            ),
            ("paths.exclude", &["**/fixtures/**", "**/testdata/**"]),
        ],
        pairs: &[("max_lines", "400")],
    },
    ExpectedRule {
        id: "lib-main-max-lines",
        kind: "file_max_lines",
        paths: &[(
            "paths.include",
            &[
                "crates/velnor-actions-*/src/lib.rs",
                "crates/velnor-actions-*/src/main.rs",
                "crates/velnor-archive-guard/src/lib.rs",
                "crates/velnor-archive-guard/src/main.rs",
                "crates/velnor-runner/crates/*/src/lib.rs",
                "crates/velnor-runner/crates/*/src/main.rs",
            ],
        )],
        pairs: &[("max_lines", "150")],
    },
];
