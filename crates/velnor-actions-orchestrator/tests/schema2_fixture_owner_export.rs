//! Opt-in export of the raw schema-2 fixture inputs and exact rendered tree.

use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use velnor_actions_orchestrator::{prepare, render_staged_tree};
use velnor_actions_workflow_renderer::{RenderedTree, tree::RenderedSymlink};

use crate::impl_common::{TestResult, make_repo};

use super::workflow_config;

const EXPORT_ENV: &str = "VELNOR_EXPORT_SCHEMA2_FIXTURE_DIR";
const FIXTURE_INPUTS: [&str; 4] = [
    ".velnor/config.toml",
    ".velnor/release-manifest.json",
    "Cargo.toml",
    "src/lib.rs",
];

#[test]
fn export_schema2_fixture_tree_for_owner_review() -> TestResult {
    export_if_requested(std::env::var_os(EXPORT_ENV))
}

fn export_if_requested(output: Option<OsString>) -> TestResult {
    let Some(output) = output else {
        return Ok(());
    };

    let workspace = workspace_root()?;
    let repo = make_repo(&workflow_config())?;
    let fixture_root = repo.path().canonicalize()?;
    let tree = render_staged_tree(&prepare(&fixture_root)?)?;
    let output = external_export_destination(Path::new(&output), &[&workspace, &fixture_root])?;
    write_export(&output, &fixture_root, &tree)
}

fn workspace_root() -> Result<PathBuf, Box<dyn std::error::Error>> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

fn external_export_destination(
    output: &Path,
    protected_roots: &[&Path],
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if !output.is_absolute() {
        return Err("fixture export path must be absolute".into());
    }
    let output_name = output
        .file_name()
        .filter(|name| {
            matches!(
                Path::new(name).components().next(),
                Some(Component::Normal(_))
            )
        })
        .ok_or("fixture export path has no normal final component")?;
    let output_parent = output.parent().ok_or("fixture export path has no parent")?;
    reject_symlink_parent_components(output_parent)?;
    let output = output_parent.canonicalize()?.join(output_name);
    ensure_external_to_roots(&output, protected_roots)?;
    match fs::symlink_metadata(&output) {
        Ok(_) => Err(format!("refusing to overwrite fixture export: {}", output.display()).into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(output),
        Err(error) => Err(error.into()),
    }
}

fn reject_symlink_parent_components(parent: &Path) -> TestResult {
    let mut current = PathBuf::new();
    for component in parent.components() {
        match component {
            Component::Prefix(_) | Component::RootDir => current.push(component.as_os_str()),
            Component::Normal(_) => {
                current.push(component.as_os_str());
                let metadata = fs::symlink_metadata(&current)?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(format!(
                        "fixture export parent is not a real directory: {}",
                        current.display()
                    )
                    .into());
                }
            }
            Component::CurDir | Component::ParentDir => {
                return Err("fixture export path parent must be normalized".into());
            }
        }
    }
    Ok(())
}

fn ensure_external_to_roots(destination: &Path, protected_roots: &[&Path]) -> TestResult {
    if let Some(root) = protected_roots
        .iter()
        .find(|root| destination.starts_with(root) || root.starts_with(destination))
    {
        return Err(format!(
            "fixture export destination {} overlaps protected root {}",
            destination.display(),
            root.display()
        )
        .into());
    }
    Ok(())
}

fn write_export(output: &Path, fixture_root: &Path, tree: &RenderedTree) -> TestResult {
    fs::create_dir(output)?;
    let inputs = output.join("fixture-inputs");
    fs::create_dir(&inputs)?;
    for source_path in FIXTURE_INPUTS {
        export_fixture_input(fixture_root, &inputs, source_path)?;
    }

    let rendered = output.join("rendered");
    fs::create_dir(&rendered)?;
    for file in &tree.files {
        export_rendered_file(&rendered, &file.path, file.bytes.as_bytes())?;
    }
    for link in &tree.symlinks {
        export_rendered_symlink(&rendered, &link.path, &link.target)?;
    }
    Ok(())
}

fn export_fixture_input(fixture_root: &Path, output_root: &Path, source_path: &str) -> TestResult {
    let relative = safe_relative_path(source_path)?;
    let bytes = fs::read(fixture_root.join(&relative))?;
    write_new_file(&output_root.join(relative), &bytes)
}

fn export_rendered_file(root: &Path, source_path: &str, bytes: &[u8]) -> TestResult {
    let relative = safe_relative_path(source_path)?;
    write_new_file(&root.join(relative), bytes)
}

fn export_rendered_symlink(root: &Path, source_path: &str, target: &str) -> TestResult {
    let relative = safe_relative_path(source_path)?;
    let destination = root.join(relative);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    create_fixture_symlink(target, &destination)?;
    Ok(())
}

fn write_new_file(destination: &Path, bytes: &[u8]) -> TestResult {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)?;
    file.write_all(bytes)?;
    Ok(())
}

fn safe_relative_path(path: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let relative = Path::new(path);
    if path.is_empty()
        || path.contains('\\')
        || path
            .split('/')
            .any(|component| component.is_empty() || matches!(component, "." | ".."))
        || relative.is_absolute()
        || relative
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("unsafe fixture tree path: {path:?}").into());
    }
    Ok(relative.to_path_buf())
}

#[cfg(unix)]
fn create_fixture_symlink(target: &str, destination: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, destination)
}

