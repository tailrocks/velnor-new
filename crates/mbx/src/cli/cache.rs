use super::cargo::{absolute, cargo_roots};
use super::exec::discover_project_root;
use crate::config::Config;
use crate::{store, target, workspace_state};
use bytesize::ByteSize;
use eyre::Result;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(usage::Args)]
pub(super) struct CacheArgs {
    #[usage(subcommand)]
    pub(super) command: CacheCommands,
}

#[derive(usage::Subcommands)]
pub(super) enum CacheCommands {
    /// Export wrapper timings from a session JSONL file as Perfetto-compatible trace JSON.
    Trace(TraceArgs),
    /// Write an empty owner comparison baseline for a cold cache miss.
    ComparisonState(ComparisonStateArgs),
    /// Print the store directory.
    Dir(JsonArgs),
    /// Summarize what the store holds.
    Stats(JsonArgs),
    /// Show cache use attributed to recorded workspaces.
    Projects,
    /// List the largest objects and action-result records.
    Largest(LargestArgs),
    /// Verify local objects and action results.
    Verify,
    /// Export the cache closure of this checkout's last build. The export includes
    /// Cargo scheduler state for recorded workspaces, with compiler outputs referenced
    /// from the content-addressed closure instead of duplicated.
    Export(ExportArgs),
    /// Import a cache export into the local store. A directory export is consumed:
    /// its objects are moved into the store and the directory is removed. If the
    /// export contains Cargo workspace state and the command runs from a matching
    /// checkout with absent or empty target and build directories, restore that state as well.
    /// Non-empty directories are never replaced. Pass the build invocation after `--`
    /// to select roots configured by command-specific Cargo options.
    Import(ImportArgs),
    /// Remove managed targets, learned incremental state, and cache claims for
    /// one workspace or selected workspaces.
    ///
    /// Provide exactly one of `<WORKSPACE>` or `--interactive`.
    Remove(RemoveCacheArgs),
}

#[derive(usage::Args)]
pub(super) struct ComparisonStateArgs {
    /// Owner comparison-state file to write.
    file: PathBuf,
    /// Print a stable machine-readable report.
    #[usage(long)]
    json: bool,
    /// Validate an existing owner baseline instead of writing an empty one.
    #[usage(long)]
    verify: bool,
}

#[derive(usage::Args)]
pub(super) struct TraceArgs {
    /// Session JSONL file under the store's sessions/v1 directory.
    session: PathBuf,
}

#[derive(usage::Args)]
pub(super) struct JsonArgs {
    /// Print a stable machine-readable report.
    #[usage(long)]
    json: bool,
}

#[derive(usage::Args)]
pub(super) struct LargestArgs {
    /// Maximum entries to print.
    #[usage(long, default = "20")]
    limit: usize,
}

#[derive(usage::Args)]
pub(super) struct ExportArgs {
    /// Retain and compare owner state recorded during import. Current task invocations
    /// replace previous predictions; required retained blobs are revalidated.
    /// Requires directory format; snapshots exceeding the owner store budget fail.
    #[usage(long, value_name = "FILE")]
    compare: Option<PathBuf>,
    /// Print a stable machine-readable report.
    #[usage(long)]
    json: bool,
    /// Export every build that set MBX_CACHE_EXPORT_GROUP to this CI group.
    #[usage(long, value_name = "GROUP")]
    group: Option<String>,
    /// Bundle layout: tar for a portable archive, or directory for a transport
    /// that archives a directory itself, such as the GitHub Actions cache.
    #[usage(long, value_name = "FORMAT", default = "tar")]
    format: String,
    /// Tar archive or directory to write.
    archive: PathBuf,
}

#[derive(usage::Args)]
#[usage(dont_delimit_trailing_values = true)]
pub(super) struct ImportArgs {
    /// Record owner comparison state before consuming the bundle.
    #[usage(long, value_name = "FILE")]
    comparison_state: Option<PathBuf>,
    /// Print a stable machine-readable report.
    #[usage(long)]
    json: bool,
    /// Tar archive or directory to import.
    archive: PathBuf,
    /// Cargo invocation options used to resolve target and intermediate build roots.
    #[usage(
        double_dash = "required",
        allow_hyphen_values,
        value_name = "CARGO_ARGS"
    )]
    pub(super) cargo_args: Vec<String>,
}

