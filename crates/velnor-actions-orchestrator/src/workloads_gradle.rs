//! Closed Gradle checks use the reviewed repository wrapper and module evidence.

use crate::OrchestratorError;
use velnor_actions_contract::FileIndex;

#[path = "workloads_gradle_database.rs"]
mod database;
#[path = "workloads_gradle_wrapper.rs"]
mod wrapper;
pub(super) use database::steps as database_steps;

/// Refuse wrapper substitution and selectors without literal project evidence.
pub(super) fn validate_evidence(
    root: &str,
    project: Option<&str>,
    index: &FileIndex,
) -> Result<(), OrchestratorError> {
    wrapper::validate(root, index)?;
    let settings = evidence_file(root, "settings.gradle.kts", index)?;
    let settings = String::from_utf8(settings).map_err(|_| failed("gradle_settings_not_utf8"))?;
    velnor_actions_native::java::validate_settings(&settings, project)?;
    require_build(root, index)?;
    if let Some(project) = project {
        let project_path = velnor_actions_native::java::project_path(project)?;
        require_build(&relative(root, &project_path), index)?;
    }
    Ok(())
}

fn require_build(root: &str, index: &FileIndex) -> Result<(), OrchestratorError> {
    let Some(file) = ["build.gradle.kts", "build.gradle"]
        .iter()
        .find(|file| index.contains(&relative(root, file)))
    else {
        return Err(failed("gradle_build_manifest_missing"));
    };
    let _ = evidence_file(root, file, index)?;
    Ok(())
}

pub(super) fn relative(root: &str, file: &str) -> String {
    if root == "." {
        file.to_owned()
    } else {
        format!("{root}/{file}")
    }
}

pub(super) fn evidence_file(
    root: &str,
    file: &str,
    index: &FileIndex,
) -> Result<Vec<u8>, OrchestratorError> {
    Ok(evidence_with_mode(root, file, index)?.0)
}

pub(super) fn evidence_with_mode(
    root: &str,
    file: &str,
    index: &FileIndex,
) -> Result<(Vec<u8>, u32), OrchestratorError> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::io::Read as _;
    let relative = relative(root, file);
    if !index.contains(&relative) {
        return Err(failed("gradle_evidence_missing"));
    }
    let dir_flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = open(index.root(), dir_flags, Mode::empty())
        .map_err(|_| failed("gradle_evidence_root_unreadable"))?;
    let mut components = relative.split('/').peekable();
    while let Some(component) = components.next() {
        if components.peek().is_some() {
            directory = openat(&directory, component, dir_flags, Mode::empty())
                .map_err(|_| failed("gradle_evidence_parent_unreadable_or_symlink"))?;
            continue;
        }
        let fd = openat(
            &directory,
            component,
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
            Mode::empty(),
        )
        .map_err(|_| failed("gradle_evidence_unreadable_or_symlink"))?;
        let stat = rustix::fs::fstat(&fd).map_err(|_| failed("gradle_evidence_unreadable"))?;
        if rustix::fs::FileType::from_raw_mode(stat.st_mode) != rustix::fs::FileType::RegularFile {
            return Err(failed("gradle_evidence_not_regular"));
        }
        let mut bytes = Vec::new();
        std::fs::File::from(fd)
            .take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| failed("gradle_evidence_unreadable"))?;
        if bytes.len() > 1_048_576 {
            return Err(failed("gradle_evidence_oversized"));
        }
        return Ok((bytes, stat.st_mode));
    }
    Err(failed("gradle_evidence_missing"))
}

/// Wrapper pins join Java in the native execution identity.
pub(super) fn authority() -> String {
    use velnor_actions_mise::catalog::{
        GRADLE_WRAPPER_DISTRIBUTION_SHA256, GRADLE_WRAPPER_JAR_SHA256,
        GRADLE_WRAPPER_SCRIPT_SHA256, GRADLE_WRAPPER_VERSION, JAVA_VERSION,
    };
    let bootstrap = velnor_actions_mise::catalog::gradle::BOOTSTRAP_VERSION;
    let bootstrap_source = velnor_actions_mise::catalog::gradle::BOOTSTRAP_SOURCE_SHA;
    format!(
        "gradle-wrapper-env-positive-v1\njava:oracle-graalvm-{JAVA_VERSION}\nbootstrap:{bootstrap}\nbootstrap-source:{bootstrap_source}\ngradle-wrapper@{GRADLE_WRAPPER_VERSION}\nscript:{GRADLE_WRAPPER_SCRIPT_SHA256}\njar:{GRADLE_WRAPPER_JAR_SHA256}\ndistribution:{GRADLE_WRAPPER_DISTRIBUTION_SHA256}"
    )
}

pub(super) fn failed(problem: &str) -> OrchestratorError {
    crate::internal::internal(problem)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indexed_evidence_requires_regular_files_and_safe_ancestors()
    -> Result<(), Box<dyn std::error::Error>> {
        let root = tempfile::TempDir::new()?;
        std::fs::write(root.path().join("build.gradle.kts"), "plugins {}")?;
        let paths = vec!["build.gradle.kts".to_owned()];
        let index = velnor_actions_contract::build_index_from_list(root.path(), &paths, &[])?;
        assert_eq!(
            evidence_file(".", "build.gradle.kts", &index)?,
            b"plugins {}"
        );
        std::fs::remove_file(root.path().join("build.gradle.kts"))?;
        std::fs::create_dir(root.path().join("build.gradle.kts"))?;
        assert!(evidence_file(".", "build.gradle.kts", &index).is_err());
        #[cfg(unix)]
        {
            std::fs::remove_dir(root.path().join("build.gradle.kts"))?;
            std::fs::write(root.path().join("real"), "plugins {}")?;
            std::os::unix::fs::symlink("real", root.path().join("build.gradle.kts"))?;
            assert!(evidence_file(".", "build.gradle.kts", &index).is_err());
            std::fs::create_dir(root.path().join("actual"))?;
            std::fs::write(root.path().join("actual/build.gradle.kts"), "plugins {}")?;
            std::os::unix::fs::symlink("actual", root.path().join("module"))?;
            let index = velnor_actions_contract::build_index_from_list(
                root.path(),
                &["module/build.gradle.kts".to_owned()],
                &[],
            )?;
            assert!(evidence_file("module", "build.gradle.kts", &index).is_err());
        }
        Ok(())
    }
}
