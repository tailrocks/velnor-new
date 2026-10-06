//! Complete fixed local-tap preparation source and typed positional arguments.

use super::audit::{BrewSourceIdentity, TapIdentity};
use crate::{OwnedSupportFile, SupportBundle};
use velnor_actions_contract::ContractError;

/// Every byte in the fixed preparation closure belongs to this domain.
/// # Errors
/// Rejects invalid source/version markers.
pub fn preparation_bundle(has_casks: bool, version: &str) -> Result<SupportBundle, ContractError> {
    let body = format!("#!/bin/sh\n{}\n", preparation_body(has_casks));
    SupportBundle::compiled(vec![OwnedSupportFile::compiled(
        ".github/velnor/homebrew_preparation.sh",
        &body,
        version,
    )?])
}

/// Exact positional arguments for the source-owned preparation program.
#[must_use]
pub fn preparation_arguments(
    source: &BrewSourceIdentity,
    tap: &TapIdentity,
    has_casks: bool,
) -> Vec<String> {
    vec![
        tap.owner().to_owned(),
        tap.name().to_owned(),
        source.sha().to_owned(),
        source.version().to_owned(),
        if has_casks { "casks" } else { "formula-only" }.to_owned(),
    ]
}

/// Recover only a complete fixed closure from reviewed pins and typed arguments.
/// # Errors
/// Rejects foreign pins, malformed aliases or unknown capability selectors.
pub fn preparation_for_arguments(
    source: &BrewSourceIdentity,
    args: &[String],
    version: &str,
) -> Result<SupportBundle, ContractError> {
    if args.len() != 5 {
        return Err(super::audit::failure(
            "homebrew_preparation_arguments_invalid",
        ));
    }
    let tap = TapIdentity::from_repository(&format!("{}/homebrew-{}", args[0], args[1]))?;
    let has_casks = match args[4].as_str() {
        "casks" => true,
        "formula-only" => false,
        _ => {
            return Err(super::audit::failure(
                "homebrew_preparation_arguments_invalid",
            ));
        }
    };
    if preparation_arguments(source, &tap, has_casks) != args {
        return Err(super::audit::failure(
            "homebrew_preparation_arguments_invalid",
        ));
    }
    preparation_bundle(has_casks, version)
}

/// Fixed source bytes; callers bind the complete versioned closure before launch.
#[must_use]
pub fn preparation_body(has_casks: bool) -> String {
    let capability = if has_casks {
        "test -x /usr/bin/plutil || { echo homebrew_cask_plutil_unavailable >&2; exit 1; }; "
    } else {
        ""
    };
    format!(
        "set -euC; umask 077; PATH=/usr/bin:/bin:/usr/sbin:/sbin; export PATH; {capability}\
         test -n \"${{RUNNER_TEMP:-}}\"; \
         test ! -e /etc/homebrew/brew.env; \
         test ! -L \"$RUNNER_TEMP\"; test ! -L \"$RUNNER_TEMP/velnor\"; \
         mkdir -p \"$RUNNER_TEMP/velnor\"; \
         alias=\"$RUNNER_TEMP/velnor/homebrew\"; test ! -e \"$alias\"; test ! -L \"$alias\"; \
         mktemp -d /tmp/vb.XXXXXXXX > \"$RUNNER_TEMP/velnor/homebrew-prefix\"; \
         IFS= read -r prefix < \"$RUNNER_TEMP/velnor/homebrew-prefix\"; \
         case \"$prefix\" in /tmp/vb.*) ;; *) echo homebrew_prefix_invalid >&2; exit 1;; esac; \
         test ! -L \"$prefix\"; test -d \"$prefix\"; \
         ln -s \"$prefix\" \"$alias\"; \
         mkdir \"$prefix/bin\" \"$prefix/Homebrew\" \"$prefix/git-template\" \
         \"$prefix/home\" \"$prefix/cache\" \"$prefix/logs\"; \
         git_pinned() {{ env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin HOME=\"$prefix/home\" \
         GIT_CONFIG_NOSYSTEM=1 GIT_CONFIG_GLOBAL=/dev/null GIT_TEMPLATE_DIR=\"$prefix/git-template\" \
         git -C \"$prefix/Homebrew\" \"$@\"; }}; \
         git_pinned init; \
         git_pinned remote add origin https://github.com/Homebrew/brew.git; \
         git_pinned fetch --depth=1 origin \"refs/tags/$4:refs/tags/$4\"; \
         git_pinned checkout --detach \"refs/tags/$4\"; \
         git_pinned rev-parse HEAD > \"$prefix/source-sha\"; \
         read -r source_sha < \"$prefix/source-sha\"; test \"$source_sha\" = \"$3\"; \
         ln -s ../Homebrew/bin/brew \"$prefix/bin/brew\"; \
         mkdir -p \"$prefix/Homebrew/Library/Taps/$1\"; \
         pwd -P > \"$prefix/checkout\"; IFS= read -r checkout < \"$prefix/checkout\"; \
         ln -s \"$checkout\" \"$prefix/Homebrew/Library/Taps/$1/homebrew-$2\"; \
         env -i PATH=/usr/bin:/bin:/usr/sbin:/sbin PWD=\"$checkout\" HOME=\"$prefix/home\" \
         HOMEBREW_CACHE=\"$prefix/cache\" HOMEBREW_LOGS=\"$prefix/logs\" \
         HOMEBREW_NO_AUTO_UPDATE=1 HOMEBREW_FORCE_VENDOR_RUBY=1 HOMEBREW_NO_ANALYTICS=1 \
         \"$prefix/bin/brew\" trust \"$1/$2\""
    )
}