#[derive(usage::Args)]
#[usage(group("removal_mode", required))]
pub(super) struct RemoveCacheArgs {
    /// Workspace root to forget.
    #[usage(group = "removal_mode")]
    workspace: Option<PathBuf>,
    /// Select recorded workspaces to remove.
    #[usage(long, group = "removal_mode")]
    interactive: bool,
}

pub(super) fn run(config: &Config, command: CacheCommands) -> Result<ExitCode> {
    match command {
        CacheCommands::ComparisonState(args) => {
            if args.verify {
                super::cache_comparison::read(&args.file)?;
            } else {
                super::cache_comparison::empty(&args.file)?;
            }
            if args.json {
                print_json(
                    &serde_json::json!({"version": 1, "valid": true, "empty": !args.verify}),
                )?;
            }
            Ok(ExitCode::SUCCESS)
        }
        CacheCommands::Trace(args) => {
            print_json(&crate::phase_timing::export(&args.session)?)?;
            Ok(ExitCode::SUCCESS)
        }
        CacheCommands::Dir(args) => {
            if args.json {
                print_json(&CacheDirReport {
                    version: 1,
                    store: config.store_dir().display().to_string(),
                })?;
            } else {
                println!("{}", config.store_dir().display());
            }
            Ok(ExitCode::SUCCESS)
        }
        CacheCommands::Stats(args) => cache_stats(config, args.json).map(|()| ExitCode::SUCCESS),
        CacheCommands::Projects => cache_projects(config).map(|()| ExitCode::SUCCESS),
        CacheCommands::Largest(args) => {
            cache_largest(config, args.limit).map(|()| ExitCode::SUCCESS)
        }
        CacheCommands::Verify => cache_verify(config),
        CacheCommands::Export(args) => cache_export(
            config,
            &args.archive,
            args.group.as_deref(),
            args.format.parse()?,
            args.compare.as_deref(),
            args.json,
        )
        .map(|()| ExitCode::SUCCESS),
        CacheCommands::Import(args) => cache_import(
            config,
            &args.archive,
            args.comparison_state.as_deref(),
            args.json,
            &args.cargo_args,
        )
        .map(|()| ExitCode::SUCCESS),
        CacheCommands::Remove(args) => {
            if args.interactive {
                cache_remove_interactive(config)
            } else {
                cache_remove(
                    config,
                    args.workspace.as_deref().expect("workspace is required"),
                )
                .map(|()| ExitCode::SUCCESS)
            }
        }
    }
}

