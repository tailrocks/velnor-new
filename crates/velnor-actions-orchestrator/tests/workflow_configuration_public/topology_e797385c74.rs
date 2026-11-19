// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Dep {
    Normal,
    Dev,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Forcing {
    None,
    BuildScript,
    ManifestDirEscape,
    ReadsSibling,
}

pub(super) struct RealPackage {
    pub(super) name: &'static str,
    pub(super) dir: &'static str,
    pub(super) deps: &'static [(&'static str, Dep)],
    pub(super) forcing: Forcing,
}

pub(super) const RUNNER_WORKSPACE: &str = "crates/velnor-runner";

pub(super) const PACKAGES: [RealPackage; 20] = [
    RealPackage {
        name: "velnor-actions-actionlint",
        dir: "crates/velnor-actions-actionlint",
        deps: &[("velnor-actions-contract", Dep::Normal)],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-ansible",
        dir: "crates/velnor-actions-ansible",
        deps: &[("velnor-actions-contract", Dep::Normal)],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-bun",
        dir: "crates/velnor-actions-bun",
        deps: &[("velnor-actions-contract", Dep::Normal)],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-cli",
        dir: "crates/velnor-actions-cli",
        deps: &[
            ("velnor-actions-contract", Dep::Dev),
            ("velnor-actions-freshness", Dep::Normal),
            ("velnor-actions-mise", Dep::Dev),
            ("velnor-actions-orchestrator", Dep::Normal),
        ],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-contract",
        dir: "crates/velnor-actions-contract",
        deps: &[],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-dbt",
        dir: "crates/velnor-actions-dbt",
        deps: &[("velnor-actions-contract", Dep::Normal)],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-freshness",
        dir: "crates/velnor-actions-freshness",
        deps: &[],
        forcing: Forcing::ManifestDirEscape,
    },
    RealPackage {
        name: "velnor-actions-gradle",
        dir: "crates/velnor-actions-gradle",
        deps: &[],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-mise",
        dir: "crates/velnor-actions-mise",
        deps: &[
            ("velnor-actions-contract", Dep::Normal),
            ("velnor-actions-tofu", Dep::Dev),
        ],
        forcing: Forcing::ManifestDirEscape,
    },
    RealPackage {
        name: "velnor-actions-orchestrator",
        dir: "crates/velnor-actions-orchestrator",
        deps: &[
            ("velnor-actions-actionlint", Dep::Normal),
            ("velnor-actions-ansible", Dep::Normal),
            ("velnor-actions-bun", Dep::Normal),
            ("velnor-actions-contract", Dep::Dev),
            ("velnor-actions-contract", Dep::Normal),
            ("velnor-actions-dbt", Dep::Normal),
            ("velnor-actions-gradle", Dep::Normal),
            ("velnor-actions-mise", Dep::Normal),
            ("velnor-actions-rust", Dep::Normal),
            ("velnor-actions-tofu", Dep::Normal),
            ("velnor-actions-workflow-renderer", Dep::Normal),
            ("velnor-archive-guard", Dep::Normal),
        ],
        forcing: Forcing::ManifestDirEscape,
    },
    RealPackage {
        name: "velnor-actions-rust",
        dir: "crates/velnor-actions-rust",
        deps: &[("velnor-actions-contract", Dep::Normal)],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-actions-tofu",
        dir: "crates/velnor-actions-tofu",
        deps: &[("velnor-actions-contract", Dep::Normal)],
        forcing: Forcing::BuildScript,
    },
    RealPackage {
        name: "velnor-actions-workflow-renderer",
        dir: "crates/velnor-actions-workflow-renderer",
        deps: &[
            ("velnor-actions-actionlint", Dep::Normal),
            ("velnor-actions-bun", Dep::Normal),
            ("velnor-actions-contract", Dep::Normal),
            ("velnor-actions-dbt", Dep::Normal),
            ("velnor-actions-mise", Dep::Dev),
            ("velnor-actions-mise", Dep::Normal),
            ("velnor-actions-rust", Dep::Normal),
            ("velnor-actions-tofu", Dep::Normal),
        ],
        forcing: Forcing::ReadsSibling,
    },
    RealPackage {
        name: "velnor-archive-guard",
        dir: "crates/velnor-archive-guard",
        deps: &[],
        forcing: Forcing::ManifestDirEscape,
    },
    RealPackage {
        name: "velnor-resource-probe",
        dir: "crates/velnor-runner/crates/velnor-resource-probe",
        deps: &[],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-runner-attestation",
        dir: "crates/velnor-runner/crates/velnor-runner-attestation",
        deps: &[],
        forcing: Forcing::ManifestDirEscape,
    },
    RealPackage {
        name: "velnor-runner-cli",
        dir: "crates/velnor-runner/crates/velnor-runner-cli",
        deps: &[
            ("velnor-runner-core", Dep::Normal),
            ("velnor-runner-host", Dep::Normal),
        ],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-runner-core",
        dir: "crates/velnor-runner/crates/velnor-runner-core",
        deps: &[],
        forcing: Forcing::None,
    },
    RealPackage {
        name: "velnor-runner-host",
        dir: "crates/velnor-runner/crates/velnor-runner-host",
        deps: &[
            ("velnor-runner-core", Dep::Normal),
            ("velnor-runner-github", Dep::Normal),
        ],
        forcing: Forcing::BuildScript,
    },
    RealPackage {
        name: "velnor-runner-github",
        dir: "crates/velnor-runner/crates/velnor-runner-github",
        deps: &[("velnor-runner-core", Dep::Normal)],
        forcing: Forcing::None,
    },
];
