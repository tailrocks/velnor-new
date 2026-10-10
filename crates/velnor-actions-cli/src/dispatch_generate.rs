//! Public workflow-generation command handling.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use velnor_actions_orchestrator::{
    GenerateOptions, generate_dispatched, parse_dispatch_mode, prepare, resolve_root,
};

use crate::dispatch::{fail_public, working_dir};

/// Dispatch `generate`: files written and recommendations go to stderr.
pub(crate) fn run_generate(output_dir: Option<PathBuf>, mode: Option<String>) -> ExitCode {
    let Some(cwd) = working_dir() else {
        return ExitCode::from(1);
    };
    let options = GenerateOptions { output_dir };
    let root = match resolve_root(&cwd) {
        Ok(root) => root,
        Err(error) => return fail_public(&error),
    };
    let preparation = match prepare(&root) {
        Ok(preparation) => preparation,
        Err(error) => return fail_public(&error),
    };
    let dispatch = match mode {
        Some(text) => match parse_dispatch_mode(&text) {
            Ok(mode) => Some(mode),
            Err(error) => return fail_public(&error),
        },
        None => None,
    };
    match generate_dispatched(&preparation, &options, dispatch) {
        Ok(report) => {
            if preparation.discovery.consumer_manifest_stand_in {
                eprintln!(
                    "velnor-actions: WARNING: .velnor/release-manifest.json is absent; generated workflows use a debug-only stand-in that MUST NOT ship"
                );
            }
            if let Some(dir) = &options.output_dir {
                eprintln!("Preview: {}", absolute_preview(&cwd, dir).display());
                eprintln!("Repository: {}", root.display());
            }
            for path in &report.files_written {
                eprintln!("{path}");
            }
            for recommendation in &report.recommendations {
                eprintln!("{recommendation}");
            }
            for warning in warning_lines(&report.warnings) {
                eprintln!("{warning}");
            }
            ExitCode::SUCCESS
        }
        Err(error) => fail_public(&error),
    }
}

/// Format post-publication cleanup warnings for the command's stderr report.
fn warning_lines(warnings: &[String]) -> impl Iterator<Item = String> + '_ {
    warnings
        .iter()
        .map(|warning| format!("velnor-actions: WARNING: {warning}"))
}

/// Absolute preview path for the stderr report; canonical when possible.
fn absolute_preview(cwd: &Path, dir: &Path) -> PathBuf {
    let joined = if dir.is_absolute() {
        dir.to_path_buf()
    } else {
        cwd.join(dir)
    };
    joined.canonicalize().unwrap_or(joined)
}

#[cfg(test)]
mod tests {
    use super::warning_lines;

    #[test]
    fn generate_report_warnings_are_prefixed_for_stderr() {
        let warnings = vec!["retired_tree_cleanup_failed: busy".to_owned()];
        assert_eq!(
            warning_lines(&warnings).collect::<Vec<_>>(),
            ["velnor-actions: WARNING: retired_tree_cleanup_failed: busy"]
        );
    }
}