pub(super) fn cache_export(
    config: &Config,
    archive: &Path,
    group: Option<&str>,
    form: store::ExportForm,
    compare: Option<&Path>,
    json: bool,
) -> Result<()> {
    if compare.is_some() && !matches!(form, store::ExportForm::Directory) {
        eyre::bail!("--compare requires --format directory");
    }
    let baseline = compare.map(super::cache_comparison::read).transpose()?;
    let working_dir = std::env::current_dir()?;
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace = cargo_roots(&cargo, &[], None)
        .map(|roots| roots.workspace_root)
        .unwrap_or_else(|| discover_project_root(&working_dir));
    let store_dir = config.store_dir();
    let targets = match group {
        Some(group) => store::group_workspace_roots(&store_dir, group)?,
        None => store::checkout_workspace_roots(&store_dir, &workspace)?
            .into_iter()
            .collect(),
    };
    let additions = match workspace_state::capture(&store_dir, &targets) {
        Ok(additions) => additions,
        Err(error) => {
            if compare.is_some() {
                return Err(error);
            }
            log::warn!("Cargo workspace state was not included in the cache export: {error}");
            store::ExportAdditions::default()
        }
    };
    let additions = workspace_state::retain(
        &store_dir,
        additions,
        baseline
            .as_ref()
            .and_then(|baseline| baseline.cache.attachments.get(workspace_state::ATTACHMENT)),
    )?;
    let snapshot_budget = baseline.as_ref().map(|_| config.gc.max_bytes);
    let mut report = None;
    let mut publish = |root: &Path, state: &store::ComparisonState| {
        if json || baseline.is_some() {
            let result = super::cache_comparison::report(baseline.as_ref(), root, state)?;
            let useful = result.0.as_ref().is_none_or(|delta| delta.useful());
            report = Some(result);
            return Ok(useful);
        }
        Ok(true)
    };
    let outcome = match group {
        Some(group) => store::export_group_as_checked(
            &store_dir,
            group,
            archive,
            additions,
            form,
            store::ExportPolicy {
                max_bytes: snapshot_budget,
                retained: baseline.as_ref().map(|baseline| &baseline.cache),
            },
            &mut publish,
        ),
        None => store::export_checkout_as_checked(
            &store_dir,
            &workspace,
            archive,
            additions,
            form,
            store::ExportPolicy {
                max_bytes: snapshot_budget,
                retained: baseline.as_ref().map(|baseline| &baseline.cache),
            },
            &mut publish,
        ),
    };
    let outcome = match outcome {
        Ok(outcome) => outcome,
        Err(error) => {
            if json && let Some(refusal) = error.downcast_ref::<store::ExportBudgetExceeded>() {
                print_json(&serde_json::json!({"version": 1, "exported": false,
                    "budget_refused": true, "snapshot_budget_bytes": refusal.budget,
                    "logical_closure_bytes": refusal.logical_bytes,
                    "qualification": "verified inventory exceeds owner budget; persistence not verified"}))?;
            }
            return Err(error);
        }
    };
    if let Some((delta, semantic_digest)) = report {
        if json {
            return print_json(&serde_json::json!({
                "version": 1, "budget_refused": false, "snapshot_budget_bytes": snapshot_budget, "exported": outcome.exported, "actions": outcome.actions, "objects": outcome.objects,
                "bytes": outcome.bytes, "useful_delta": delta.as_ref().map_or(outcome.actions > 0, |d| d.useful()),
                "delta": delta,
                "semantic_digest": semantic_digest,
                "workspace_comparison": "relative_path_type_content_mode_symlink_target",
                "workspace_comparison_exclusions": ["effective_build_root/.rustc_info.json"],
                "workspace_transport_scope": "recorded_target_and_build_directories",
                "qualification": "compiled actions, predictions and Cargo unit state; includes Cargo output and intermediate roots; excludes only effective build-root compiler-query cache .rustc_info.json; other scheduler content differences are reported, not proven additional cache hits"
            }));
        }
        if let Some(delta) = delta {
            println!("useful delta: {}", delta.useful());
        }
    }
    if !outcome.exported {
        return Ok(());
    }
    let subject = group.map_or_else(
        || workspace.display().to_string(),
        |group| format!("export group {group:?}"),
    );
    println!(
        "exported {} actions and {} objects ({}) for {subject}",
        outcome.actions,
        outcome.objects,
        ByteSize::b(outcome.bytes).display().iec(),
    );
    Ok(())
}

