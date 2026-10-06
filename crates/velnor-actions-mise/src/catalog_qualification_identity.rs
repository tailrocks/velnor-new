//! Canonical qualification digest binds every immutable authority field.

use super::{
    DistributionAssetFormat, DistributionTool, ProvisioningMode, QualifiedDistribution,
    QualifiedInstallBackend, QualifiedLaunchKind,
};
use sha2::{Digest, Sha256};

impl QualifiedDistribution {
    /// Canonical SHA256 binding every qualification field, including source and behavior.
    #[must_use]
    pub fn qualification_digest(&self) -> String {
        let mut digest = Sha256::new();
        for field in self.identity_fields() {
            digest.update(field.len().to_string().as_bytes());
            digest.update(b":");
            digest.update(field.as_bytes());
        }
        let digest = digest.finalize();
        encode_hex(&digest)
    }

    fn identity_fields(&self) -> Vec<String> {
        let mut fields: Vec<String> = [
            "velnor-qualified-distribution-v1",
            match self.tool {
                DistributionTool::ReleasePlz => "release-plz",
                DistributionTool::CargoSemverChecks => "cargo-semver-checks",
                DistributionTool::Gh => "gh",
                DistributionTool::Mise => "mise",
                DistributionTool::Mbx => "mbx",
                DistributionTool::Bun => "bun",
                DistributionTool::Node => "node",
                DistributionTool::OpenTofu => "opentofu",
                DistributionTool::Python => "python",
                DistributionTool::Uv => "uv",
                DistributionTool::Java => "java",
                DistributionTool::Gradle => "gradle",
            },
            self.host.abi(),
            self.selector,
            self.asset_url,
            self.archive_sha256,
            self.binary_sha256,
            match self.asset_format {
                DistributionAssetFormat::Binary => "binary",
                DistributionAssetFormat::TarGzip => "tar-gzip",
                DistributionAssetFormat::TarXz => "tar-xz",
                DistributionAssetFormat::Zip => "zip",
            },
            self.binary_member,
            self.source_repository,
            self.source_commit,
            self.source_tree,
            self.owner,
            self.version,
            self.selection_version,
            self.abi,
            match self.provisioning_mode {
                ProvisioningMode::Official => "official",
                ProvisioningMode::MiseNoMisercExclusiveConfig => "mise-no-miserc-exclusive-config",
                ProvisioningMode::MbxIsolatedUsefulState => "mbx-isolated-useful-state",
            },
            match self.installed_binary_relative_path {
                Some(_) => "installed-path",
                None => "uninstalled",
            },
            self.installed_binary_relative_path.unwrap_or_default(),
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        self.append_launch_identity(&mut fields);
        self.append_source_identity(&mut fields);
        self.append_install_identity(&mut fields);
        fields
    }

    fn append_launch_identity(&self, fields: &mut Vec<String>) {
        let mut entries = self.launch_entries.to_vec();
        entries.sort_by_key(|entry| (entry.archive_member, entry.installed_relative_path));
        fields.push(entries.len().to_string());
        for entry in entries {
            fields.extend([
                entry.archive_member.to_owned(),
                if entry.installed_relative_path.is_some() {
                    "installed-path"
                } else {
                    "uninstalled"
                }
                .to_owned(),
                entry.installed_relative_path.unwrap_or("").to_owned(),
                entry.sha256.to_owned(),
                match entry.kind {
                    QualifiedLaunchKind::Executable => "executable",
                    QualifiedLaunchKind::Script => "script",
                    QualifiedLaunchKind::JavaArchive => "java-archive",
                }
                .to_owned(),
            ]);
        }
    }

    fn append_source_identity(&self, fields: &mut Vec<String>) {
        let mut sources = self.source_lineage.to_vec();
        sources.sort_by_key(|source| (source.name, source.repository, source.commit));
        fields.push(sources.len().to_string());
        for source in sources {
            fields.extend(
                [
                    source.name,
                    source.repository,
                    source.commit,
                    source.tree,
                    source.version,
                ]
                .into_iter()
                .map(str::to_owned),
            );
        }
    }

    fn append_install_identity(&self, fields: &mut Vec<String>) {
        let Some(plan) = self.install_plan else {
            fields.push("installer-unqualified".to_owned());
            return;
        };
        fields.extend([
            "installer-qualified".to_owned(),
            match plan.backend {
                QualifiedInstallBackend::MiseHttp => "mise-http",
                QualifiedInstallBackend::SourceBoundBootstrap => "source-bound-bootstrap",
            }
            .to_owned(),
            plan.strip_components.to_string(),
            plan.bin_path.to_owned(),
            plan.root_relative_path.to_owned(),
            plan.transform_abi.to_owned(),
        ]);
        let mut environment = plan.environment.to_vec();
        environment.sort_by_key(|entry| (entry.name, entry.relative_path));
        fields.push(environment.len().to_string());
        for entry in environment {
            fields.extend([entry.name.to_owned(), entry.relative_path.to_owned()]);
        }
    }
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(char::from(HEX[usize::from(*byte >> 4)]));
        encoded.push(char::from(HEX[usize::from(*byte & 0x0f)]));
    }
    encoded
}
