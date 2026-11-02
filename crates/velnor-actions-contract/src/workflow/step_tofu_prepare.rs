// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::step::StepKind;

pub const TOFU_PROVIDER_CACHE_PATH_ENV: &str = "TOFU_PROVIDER_CACHE_PATH";

pub const TOFU_PROVIDERS_PREPARE_SCRIPT: &str = concat!(
    "set -eu; ",
    "cd -P \"$RUNNER_TEMP\"; ",
    "v=\"$PWD/velnor\"; ",
    "p=\"$v/tofu-cache\"; ",
    "[ ! -L \"$v\" ] && [ ! -L \"$p\" ] || exit 1; ",
    "mkdir -p \"$p\"; ",
    "cd -P \"$p\"; ",
    "[ \"$PWD\" = \"$p\" ] || exit 1; ",
    "case \"$TOFU_PROVIDER_CACHE_PATH\" in \"$p/\"*) slug=${TOFU_PROVIDER_CACHE_PATH#\"$p/\"} ;; *) exit 1 ;; esac; ",
    "case \"$slug\" in ''|*/*|*[!A-Za-z0-9_-]*) exit 1 ;; esac; ",
    "d=\"$p/$slug\"; ",
    "[ \"$TOFU_PROVIDER_CACHE_PATH\" = \"$d\" ] || exit 1; ",
    "rm -rf -- \"$d\"; ",
    "mkdir -m 700 -- \"$d\"",
);

const CREDENTIAL_PRELUDE_PREFIX: &str = "unset ";

const CREDENTIAL_PRELUDE_SUFFIX: &str = "; ";

pub(super) fn valid_provider_prepare(kind: &StepKind, path: &str) -> bool {
    let StepKind::Shell { run, env } = kind else {
        return false;
    };
    let [shell, flag, script] = run.as_slice() else {
        return false;
    };
    let Some(prelude) = script.strip_suffix(TOFU_PROVIDERS_PREPARE_SCRIPT) else {
        return false;
    };
    shell == "sh"
        && flag == "-c"
        && valid_credential_prelude(prelude)
        && env.get(TOFU_PROVIDER_CACHE_PATH_ENV).map(String::as_str) == Some(path)
        && env
            .iter()
            .all(|(name, value)| name == TOFU_PROVIDER_CACHE_PATH_ENV || value.is_empty())
}

fn valid_credential_prelude(prelude: &str) -> bool {
    prelude
        .strip_prefix(CREDENTIAL_PRELUDE_PREFIX)
        .and_then(|rest| rest.strip_suffix(CREDENTIAL_PRELUDE_SUFFIX))
        .is_some_and(|vars| {
            vars.bytes()
                .all(|b| b.is_ascii_uppercase() || b == b'_' || b == b' ')
        })
}