pub(super) fn cache_import(
    config: &Config,
    archive: &Path,
    comparison_state: Option<&Path>,
    json: bool,
    cargo_args: &[String],
) -> Result<()> {
    let store = config.store_dir();
    let imported = store::import_archive_with_comparison(&store, archive, |root, state| {
        if let Some(path) = comparison_state {
            super::cache_comparison::write(path, root, state)?;
        }
        Ok(())
    })?;
    let mut restore_status = "not_present";
    let restored = if let Some(attachment) = imported.attachments.get(workspace_state::ATTACHMENT) {
        let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let target_env = std::env::var_os(super::cargo::CARGO_TARGET_DIR_ENV);
        if let Some(roots) = cargo_roots(&cargo, cargo_args, target_env.as_deref()) {
            let cargo = store::CargoBuildRoots {
                build_dir: roots.build_dir.unwrap_or_else(|| roots.target_dir.clone()),
                target_dir: roots.target_dir,
            };
            match workspace_state::restore(
                config,
                &store,
                attachment,
                &roots.workspace_root,
                &cargo,
            ) {
                Ok(workspace_state::RestoreOutcome::Restored {
                    files,
                    referenced_bytes,
                }) => {
                    restore_status = "restored";
                    Some((files, referenced_bytes))
                }
                Ok(workspace_state::RestoreOutcome::SkippedAmbiguous) => {
                    restore_status = "skipped_ambiguous";
                    log::warn!(
                        "cache imported without restoring Cargo workspace state: multiple root pairs match; pass the Cargo invocation after `--` to select its roots"
                    );
                    None
                }
                Ok(workspace_state::RestoreOutcome::SkippedIncompatible) => {
                    restore_status = "skipped_incompatible";
                    None
                }
                Ok(workspace_state::RestoreOutcome::SkippedUnavailable) => {
                    restore_status = "skipped_unavailable";
                    None
                }
                Ok(workspace_state::RestoreOutcome::SkippedNonempty) => {
                    restore_status = "skipped_nonempty";
                    None
                }
                Err(error) => {
                    restore_status = "failed";
                    log::warn!("cache imported without restoring Cargo workspace state: {error}");
                    None
                }
            }
        } else {
            restore_status = "metadata_unavailable";
            log::debug!("no Cargo workspace was available for workspace-state restore");
            None
        }
    } else {
        None
    };
    let outcome = imported.transfer;
    if json {
        return print_json(
            &serde_json::json!({ "version": 1, "actions": outcome.actions,
            "objects": outcome.objects, "bytes": outcome.bytes,
            "comparison_state_recorded": comparison_state.is_some(),
            "workspace_restored": restored.is_some(), "workspace_restore": restore_status }),
        );
    }
    println!(
        "imported {} actions and {} objects from {} ({})",
        outcome.actions,
        outcome.objects,
        archive.display(),
        ByteSize::b(outcome.bytes).display().iec()
    );
    if let Some((files, referenced_bytes)) = restored {
        println!(
            "restored Cargo workspace state ({} referenced files, {})",
            files,
            ByteSize::b(referenced_bytes).display().iec()
        );
    }
    Ok(())
}

pub(super) fn cache_stats(config: &Config, json: bool) -> Result<()> {
    let store = config.store_dir();
    let stats = store::stats(&store)?;
    let views = target::stats(&config.target.root)?;
    let incremental = crate::incremental::stats(&config.cache_dir.join("incremental"))?;
    let generated_root = config.cache_dir.join(crate::out_dir::ROOT);
    let generated = crate::out_dir::stats(&generated_root).ok_or_else(|| {
        eyre::eyre!(
            "generated source trees under {} could not be listed",
            generated_root.display()
        )
    })?;
    let combined_total_bytes = stats
        .total_bytes()
        .saturating_add(views.bytes)
        .saturating_add(incremental.bytes)
        .saturating_add(generated.remaining_bytes);
    if json {
        return print_json(&CacheStatsReport {
            version: 1,
            byte_accounting: "logical",
            store: store.display().to_string(),
            objects: stats.objects,
            object_bytes: stats.object_bytes,
            action_results: stats.action_results,
            action_result_bytes: stats.action_result_bytes,
            total_bytes: stats.total_bytes(),
            live_checkouts: stats.live_checkouts,
            stale_checkouts: stats.stale_checkouts,
            target_directories: views.views,
            target_bytes: views.bytes,
            incremental_directories: incremental.directories,
            incremental_bytes: incremental.bytes,
            incremental_live_checkouts: incremental.live_checkouts,
            incremental_stale_checkouts: incremental.stale_checkouts,
            incremental_untracked_directories: incremental.untracked_directories,
            generated_directories: generated.remaining_directories,
            generated_bytes: generated.remaining_bytes,
            combined_total_bytes,
        });
    }
    println!("store: {}", store.display());
    println!(
        "objects: {} ({})",
        stats.objects,
        ByteSize::b(stats.object_bytes).display().iec()
    );
    println!(
        "action results: {} ({})",
        stats.action_results,
        ByteSize::b(stats.action_result_bytes).display().iec()
    );
    println!(
        "action store total: {}",
        ByteSize::b(stats.total_bytes()).display().iec()
    );
    println!(
        "checkouts: {} live, {} stale",
        stats.live_checkouts, stats.stale_checkouts
    );
    println!(
        "target directories: {} ({})",
        views.views,
        ByteSize::b(views.bytes).display().iec()
    );
    println!(
        "learned incremental: {} directories ({}, {} live, {} stale, and {} untracked)",
        incremental.directories,
        ByteSize::b(incremental.bytes).display().iec(),
        incremental.live_checkouts,
        incremental.stale_checkouts,
        incremental.untracked_directories,
    );
    println!(
        "generated source trees: {} ({})",
        generated.remaining_directories,
        ByteSize::b(generated.remaining_bytes).display().iec()
    );
    println!(
        "combined logical total: {}",
        ByteSize::b(combined_total_bytes).display().iec()
    );
    Ok(())
}

