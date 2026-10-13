//! Per-product asset contracts and immutable release scripts.

use crate::yaml::Yaml;

use super::release_eligibility as eligibility;

const MISE_USES: &str = "jdx/mise-action@9149ea85001c7435d5a66bb127d6a1b6227cb0a5";
const MISE_VERSION: &str = "2026.9.18";
const PREPARE_TEMPLATE: &str = include_str!("schema2_release_prepare.sh");
const PUBLISH_TEMPLATE: &str = include_str!("schema2_release_publish.sh");

/// One independently published family inside the composed release workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Family {
    /// Linux runner and `DinD` image archives.
    Images,
    /// macOS Velnor host binary.
    Binary,
    /// Linux and macOS Velnor generator binaries.
    Generator,
}

impl Family {
    /// Stable release tag prefix retained for existing consumers.
    pub(super) const fn tag_prefix(self) -> &'static str {
        match self {
            Self::Images => "runner",
            Self::Binary => "binary",
            Self::Generator => "generator",
        }
    }

    /// Exact release asset names. Checksum sidecars are attested assets too.
    pub(super) const fn assets(self) -> &'static [&'static str] {
        match self {
            Self::Images => &[
                "velnor-runner-linux-amd64.tar",
                "velnor-dind-linux-amd64.tar",
                "SHA256SUMS",
            ],
            Self::Binary => &["velnor-host", "SHA256SUMS"],
            Self::Generator => &[
                "velnor-actions-0.1.0-x86_64-unknown-linux-gnu",
                "velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256",
                "velnor-actions-0.1.0-aarch64-apple-darwin",
                "velnor-actions-0.1.0-aarch64-apple-darwin.sha256",
            ],
        }
    }

    /// Build and attestation IDs from the existing family DAG.
    pub(super) const fn job_ids(
        self,
    ) -> (
        &'static [&'static str],
        &'static [&'static str],
        &'static str,
    ) {
        match self {
            Self::Images => (&["build-images"], &["attest-images"], "publish-images"),
            Self::Binary => (&["build-binary"], &["attest-binary"], "publish-binary"),
            Self::Generator => (
                &["build-linux", "build-macos"],
                &["attest-linux", "attest-macos"],
                "publish-generator",
            ),
        }
    }

    /// Stable preparation job identity.
    pub(super) const fn prepare_id(self) -> &'static str {
        match self {
            Self::Images => "prepare-images",
            Self::Binary => "prepare-binary",
            Self::Generator => "prepare-generator",
        }
    }

    /// Human-readable family label for job names and release notes.
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Images => "runner images",
            Self::Binary => "velnor-host binary",
            Self::Generator => "velnor-actions generator",
        }
    }

    /// Paths where same-run workflow artifacts are downloaded.
    fn asset_paths(self) -> Vec<String> {
        self.assets()
            .iter()
            .enumerate()
            .map(|(index, asset)| {
                let directory = match self {
                    Self::Images | Self::Binary => "assets",
                    Self::Generator if index < 2 => "linux-assets",
                    Self::Generator => "macos-assets",
                };
                format!("{directory}/{asset}")
            })
            .collect()
    }
}

/// Install Mise and the one canonical GitHub CLI version.
pub(super) fn setup_steps() -> Vec<Yaml> {
    vec![
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("Setup pinned Mise")),
            ("uses".to_owned(), Yaml::str(MISE_USES)),
            (
                "with".to_owned(),
                Yaml::Map(vec![
                    ("cache".to_owned(), Yaml::str("false")),
                    ("env".to_owned(), Yaml::str("false")),
                    ("install".to_owned(), Yaml::str("false")),
                    ("version".to_owned(), Yaml::str(MISE_VERSION)),
                ]),
            ),
        ]),
        Yaml::Map(vec![
            ("name".to_owned(), Yaml::str("Install pinned GitHub CLI")),
            (
                "run".to_owned(),
                Yaml::str(format!(
                    "mise --no-config --no-env --no-hooks install gh@{}",
                    eligibility::GH_VERSION
                )),
            ),
        ]),
    ]
}

/// Validate an existing release or require the exact tag to be absent.
pub(super) fn prepare_script(family: Family) -> String {
    PREPARE_TEMPLATE
        .replace("@REPOSITORY@", eligibility::REPOSITORY)
        .replace("@GH_VERSION@", eligibility::GH_VERSION)
        .replace("@WORKFLOW_PATH@", eligibility::WORKFLOW_PATH)
        .replace("@TAG_PREFIX@", family.tag_prefix())
        .replace("@LABEL@", family.label())
        .replace("@ASSET_NAMES_JSON@", &json_asset_array(family.assets()))
        .replace("@DOWNLOAD_COMMANDS@", &download_commands(family))
        .replace("@CHECKSUM_COMMAND@", &checksum_check(family, "$temp_dir"))
        .replace(
            "@ASSET_VERIFY_COMMANDS@",
            &prepare_asset_verification(family),
        )
}

