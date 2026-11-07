// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub(super) type Phase = (&'static str, &'static str, bool);

pub(super) const GATHER_JOB_ID: &str = "source-qualification";
pub(super) const GATHER_JOB_NAME: &str = "Source qualification";
pub(super) const GATHER_PHASE: Phase = (
    "Bundle selected qualification evidence",
    "bundle-selected-qualification-evidence",
    true,
);

pub(super) const PREPARE_PHASE: Phase = (
    "Prepare external evidence directories",
    "prepare-external-evidence-directories",
    true,
);

pub(super) const VERIFY_CHECKOUT_PHASE: Phase = (
    "Verify exact clean source checkout",
    "verify-exact-clean-source-checkout",
    false,
);

pub(super) const PRELUDE_AFTER_MISE_BEFORE_PREFETCH: &[Phase] = &[
    (
        "Read and verify repository tool pins",
        "read-and-verify-repository-tool-pins",
        false,
    ),
    (
        "Install pinned Rust and MBX",
        "install-pinned-rust-and-mbx",
        false,
    ),
    (
        "Record installed tool identities",
        "record-installed-tool-identities",
        false,
    ),
];

pub(super) const PREFETCH_PHASE: Phase = (
    "Prefetch locked sources for qualification workspaces",
    "prefetch-locked-sources-for-qualification-workspaces",
    false,
);

pub(super) const PRELUDE_AFTER_PREFETCH: &[Phase] = &[
    (
        "Install validator tools with native Mise checksums",
        "install-validator-tools-with-native-mise-checksums",
        false,
    ),
    (
        "Test validator tool lock admission",
        "test-validator-tool-lock-admission",
        false,
    ),
    (
        "Build source CLI and archive guard concurrently",
        "build-source-cli-and-archive-guard-concurrently",
        false,
    ),
    (
        "Capture all five external goldens",
        "capture-all-five-external-goldens",
        true,
    ),
];

pub(super) struct ProducerJob {
    pub(super) id: &'static str,
    pub(super) name: &'static str,
    pub(super) phases: &'static [Phase],
}

pub(super) const PRODUCER_JOBS: &[ProducerJob] = &[
    ProducerJob {
        id: "ordinary-harness",
        name: "Source qualification ordinary harness",
        phases: &[
            (
                "Run full golden shell harness against external oracle",
                "run-full-golden-shell-harness-against-external-oracle",
                true,
            ),
            (
                "Preserve ordinary output-binding JSON proof files",
                "preserve-ordinary-output-binding-json-proof-files",
                true,
            ),
        ],
    },
    ProducerJob {
        id: "keep-harness",
        name: "Source qualification keep harness",
        phases: &[
            (
                "Run full golden shell harness with retained workspaces",
                "run-full-golden-shell-harness-with-retained-workspaces",
                true,
            ),
            (
                "Preserve keep output-binding JSON proof files",
                "preserve-keep-output-binding-json-proof-files",
                true,
            ),
        ],
    },
    ProducerJob {
        id: "generation-checks",
        name: "Source qualification generation and repository checks",
        phases: &[
            (
                "Observe hosted Docker identity",
                "observe-hosted-docker-identity",
                false,
            ),
            (
                "Author Linux x64 native validator candidates",
                "qualify-linux-x64-native-validator-tools",
                false,
            ),
            (
                "Repeat check against external oracle",
                "repeat-check-against-external-oracle",
                true,
            ),
            (
                "Generate twice and retain root-to-preview identities",
                "generate-twice-and-retain-root-to-preview-identities",
                true,
            ),
            (
                "Test generated Rust verifier transport and digest gate",
                "test-generated-rust-verifier-transport-and-digest-gate",
                true,
            ),
            (
                "Check source output against checked-in repository goldens",
                "check-source-output-against-checked-in-repository-goldens",
                true,
            ),
        ],
    },
];