#[derive(serde::Serialize)]
pub(super) struct CacheDirReport {
    version: u8,
    store: String,
}

#[derive(serde::Serialize)]
pub(super) struct CacheStatsReport {
    version: u8,
    byte_accounting: &'static str,
    store: String,
    objects: u64,
    object_bytes: u64,
    action_results: u64,
    action_result_bytes: u64,
    total_bytes: u64,
    live_checkouts: u64,
    stale_checkouts: u64,
    target_directories: u64,
    target_bytes: u64,
    incremental_directories: u64,
    incremental_bytes: u64,
    incremental_live_checkouts: u64,
    incremental_stale_checkouts: u64,
    incremental_untracked_directories: u64,
    /// Content-addressed copies of build-script output that compilations read.
    generated_directories: u64,
    generated_bytes: u64,
    combined_total_bytes: u64,
}

#[derive(serde::Serialize)]
pub(super) struct GcReport {
    pub(super) version: u8,
    /// Byte counts sum file lengths, not physical blocks released.
    pub(super) byte_accounting: &'static str,
    pub(super) max_bytes: u64,
    pub(super) max_total_bytes: Option<u64>,
    pub(super) target_max_bytes: Option<u64>,
    pub(super) incremental_max_bytes: Option<u64>,
    pub(super) dry_run: bool,
    pub(super) action_store: GcActionStoreReport,
    pub(super) targets: GcTargetReport,
    pub(super) incremental: GcIncrementalReport,
    pub(super) generated: GcGeneratedReport,
}

/// Content-addressed copies of build-script output that compilations read.
#[derive(serde::Serialize)]
pub(super) struct GcGeneratedReport {
    pub(super) removed_directories: u64,
    pub(super) removed_bytes: u64,
    pub(super) remaining_directories: u64,
    pub(super) remaining_bytes: u64,
}

#[derive(serde::Serialize)]
pub(super) struct GcActionStoreReport {
    pub(super) removed_objects: u64,
    pub(super) removed_action_results: u64,
    pub(super) removed_checkout_records: u64,
    pub(super) removed_session_streams: u64,
    pub(super) removed_bytes: u64,
    pub(super) remaining_bytes: u64,
}

#[derive(serde::Serialize)]
pub(super) struct GcTargetReport {
    pub(super) removed_directories: u64,
    pub(super) removed_bytes: u64,
    /// Unused build units removed from target directories that were kept.
    pub(super) removed_units: u64,
    pub(super) removed_unit_bytes: u64,
    pub(super) remaining_directories: u64,
    pub(super) remaining_bytes: u64,
    /// Selected for removal but kept, because a running command was using
    /// them or a build claimed them after the selection.
    pub(super) kept_active_directories: u64,
}

