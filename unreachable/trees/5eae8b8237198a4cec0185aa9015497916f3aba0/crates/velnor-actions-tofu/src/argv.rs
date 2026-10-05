//! Fixed tofu payload argv per kind (contract S3, spec §6.1).
//!
//! Pure data over `TofuTaskKind` plus the
//! normalized root: the orchestrator wraps this payload in a pinned-tool
//! execution via the Mise adapter; this crate builds no invocations.
//! `-chdir` is global and precedes the subcommand, so subdir roots
//! prefix `-chdir <root>` FIRST. The repo root (`""`) carries no
//! `-chdir`; its identity fields carry `.` instead (S3).
//! Fixed shapes carry no file operands, so no `--` separator is
//! emitted; `-`-leading roots fail closed (H6) and scope filenames
//! never enter argv (recursive form, never shell-joined).

use std::ffi::OsString;

use velnor_actions_contract::ContractError;

use crate::kinds::TofuTaskKind;

/// `path.cwd` finding tag for subdir roots (S3 caveat).
pub const CHDIR_FINDING_TAG: &str = "path.cwd";

/// Fixed tofu payload argv for one kind in one root.
///
/// Fmt is `fmt -check -recursive -no-color`, init is `init
/// -backend=false -input=false -lockfile=readonly -no-color`,
/// validate is `validate -no-color`, each prefixed with `-chdir
/// <root>` for subdir roots. Readonly init never repairs the lock:
/// a missing or stale lock fails with the S4 marker instead of
/// silently selecting new providers.
///
/// # Errors
///
/// Returns [`ContractError`] when the root starts with `-` (the
/// separate `-chdir <value>` form would let tofu reparse it as
/// flags) or carries a `..` segment. Proposals qualify roots before
/// building payloads, so traversal never arrives via `propose_task`;
/// the check fails closed for direct callers.
pub fn tofu_payload_argv(kind: TofuTaskKind, root: &str) -> Result<Vec<OsString>, ContractError> {
    let mut argv: Vec<OsString> = Vec::new();
    if !root.is_empty() {
        reject_leading_dash(root, "root")?;
        if root.split('/').any(|segment| segment == "..") {
            return Err(ContractError::identity("tofu_cli_argv", "traversal_root"));
        }
        argv.push(OsString::from("-chdir"));
        argv.push(OsString::from(root));
    }
    let flag = OsString::from;
    match kind {
        TofuTaskKind::Fmt => {
            argv.extend([flag("fmt"), flag("-check"), flag("-recursive")]);
            argv.push(flag("-no-color"));
        }
        TofuTaskKind::InitForValidate => {
            argv.extend([flag("init"), flag("-backend=false")]);
            argv.extend([flag("-input=false"), flag("-lockfile=readonly")]);
            argv.push(flag("-no-color"));
        }
        TofuTaskKind::Validate => {
            argv.extend([flag("validate"), flag("-no-color")]);
        }
    }
    Ok(argv)
}

/// `path.cwd` caveat for one root: `path.cwd:<root>` when `-chdir`
/// is used, `None` for the repo root (identity carries `.`).
#[must_use]
pub fn chdir_finding_for_root(root: &str) -> Option<String> {
    if root.is_empty() {
        None
    } else {
        Some(format!("{CHDIR_FINDING_TAG}:{root}"))
    }
}

/// Reject one tofu-side value starting with `-` (tofu would reparse it).
fn reject_leading_dash(value: &str, what: &str) -> Result<(), ContractError> {
    if value.starts_with('-') {
        return Err(ContractError::identity(
            "tofu_argv",
            format!("leading_dash_{what}"),
        ));
    }
    Ok(())
}
