//! Conservative attribution of rustc's non-compiling query forms.
//!
//! This is a closed query recognizer, not a compiler argument parser. The
//! cache parser reports `CompilerQuery` before checking the remaining arguments
//! and therefore cannot prove this attribution. Pass original compiler argv:
//! rustc expands `@path` before parsing options, including option values.

use std::ffi::OsString;

pub(crate) fn rustc_probe(arguments: &[OsString]) -> bool {
    if arguments.iter().any(|argument| {
        argument
            .to_str()
            .is_none_or(|value| value.is_empty() || value.starts_with('@'))
    }) {
        return false;
    }
    if !arguments.is_empty() && arguments.iter().all(|argument| version_argument(argument)) {
        return arguments
            .iter()
            .any(|argument| !matches!(argument.to_str(), Some("-v" | "--verbose")));
    }
    print_probe(arguments)
}

fn version_argument(argument: &OsString) -> bool {
    matches!(
        argument.to_str(),
        Some("-V" | "-Vv" | "-vV" | "--version" | "--help" | "-h" | "-v" | "--verbose")
    )
}

fn print_probe(arguments: &[OsString]) -> bool {
    let mut printed = false;
    let mut inputs = 0;
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        let Some(argument) = argument.to_str() else {
            return false;
        };
        let (flag, inline) = argument
            .split_once('=')
            .map_or((argument, None), |(flag, value)| (flag, Some(value)));
        if flag == "--print" {
            let value = inline.or_else(|| remaining.next().and_then(|value| value.to_str()));
            if !matches!(
                value,
                Some(
                    "cfg"
                        | "target-list"
                        | "target-spec-json"
                        | "sysroot"
                        | "target-libdir"
                        | "host-tuple"
                        | "split-debuginfo"
                        | "crate-name"
                        | "file-names"
                        | "deployment-target"
                )
            ) {
                return false;
            }
            printed = true;
        } else if valued_query_option(flag) {
            let value = inline.or_else(|| remaining.next().and_then(|value| value.to_str()));
            if value.is_none_or(str::is_empty) {
                return false;
            }
        } else if ["-L", "-C", "-A", "-W", "-D", "-F"]
            .iter()
            .any(|prefix| argument.starts_with(prefix) && argument.len() > prefix.len())
        {
            // Joined option values are opaque, including query-like substrings.
        } else if argument == "-" || !argument.starts_with('-') && !argument.starts_with('@') {
            inputs += 1;
            if inputs > 1 {
                return false;
            }
        } else {
            return false;
        }
    }
    printed
}

fn valued_query_option(flag: &str) -> bool {
    matches!(
        flag,
        "--crate-name"
            | "--crate-type"
            | "--target"
            | "--sysroot"
            | "--edition"
            | "--cfg"
            | "--check-cfg"
            | "--out-dir"
            | "--cap-lints"
            | "-L"
            | "-C"
            | "-A"
            | "-W"
            | "-D"
            | "-F"
    )
}

#[cfg(test)]
#[path = "probe_classifier_tests.rs"]
mod tests;