#[derive(serde::Serialize)]
pub(super) struct GcIncrementalReport {
    pub(super) removed_directories: u64,
    pub(super) removed_bytes: u64,
    pub(super) remaining_directories: u64,
    pub(super) remaining_bytes: u64,
    pub(super) skipped_active_directories: u64,
    pub(super) untracked_directories: u64,
}

pub(super) fn print_json(value: &impl serde::Serialize) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub(super) fn cache_projects(config: &Config) -> Result<()> {
    let projects = store::projects(&config.store_dir())?;
    if projects.is_empty() {
        println!("no recorded workspaces");
        return Ok(());
    }
    for project in projects {
        let state = if project.live { "live" } else { "stale" };
        println!(
            "{}\t{} action cache\t{} managed targets\t{} identities\t{state}",
            project.workspace_root.display(),
            ByteSize::b(project.action_bytes).display().iec(),
            ByteSize::b(project.target_bytes).display().iec(),
            project.identities,
        );
    }
    Ok(())
}

pub(super) fn cache_largest(config: &Config, limit: usize) -> Result<()> {
    for entry in store::largest(&config.store_dir(), limit)? {
        let path = entry
            .path
            .strip_prefix(config.store_dir())
            .unwrap_or(&entry.path);
        println!(
            "{}\t{}\t{}",
            ByteSize::b(entry.bytes).display().iec(),
            entry.kind,
            path.display()
        );
    }
    Ok(())
}

pub(super) fn cache_verify(config: &Config) -> Result<ExitCode> {
    let outcome = store::verify(&config.store_dir())?;
    println!(
        "verified {} objects and {} action results",
        outcome.checked_objects, outcome.checked_action_results
    );
    for path in &outcome.problems {
        println!("invalid: {}", path.display());
    }
    Ok(if outcome.problems.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

pub(super) fn cache_remove(config: &Config, workspace: &Path) -> Result<()> {
    let working_dir = std::env::current_dir()?;
    let requested = absolute(&working_dir, &workspace.to_string_lossy());
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let workspace = cache_workspace_root(&cargo, &requested);
    let incremental_bytes = match crate::incremental::remove_workspace(
        &config.cache_dir.join("incremental"),
        &workspace,
    )? {
        crate::incremental::RemoveOutcome::Removed(bytes) => Some(bytes),
        crate::incremental::RemoveOutcome::Missing => None,
        crate::incremental::RemoveOutcome::Active => {
            log::warn!(
                "{} is being built, so its learned incremental state was kept",
                workspace.display()
            );
            None
        }
    };
    let target_bytes = match target::remove_workspace(&config.target.root, &workspace)? {
        target::RemoveOutcome::Removed(bytes) => Some(bytes),
        target::RemoveOutcome::Missing => None,
        target::RemoveOutcome::Active => {
            log::warn!(
                "{} is in use by a running command, so its managed target was kept",
                workspace.display()
            );
            None
        }
    };
    let removed = store::remove_project(&config.store_dir(), &workspace)?;
    println!(
        "removed {} checkout records for {}",
        removed.removed_checkout_records,
        workspace.display()
    );
    let requested_bytes = target_bytes
        .unwrap_or_default()
        .saturating_add(incremental_bytes.unwrap_or_default());
    if requested_bytes > 0 {
        crate::savings::record_quietly(
            &config.store_dir(),
            &crate::savings::Delta {
                freed_requested_bytes: requested_bytes,
                ..crate::savings::Delta::default()
            },
        );
    }
    if let Some(bytes) = target_bytes {
        println!(
            "removed managed target directory ({} logical)",
            ByteSize::b(bytes).display().iec()
        );
    }
    if let Some(bytes) = incremental_bytes {
        println!(
            "removed learned incremental state ({} logical)",
            ByteSize::b(bytes).display().iec()
        );
    }
    println!("shared cache objects remain available to other workspaces and normal GC");
    Ok(())
}

fn cache_remove_interactive(config: &Config) -> Result<ExitCode> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        eyre::bail!(
            "--interactive requires a terminal; use `mbx cache remove <WORKSPACE>` instead"
        );
    }
    eprintln!("Loading recorded workspaces...");
    let projects = store::projects(&config.store_dir())?;
    cache_remove_interactive_with(
        &projects,
        select_projects,
        confirm_project_removal,
        |workspace| cache_remove(config, workspace),
    )
}