/// Recheck source eligibility, then publish or idempotently validate one family.
pub(super) fn publish_script(family: Family) -> String {
    PUBLISH_TEMPLATE
        .replace("@ELIGIBILITY_SCRIPT@", &eligibility::publisher_script())
        .replace("@PREPARE_SCRIPT@", &prepare_script(family))
        .replace("@REPOSITORY@", eligibility::REPOSITORY)
        .replace("@GH_VERSION@", eligibility::GH_VERSION)
        .replace("@WORKFLOW_PATH@", eligibility::WORKFLOW_PATH)
        .replace("@TAG_PREFIX@", family.tag_prefix())
        .replace("@LABEL@", family.label())
        .replace("@ASSET_NAMES_JSON@", &json_asset_array(family.assets()))
        .replace("@ASSET_PATHS@", &shell_words(&family.asset_paths()))
        .replace("@CHECKSUM_COMMAND@", &checksum_check(family, "."))
        .replace(
            "@ASSET_VERIFY_COMMANDS@",
            &publish_asset_verification(family),
        )
}

/// Return commands that download each asset into its consumer-compatible path.
fn download_commands(family: Family) -> String {
    match family {
        Family::Images | Family::Binary => format!(
            "mkdir -p \"$temp_dir/assets\" || fail 'could not create release asset directory'\ngh release download \"$prepare_tag\" --repo \"$repository\" --dir \"$temp_dir/assets\" {} || fail 'release asset download failed'",
            download_patterns(family.assets())
        ),
        Family::Generator => format!(
            "mkdir -p \"$temp_dir/linux-assets\" \"$temp_dir/macos-assets\" || fail 'could not create generator asset directories'\ngh release download \"$prepare_tag\" --repo \"$repository\" --dir \"$temp_dir/linux-assets\" {} || fail 'Linux generator asset download failed'\ngh release download \"$prepare_tag\" --repo \"$repository\" --dir \"$temp_dir/macos-assets\" {} || fail 'macOS generator asset download failed'",
            download_patterns(&family.assets()[..2]),
            download_patterns(&family.assets()[2..])
        ),
    }
}

/// Return local checksum validation commands for downloaded or new assets.
fn checksum_check(family: Family, root: &str) -> String {
    match family {
        Family::Images | Family::Binary => {
            format!(
                "(cd \"{root}/assets\" && shasum -a 256 --check SHA256SUMS) || fail 'release asset checksum verification failed'"
            )
        }
        Family::Generator => format!(
            "(cd \"{root}/linux-assets\" && shasum -a 256 --check velnor-actions-0.1.0-x86_64-unknown-linux-gnu.sha256) || fail 'Linux generator checksum verification failed'\n(cd \"{root}/macos-assets\" && shasum -a 256 --check velnor-actions-0.1.0-aarch64-apple-darwin.sha256) || fail 'macOS generator checksum verification failed'"
        ),
    }
}

fn prepare_asset_verification(family: Family) -> String {
    family
        .asset_paths()
        .iter()
        .map(|path| {
            format!(
                "gh release verify-asset \"$prepare_tag\" \"$temp_dir/{path}\" --repo \"$repository\" >/dev/null || fail 'release asset verification failed: {path}'\ngh attestation verify \"$temp_dir/{path}\" --repo \"$repository\" --source-ref refs/heads/main --source-digest \"$source_sha\" --signer-workflow \"$repository/$workflow_path\" --signer-digest \"$authority_sha\" >/dev/null || fail 'asset attestation verification failed: {path}'"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn publish_asset_verification(family: Family) -> String {
    family
        .asset_paths()
        .iter()
        .map(|path| {
            format!(
                "family_gh release verify-asset \"$release_tag\" \"{path}\" --repo \"$release_repository\" >/dev/null\nfamily_gh attestation verify \"{path}\" --repo \"$release_repository\" --source-ref refs/heads/main --source-digest \"$release_source_sha\" --signer-workflow \"$release_repository/$release_workflow_path\" --signer-digest \"$release_authority_sha\" >/dev/null"
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn download_patterns(assets: &[&str]) -> String {
    assets
        .iter()
        .map(|asset| format!("--pattern '{asset}'"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| format!("'{word}'"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn json_asset_array(assets: &[&str]) -> String {
    format!(
        "[{}]",
        assets
            .iter()
            .map(|asset| format!("\"{asset}\""))
            .collect::<Vec<_>>()
            .join(",")
    )
}

/// Captured release verification flags from the pinned GitHub CLI binary.
#[cfg(test)]
pub(super) const GH_RELEASE_CAPABILITIES: &str =
    include_str!("schema2_release_gh_capabilities.txt");
