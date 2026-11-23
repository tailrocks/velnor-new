// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeMap;
use std::error::Error;

use velnor_model::RepoPath;
use velnor_rust::CONFIG_PATH;

use crate::facts::TestResult;
use crate::private_sources::SECTION;
use crate::release_family_fixture::{generate, tools_like};
use crate::release_fixture::{APP, Root, TOOLS, release_view};
use crate::release_unguarded::expected_release;
use crate::step_refs_shapes::{Keys, Rendered, Shape, sweep_shapes};
use crate::step_refs_sweep::pinned;

const BINARY: &str = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n";
const FAMILY: &str = "family_tag = \"tools-v{{ version }}\"\nroots = [\".\", \"tools\"]\n";
const FAMILY_BINARIES: &str = "[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\"]\n";

struct Release {
    label: &'static str,
    keys: Keys,
    roots: &'static [Root],
    toml: String,
    rust_ci: Option<String>,
}

fn release(label: &'static str, keys: Keys, release: impl Into<String>) -> Release {
    Release {
        label,
        keys,
        roots: &[APP],
        toml: release.into(),
        rust_ci: None,
    }
}

fn release_shapes() -> Vec<Release> {
    let binary = |extra: &str| {
        format!("[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\"]\n{extra}")
    };
    let check = |extra: &str| format!("{BINARY}[[checks]]\n{extra}");
    let mut private = release(
        "private binary",
        &["private_sources"],
        "[[binaries]]\npackage = \"app\"\n",
    );
    private.rust_ci = Some(format!("full_run = \"off\"\n{SECTION}"));
    let mut two_roots = release(
        "after ordering",
        &["after", "roots"],
        "roots = [\".\", \"tools\"]\nafter = [\"CI\", \"Lint\"]\n",
    );
    two_roots.roots = &[APP, TOOLS];
    vec![
        release(
            "pr-only checks",
            &["pr_only_checks"],
            "pr_only_checks = [\"DCO\"]\n",
        ),
        release(
            "raw format",
            &["binaries.format"],
            "[[binaries]]\npackage = \"app\"\nformat = \"raw\"\n",
        ),
        release(
            "versioned asset name",
            &["binaries.asset_name"],
            "[[binaries]]\npackage = \"app\"\nasset_name = \"app-{{ version }}-{{ platform }}\"\n",
        ),
        release(
            "attested versioned asset name",
            &["binaries.asset_name", "binaries.attest"],
            "[[binaries]]\npackage = \"app\"\nasset_name = \"app-{{ version }}-{{ platform }}\"\nattest = true\n",
        ),
        release(
            "release profile",
            &["binaries.profile"],
            binary("profile = \"release\"\n"),
        ),
        release(
            "bins and include",
            &["binaries.bins", "binaries.include"],
            binary("bins = [{ bin = \"rg\" }]\ninclude = [\"LICENSE\"]\n"),
        ),
        release(
            "smoke, target, commit file and checksum files",
            &[
                "binaries.smoke",
                "binaries.target",
                "binaries.commit_file",
                "binaries.checksum_files",
            ],
            binary(
                "smoke = true\ntarget = \"gnu\"\ncommit_file = true\nchecksum_files = \"sha256\"\n",
            ),
        ),
        release(
            "member digests",
            &["binaries.member_digests", "binaries.bins"],
            binary("attest = true\nmember_digests = \"sha256\"\nbins = [{ bin = \"rg\" }]\n"),
        ),
        release(
            "check",
            &[
                "checks",
                "checks.name",
                "checks.run",
                "checks.runs_on",
                "checks.timeout_minutes",
                "checks.permissions",
            ],
            "[[checks]]\nname = \"Lint the repository\"\nrun = \"bash scripts/lint.sh\"\nruns_on = \"ubuntu-24.04\"\ntimeout_minutes = 10\n[checks.permissions]\nactions = \"read\"\n",
        ),
        release(
            "check consuming an artifact",
            &["checks.artifacts", "checks.before"],
            check(
                "name = \"Qualify linux\"\nrun = \"bash scripts/qualify.sh\"\nbefore = \"publish\"\nartifacts = [\"app-linux-x64\"]\n",
            ),
        ),
        release(
            "check needs",
            &["checks.needs"],
            check(
                "name = \"one\"\nrun = \"bash a.sh\"\n[[checks]]\nname = \"two\"\nrun = \"bash b.sh\"\nneeds = [\"one\"]\n",
            ),
        ),
        private,
        two_roots,
    ]
}

impl Shape for Release {
    fn label(&self) -> &'static str {
        self.label
    }

    fn keys(&self) -> Keys {
        self.keys
    }

    fn render(&self, patch: u64) -> Result<Option<Rendered>, Box<dyn Error>> {
        let mut repo = release_view(self.roots, Some(&self.toml))?;
        if let Some(config) = &self.rust_ci {
            repo.files.insert(CONFIG_PATH, config.as_bytes().to_vec());
        }
        let effective = pinned(&mut repo, patch)?;
        Ok(Some((effective, expected_release(&repo)?)))
    }
}

#[test]
fn every_steps_reference_in_a_release_shape_names_a_step_of_its_own_job() -> TestResult {
    sweep_shapes(&release_shapes())
}

struct Family {
    label: &'static str,
    keys: Keys,
    config: String,
}

fn family(label: &'static str, keys: Keys, config: String) -> Family {
    Family {
        label,
        keys,
        config,
    }
}

const FAMILY_BASE: Keys = &[
    "roots",
    "family_tag",
    "binaries.package",
    "binaries.smoke",
    "binaries.platforms",
];

fn family_shapes() -> Vec<Family> {
    let tag_push = "family_trigger = \"tag_push\"\n";
    let attested = format!(
        "{FAMILY}{tag_push}checksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\n{FAMILY_BINARIES}"
    );
    vec![
        family("family", FAMILY_BASE, tools_like("")),
        family(
            "tag-push family",
            &["family_trigger", "commit_file"],
            tools_like("family_trigger = \"tag_push\"\ncommit_file = true\n"),
        ),
        family(
            "family required checks",
            &["family_trigger", "family_required_checks"],
            format!("{FAMILY}{tag_push}family_required_checks = false\n{FAMILY_BINARIES}"),
        ),
        family(
            "attested family",
            &["family_trigger", "checksums_file", "commit_file", "attest"],
            attested,
        ),
        family(
            "optional platforms",
            &["binaries.optional_platforms"],
            format!(
                "{FAMILY}checksums_file = \"BINARY-SHA256SUMS\"\ncommit_file = true\nattest = true\n[[binaries]]\npackage = \"app\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n[[binaries]]\npackage = \"tools\"\nplatforms = [\"linux-x64\", \"macos-arm64\"]\noptional_platforms = [\"macos-arm64\"]\n"
            ),
        ),
    ]
}

impl Shape for Family {
    fn label(&self) -> &'static str {
        self.label
    }

    fn keys(&self) -> Keys {
        self.keys
    }

    fn render(&self, patch: u64) -> Result<Option<Rendered>, Box<dyn Error>> {
        let (text, _) = generate(&format!("0.1.{patch}"), &self.config)?;
        let files = BTreeMap::from([(RepoPath::new(".github/workflows/family.yml")?, text)]);
        Ok(Some((patch, files)))
    }
}

#[test]
fn every_steps_reference_in_a_family_release_shape_names_a_step_of_its_own_job() -> TestResult {
    sweep_shapes(&family_shapes())
}
