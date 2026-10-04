use std::io;
use std::path::{Path, PathBuf};

use super::super::index::repository::RepositoryContext;
use super::super::index::repository_format::IndexObjectFormat;
use super::super::index::repository_format::config::{self, ConfigSnapshot};
use super::super::private_root::{OwnedFile, PrivateGitRoot};
use super::support::{self, Prepared, SourceBindings};
use crate::MiseError;

const UNBORN: &[u8] = b"ref: refs/heads/velnor-unborn\n";
const INFO_NAMES: [&str; 2] = ["attributes", "exclude"];

pub(super) struct SourceGitInputs<'a> {
    pub(super) source_cwd: &'a Path,
    pub(super) context: &'a RepositoryContext,
    pub(super) common: Option<&'a Path>,
    pub(super) has_index: bool,
    pub(super) expected: Option<IndexObjectFormat>,
}

pub(super) fn prepare_source(
    binding: config::DirectoryBinding,
    private_root: &PrivateGitRoot,
    source: SourceGitInputs<'_>,
    bounds: &super::super::Bounds<'_>,
) -> Result<Prepared, MiseError> {
    source
        .context
        .verify_binding()
        .map_err(|error| support::spawn_failed(&error))?;
    let (cwd, cwd_binding) = config::canonical_directory(source.source_cwd, "source_cwd")
        .map_err(|error| support::spawn_failed(&error))?;
    if cwd != source.context.cwd() {
        return Err(support::invalid("source_cwd_mismatch"));
    }
    let (common, common_binding) = config::canonical_directory(
        source
            .common
            .ok_or_else(|| support::invalid("missing_common_gitdir"))?,
        "common_gitdir",
    )
    .map_err(|error| support::spawn_failed(&error))?;
    let (objects, objects_binding) =
        config::canonical_directory(&common.join("objects"), "source_objects")
            .map_err(|error| support::spawn_failed(&error))?;
    let worktree = source.context.worktree().map(Path::to_path_buf);
    let source_gitdir = source.context.gitdir().to_path_buf();
    let mut bindings = SourceBindings {
        dirs: vec![
            (
                source_gitdir.clone(),
                config::canonical_directory(&source_gitdir, "source_gitdir")
                    .map_err(|error| support::spawn_failed(&error))?
                    .1,
                "source_gitdir",
            ),
            (cwd.clone(), cwd_binding, "source_cwd"),
            (common.clone(), common_binding, "common_gitdir"),
            (objects.clone(), objects_binding, "source_objects"),
        ],
        files: Vec::new(),
        forbidden: forbidden(&source_gitdir, &common, &objects),
        pack_dirs: vec![objects.join("pack")],
    };
    if let Some(worktree) = &worktree {
        bindings.dirs.push((
            worktree.clone(),
            config::canonical_directory(worktree, "source_worktree")
                .map_err(|error| support::spawn_failed(&error))?
                .1,
            "source_worktree",
        ));
    }
    bindings
        .verify()
        .map_err(|error| support::spawn_failed(&error))?;
    let head_file = required(&source_gitdir.join("HEAD"), "source_head")
        .map_err(|error| support::spawn_failed(&error))?;
    let head = query_head(source.context, source.expected, &head_file, bounds)?;
    bindings.files.push(head_file);
    copy_info(private_root, &source_gitdir, &mut bindings)
        .map_err(|error| support::spawn_failed(&error))?;
    copy_info(private_root, &common, &mut bindings)
        .map_err(|error| support::spawn_failed(&error))?;
    let index = if source.has_index {
        Some(copy_index(private_root).map_err(|error| support::spawn_failed(&error))?)
    } else {
        None
    };
    private_root
        .write_file(OwnedFile::Head, head.as_deref().unwrap_or(UNBORN))
        .map_err(|error| support::spawn_failed(&error))?;
    Ok(Prepared {
        binding,
        cwd: Some(cwd),
        worktree,
        objects: Some(objects),
        index,
        bindings,
    })
}