pub(super) fn cache_remove_interactive_with(
    projects: &[store::ProjectUsage],
    select: impl FnOnce(&[store::ProjectUsage]) -> Result<Vec<PathBuf>>,
    confirm: impl FnOnce(&[PathBuf]) -> Result<bool>,
    remove: impl FnMut(&Path) -> Result<()>,
) -> Result<ExitCode> {
    if projects.is_empty() {
        println!("no recorded workspaces");
        return Ok(ExitCode::SUCCESS);
    }
    let selected = select(projects)?;
    if selected.is_empty() || !confirm(&selected)? {
        return Ok(ExitCode::SUCCESS);
    }
    Ok(cache_remove_selected_with(&selected, remove))
}

fn select_projects(projects: &[store::ProjectUsage]) -> Result<Vec<PathBuf>> {
    use demand::{DemandOption, MultiSelect};

    let options = projects
        .iter()
        .map(|project| {
            let state = if project.live { "live" } else { "stale" };
            let description = format!(
                "{state} | {} build outputs | {} reusable cache (shared)",
                ByteSize::b(project.target_bytes).display().iec(),
                ByteSize::b(project.action_bytes).display().iec(),
            );
            DemandOption::with_label(
                project.workspace_root.display().to_string(),
                project.workspace_root.clone(),
            )
            .description(&description)
        })
        .collect();
    match MultiSelect::new("Select workspaces to remove")
        .description("Space selects; Enter continues. Nothing is selected by default.")
        .filterable(true)
        .options(options)
        .run()
    {
        Ok(selected) => Ok(selected),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Ok(Vec::new()),
        Err(error) => Err(error.into()),
    }
}

fn confirm_project_removal(selected: &[PathBuf]) -> Result<bool> {
    println!("selected workspaces:");
    for workspace in selected {
        println!("  {}", workspace.display());
    }
    let description = "Removal deletes managed build targets and learned incremental state, and forgets the selected workspaces' cache claims. Source files remain untouched. Shared cache objects remain available to other workspaces and normal garbage collection. Displayed sizes are logical and may not equal physical disk space reclaimed.";
    match demand::Confirm::new("Remove the selected workspaces?")
        .description(description)
        .affirmative("Remove")
        .negative("Cancel")
        .selected(false)
        .run()
    {
        Ok(answer) => Ok(answer),
        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => Ok(false),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn cache_remove_selected_with(
    selected: &[PathBuf],
    mut remove: impl FnMut(&Path) -> Result<()>,
) -> ExitCode {
    let mut failed = false;
    for workspace in selected {
        if let Err(error) = remove(workspace) {
            eprintln!("failed to remove {}: {error}", workspace.display());
            failed = true;
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

/// Resolve the workspace exactly as Cargo does when recording cache ownership.
///
/// Filesystem canonicalization is not interchangeable with Cargo's reported
/// root: on Windows it can introduce a `\\?\` prefix, and symlink spellings can
/// differ too. Both checkout records and managed-target keys use the metadata
/// spelling, so removal must obtain that same identity or it can silently miss
/// the workspace it was asked to forget.
pub(super) fn cache_workspace_root(cargo: &std::ffi::OsStr, requested: &Path) -> PathBuf {
    let arguments = vec![
        "--manifest-path".to_string(),
        requested.join("Cargo.toml").to_string_lossy().into_owned(),
    ];
    cargo_roots(cargo, &arguments, None)
        .map(|roots| roots.workspace_root)
        .unwrap_or_else(|| requested.to_path_buf())
}
