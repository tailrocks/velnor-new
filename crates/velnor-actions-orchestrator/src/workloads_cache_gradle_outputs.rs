//! Source-bound eligibility for one audited native Java compiler obligation.

use std::path::Path;

use sha2::{Digest, Sha256};
use velnor_actions_contract::FileIndex;

#[path = "workloads_cache_gradle_policy.rs"]
mod policy;

const ROOT: &str = "backend";
const BUILD_LOGIC_DIGEST: &str = "a46f04882d38bc847c8a614a0c5575bdb50fb8a2f8e50836468fa9f5c919e754";
const SOURCE: &str = "processor-target-validation/src/main/java/com/chainargos/processor/binding/RpcEndpointBinding.java";
const REVIEWED: [(&str, &str); 7] = [
    (
        "build.gradle.kts",
        "9bf7e3a4077ef56450652f2c46cafd503debd697504ff1a0da88731377175e27",
    ),
    (
        "settings.gradle.kts",
        "02677c115c7ed08c69657672cea1b9a6bbcb431f331a53a7cd8ae50fdf6e0a65",
    ),
    (
        "gradle.properties",
        "549b12d0468635c7fdd151e39406708b75a6068cb6af86e1af2a761538b1b02a",
    ),
    (
        "gradle/libs.versions.toml",
        "92a07aed3a3a0789356439278232ec68e128d060e8b7c870bb99172256b5a152",
    ),
    (
        "gradle/wrapper/gradle-wrapper.properties",
        "d8bfc5d38a726e7ecb4830c8503cafd0080a27527c073c2a64ed335df5773d90",
    ),
    (
        "processor-target-validation/build.gradle.kts",
        "cdbc352e74666792d814dbc8229e97dc090d8eb27c6e626b5a6154af7893915d",
    ),
    (
        SOURCE,
        "5860e10d5ce8657e00b38824fd8a68888bfab44397e9aaa8fd8df300a9c43ba4",
    ),
];

/// The private constructor prevents arbitrary task selectors granting reuse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct GradleOutputContract {
    root: String,
}