#[cfg(windows)]
fn create_fixture_symlink(target: &str, destination: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(target, destination)
}

#[test]
fn fixture_export_preserves_raw_inputs_and_rendered_tree() -> TestResult {
    let workspace = workspace_root()?;
    let repo = make_repo(&workflow_config())?;
    let fixture_root = repo.path().canonicalize()?;
    let tree = render_staged_tree(&prepare(&fixture_root)?)?;
    let temporary_parent = tempfile::tempdir()?;
    let parent = temporary_parent.path().canonicalize()?;
    let output =
        external_export_destination(&parent.join("fixture-export"), &[&workspace, &fixture_root])?;

    write_export(&output, &fixture_root, &tree)?;
    for source_path in FIXTURE_INPUTS {
        assert_eq!(
            fs::read(output.join("fixture-inputs").join(source_path))?,
            fs::read(fixture_root.join(source_path))?,
            "raw fixture input changed: {source_path}"
        );
    }
    assert_exported_tree(&output.join("rendered"), &tree)
}

fn assert_exported_tree(root: &Path, tree: &RenderedTree) -> TestResult {
    for file in &tree.files {
        assert_eq!(
            fs::read(root.join(safe_relative_path(&file.path)?))?.as_slice(),
            file.bytes.as_bytes(),
            "rendered file changed: {}",
            file.path
        );
    }
    for link in &tree.symlinks {
        assert_eq!(
            fs::read_link(root.join(safe_relative_path(&link.path)?))?,
            PathBuf::from(&link.target),
            "rendered symlink target changed: {}",
            link.path
        );
    }
    Ok(())
}

#[cfg(unix)]
#[test]
fn fixture_export_preserves_rendered_symlink_target() -> TestResult {
    let workspace = workspace_root()?;
    let repo = make_repo(&workflow_config())?;
    let fixture_root = repo.path().canonicalize()?;
    let temporary_parent = tempfile::tempdir()?;
    let parent = temporary_parent.path().canonicalize()?;
    let output =
        external_export_destination(&parent.join("fixture-export"), &[&workspace, &fixture_root])?;
    let target = "../workflows/ci.yml";
    let tree = RenderedTree {
        files: Vec::new(),
        symlinks: vec![RenderedSymlink {
            path: ".github/actions/fixture-link".to_owned(),
            target: target.to_owned(),
        }],
    };

    write_export(&output, &fixture_root, &tree)?;
    assert_eq!(
        fs::read_link(output.join("rendered/.github/actions/fixture-link"))?,
        PathBuf::from(target)
    );
    Ok(())
}

#[test]
fn fixture_export_rejects_overlap_existing_paths_and_symlink_parents() -> TestResult {
    let workspace = workspace_root()?;
    let repo = make_repo(&workflow_config())?;
    let fixture_root = repo.path().canonicalize()?;
    let outside = tempfile::tempdir()?;
    let outside_root = outside.path().canonicalize()?;

    assert_rejected(
        &workspace.join("fixture-export"),
        &[&workspace, &fixture_root],
    );
    assert_rejected(
        &fixture_root.join("fixture-export"),
        &[&workspace, &fixture_root],
    );
    assert_rejected(
        &outside_root.join("future-export-root"),
        &[&outside_root.join("future-export-root/source-checkout")],
    );
    let existing = outside_root.join("already-exists");
    fs::create_dir(&existing)?;
    let sentinel = existing.join("keep.txt");
    fs::write(&sentinel, b"preserve existing destination")?;
    assert!(
        external_export_destination(&existing, &[&workspace, &fixture_root]).is_err(),
        "existing destination was accepted"
    );
    assert_eq!(fs::read(&sentinel)?, b"preserve existing destination");
    assert_eq!(fs::read_dir(&existing)?.count(), 1);

    #[cfg(unix)]
    {
        assert_symlink_parents_rejected(&outside_root, &workspace, &fixture_root)?;
        let symlink_leaf = outside_root.join("existing-symlink-leaf");
        std::os::unix::fs::symlink(outside_root.join("missing-target"), &symlink_leaf)?;
        assert!(
            external_export_destination(&symlink_leaf, &[&workspace, &fixture_root]).is_err(),
            "existing symlink destination was accepted"
        );
    }

    let safe = outside_root.join("safe-export");
    assert_eq!(
        external_export_destination(&safe, &[&workspace, &fixture_root])?,
        safe
    );
    assert!(!safe.exists(), "destination validation created its output");
    Ok(())
}

fn assert_rejected(output: &Path, protected_roots: &[&Path]) {
    assert!(
        external_export_destination(output, protected_roots).is_err(),
        "unsafe output was accepted: {}",
        output.display()
    );
    assert!(!output.exists(), "guard created {}", output.display());
}

#[cfg(unix)]
fn assert_symlink_parents_rejected(
    outside_root: &Path,
    workspace: &Path,
    fixture_root: &Path,
) -> TestResult {
    use std::os::unix::fs::symlink;

    let workspace_alias = outside_root.join("workspace-link");
    symlink(workspace, &workspace_alias)?;
    assert_rejected(
        &workspace_alias.join("fixture-export"),
        &[workspace, fixture_root],
    );

    let outside_alias = outside_root.join("outside-link");
    symlink(outside_root, &outside_alias)?;
    assert_rejected(
        &outside_alias.join("fixture-export"),
        &[workspace, fixture_root],
    );
    Ok(())
}