fn query_head(
    context: &RepositoryContext,
    expected: Option<IndexObjectFormat>,
    source_head: &ConfigSnapshot,
    bounds: &super::super::Bounds<'_>,
) -> Result<Option<Vec<u8>>, MiseError> {
    let query = bounds
        .native
        .command(vec!["rev-parse".into(), "--verify".into(), "HEAD".into()]);
    let mut command = query.command()?;
    context
        .apply(&mut command)
        .map_err(|error| support::spawn_failed(&error))?;
    command.env("GIT_NO_LAZY_FETCH", "1");
    let output = super::super::super::process::run(
        &query,
        command,
        bounds.cap,
        bounds.remaining()?,
        bounds.cancel,
    )
    .result?;
    source_head
        .verify()
        .map_err(|error| support::spawn_failed(&error))?;
    context
        .verify_binding()
        .map_err(|error| support::spawn_failed(&error))?;
    if !output.success {
        if output.code == Some(128) && output.stdout.is_empty() {
            return Ok(None);
        }
        return Err(support::invalid("head_query_failed"));
    }
    let width = expected
        .map(format_width)
        .or(match output.stdout.len() {
            41 => Some(40),
            65 => Some(64),
            _ => None,
        })
        .ok_or_else(|| support::invalid("head_oid_invalid"))?;
    if output.stdout.len() != width + 1
        || output.stdout.last() != Some(&b'\n')
        || !output.stdout[..width]
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
    {
        return Err(support::invalid("head_oid_invalid"));
    }
    Ok(Some(output.stdout))
}

fn format_width(format: IndexObjectFormat) -> usize {
    match format {
        IndexObjectFormat::Sha1 => 40,
        IndexObjectFormat::Sha256 => 64,
    }
}

fn copy_index(private_root: &PrivateGitRoot) -> io::Result<PathBuf> {
    let file = required(private_root.slot_path(OwnedFile::Index), "private_index")?;
    private_root.write_or_match(
        OwnedFile::Index,
        file.bytes
            .as_deref()
            .ok_or_else(|| support::io_invalid("missing_index"))?,
    )?;
    Ok(private_root.slot_path(OwnedFile::Index).to_path_buf())
}

fn copy_info(
    private_root: &PrivateGitRoot,
    gitdir: &Path,
    bindings: &mut SourceBindings,
) -> io::Result<()> {
    let info = gitdir.join("info");
    match std::fs::symlink_metadata(&info) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(support::path_error(&error, "info", &info)),
        Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
            return Err(support::io_invalid("info_not_directory"));
        }
        Ok(_) => {}
    }
    for entry in
        std::fs::read_dir(&info).map_err(|error| support::path_error(&error, "info", &info))?
    {
        let entry = entry.map_err(|error| support::path_error(&error, "info_entry", &info))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Err(support::io_invalid("info_name_invalid"));
        };
        if !INFO_NAMES.contains(&name) {
            return Err(support::io_invalid("unsupported_info_metadata"));
        }
        let file = required(&entry.path(), "info_file")?;
        private_root.write_or_match(
            info_slot(name)?,
            file.bytes
                .as_deref()
                .ok_or_else(|| support::io_invalid("missing_info"))?,
        )?;
        bindings.files.push(file);
    }
    Ok(())
}

fn info_slot(name: &str) -> io::Result<OwnedFile> {
    match name {
        "attributes" => Ok(OwnedFile::InfoAttributes),
        "exclude" => Ok(OwnedFile::InfoExclude),
        _ => Err(support::io_invalid("unsupported_info_metadata")),
    }
}

fn forbidden(context: &Path, common: &Path, objects: &Path) -> Vec<PathBuf> {
    vec![
        common.join("shallow"),
        common.join("info/grafts"),
        common.join("refs/replace"),
        common.join("reftable"),
        common.join("refs/reftable"),
        common.join("modules"),
        objects.join("info/alternates"),
        objects.join("info/http-alternates"),
        objects.join("info/promisor"),
        context.join("shallow"),
        context.join("info/grafts"),
        context.join("refs/replace"),
        context.join("reftable"),
        context.join("refs/reftable"),
        context.join("modules"),
    ]
}

fn required(path: &Path, label: &'static str) -> io::Result<ConfigSnapshot> {
    let file = config::snapshot(path.to_path_buf(), support::MAX_BYTES, label)?;
    if file.bytes.is_some() {
        Ok(file)
    } else {
        Err(support::io_invalid("source_file_missing"))
    }
}