impl GradleOutputContract {
    /// Recipe compatibility; exact tool source identities and platform are caller key inputs.
    pub(crate) fn authority(&self) -> String {
        let reviewed = REVIEWED
            .iter()
            .map(|(path, hash)| format!("{path}:{hash}"))
            .collect::<Vec<_>>()
            .join("\n");
        let artifacts = super::gradle_artifacts::artifact_descriptors()
            .iter()
            .map(|artifact| {
                format!(
                    "{}:{}:{}:{}",
                    artifact.group, artifact.module, artifact.version, artifact.jar_sha256
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let recipe = [
            include_str!("workloads_cache_gradle.rs"),
            include_str!("workloads_cache_gradle_outputs.rs"),
            include_str!("workloads_cache_gradle_producer.rs"),
            include_str!("workloads_cache_gradle_producer_prepare.py"),
            include_str!("workloads_cache_gradle_producer_export.py"),
            include_str!("workloads_cache_gradle_import.py"),
            include_str!("workloads_cache_gradle_native_entry.py"),
            include_str!("workloads_cache_gradle_producer.init.gradle"),
            include_str!("workloads_cache_gradle_policy.init.gradle"),
            include_str!("workloads_cache_gradle_policy.rs"),
            include_str!("workloads_cache_gradle_policy_prepare.py"),
        ]
        .join("\n");
        velnor_actions_contract::digest_b3(
            format!(
                "gradle-native-output-v1\n{}\n{}\n{BUILD_LOGIC_DIGEST}\n{reviewed}\n{artifacts}\n{recipe}",
                self.root,
                crate::workloads::gradle::authority(),
            )
            .as_bytes(),
        )
    }

    /// Sealed producer export; consumer-owned writable state is never archived.
    pub(crate) fn payload_paths(&self) -> Vec<String> {
        vec!["${{ runner.temp }}/velnor/native/gradle/velnor-compile-export-v1".to_owned()]
    }

    /// Closed command flags preserve native keys and prohibit config reuse.
    pub(crate) fn arguments(&self) -> Vec<String> {
        [
            "--build-cache",
            "--no-configuration-cache",
            "--init-script",
            "${{ runner.temp }}/velnor/native/gradle/velnor-output-policy.init.gradle",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect()
    }

    /// Materialize the compiler policy before restoring its owned content store.
    pub(crate) fn prepare_step(
        &self,
    ) -> Result<velnor_actions_contract::Step, crate::OrchestratorError> {
        policy::prepare_step(&self.root)
    }
}

/// Exact audited source evidence grants only the fixed native compiler policy.
/// Changed or uninspected build logic remains valid cold work without transport.
pub(crate) fn eligibility(index: &FileIndex, root: &str) -> Option<GradleOutputContract> {
    if root != ROOT || index.skipped_non_utf8() {
        return None;
    }
    for (relative, digest) in REVIEWED {
        let path = format!("{ROOT}/{relative}");
        if !index.contains(&path) || !reviewed_file(index.root(), &path, digest) {
            return None;
        }
    }
    let directory = index.root().join(ROOT);
    if directory.join("buildSrc").exists() || directory.join("build-logic").exists() {
        return None;
    }
    if build_logic_digest(&directory)? != BUILD_LOGIC_DIGEST {
        return None;
    }
    Some(GradleOutputContract {
        root: root.to_owned(),
    })
}

fn build_logic_digest(root: &Path) -> Option<String> {
    let mut directories = vec![root.to_path_buf()];
    let mut scripts = std::collections::BTreeMap::new();
    let mut entries = 0_u32;
    while let Some(directory) = directories.pop() {
        for entry in std::fs::read_dir(directory).ok()? {
            entries = entries.checked_add(1)?;
            if entries > 100_000 {
                return None;
            }
            let entry = entry.ok()?;
            let kind = entry.file_type().ok()?;
            if kind.is_symlink() {
                return None;
            }
            if kind.is_dir() {
                directories.push(entry.path());
            } else if kind.is_file() {
                let path = entry.path();
                let relative = path.strip_prefix(root).ok()?.to_str()?;
                if relative.ends_with(".gradle") || relative.ends_with(".gradle.kts") {
                    scripts.insert(
                        relative.to_owned(),
                        Sha256::digest(read_source_file(root, relative)?),
                    );
                }
            }
        }
    }
    let mut digest = Sha256::new();
    for (path, bytes) in scripts {
        digest.update(path.as_bytes());
        digest.update([0]);
        digest.update(bytes);
    }
    Some(format!("{:x}", digest.finalize()))
}

#[cfg(test)]
#[path = "workloads_cache_gradle_outputs_tests.rs"]
mod tests;

fn reviewed_file(repository: &Path, relative: &str, digest: &str) -> bool {
    read_source_file(repository, relative)
        .is_some_and(|bytes| format!("{:x}", Sha256::digest(bytes)) == digest)
}

fn read_source_file(repository: &Path, relative: &str) -> Option<Vec<u8>> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::io::Read as _;
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open(repository, flags, Mode::empty()).ok()?;
    let mut components = relative.split('/').peekable();
    while let Some(component) = components.next() {
        if component.is_empty() || matches!(component, "." | "..") {
            return None;
        }
        if components.peek().is_some() {
            directory = openat(&directory, component, flags, Mode::empty()).ok()?;
            continue;
        }
        let fd = openat(
            &directory,
            component,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .ok()?;
        let stat = rustix::fs::fstat(&fd).ok()?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile
            || stat.st_nlink != 1
            || stat.st_size > 1_048_576
        {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::from(fd)
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .ok()?;
        return (bytes.len() <= 1_048_576).then_some(bytes);
    }
    None
}
