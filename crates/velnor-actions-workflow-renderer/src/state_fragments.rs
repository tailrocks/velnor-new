// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_actions_contract::workflow::cache_family::{StateCompat, StateSnapshot};

pub(crate) const S256_FN: &str = "s256() { sha256sum \"$1\" > \"$d/sum\" || return 1; h=; rest=; read -r h rest < \"$d/sum\" || [ -n \"$h\" ]; case \"$h\" in ''|*[!0-9a-f]*) printf '%s\\n' 'sha256sum returned no digest' >&2; return 1 ;; esac; [ \"${#h}\" -eq 64 ] || return 1; }";

pub(crate) struct CompatBinding<'a> {
    pub(crate) unit: &'a str,
    pub(crate) mbx: &'a str,
    pub(crate) toolchain: &'a str,
    pub(crate) root: &'a str,
}

fn quote(line: &str) -> String {
    if line.starts_with('$') {
        format!("'{line}'")
    } else {
        format!("\"{line}\"")
    }
}

pub(crate) fn compat_lines(dir: &str, binding: &CompatBinding<'_>) -> Vec<String> {
    let compat = StateCompat {
        unit: binding.unit.to_owned(),
        root: binding.root.to_owned(),
        mbx: binding.mbx.to_owned(),
        toolchain: binding.toolchain.to_owned(),
        image_os: "${ImageOS-}".to_owned(),
        uname_s: "$uname_s".to_owned(),
        uname_m: "$uname_m".to_owned(),
        store: "$RUNNER_TEMP/velnor/mbx".to_owned(),
        checkout: "$GITHUB_WORKSPACE".to_owned(),
    };
    let quoted = compat
        .lines()
        .iter()
        .map(|line| quote(line))
        .collect::<Vec<_>>()
        .join(" ");
    vec![
        format!(
            "uname -s > \"{dir}/uname_s\" || :; uname_s=; read -r uname_s < \"{dir}/uname_s\" || :"
        ),
        format!(
            "uname -m > \"{dir}/uname_m\" || :; uname_m=; read -r uname_m < \"{dir}/uname_m\" || :"
        ),
        format!("printf '%s\\n' {quoted} > \"{dir}/compat\""),
        format!("s256 \"{dir}/compat\""),
    ]
}

pub(crate) fn lock_digest_lines(dir: &str, lock_path: &str, into: &str) -> Vec<String> {
    vec![
        format!("{into}="),
        format!(
            "if [ -f \"{lock_path}\" ] && [ ! -L \"{lock_path}\" ]; then s256 \"{lock_path}\"; {into}=\"s256-$h\"; fi"
        ),
        format!(
            "if [ -z \"${into}\" ]; then printf '%s\\n' absent > \"{dir}/absent\"; s256 \"{dir}/absent\"; {into}=\"s256-$h\"; fi"
        ),
    ]
}

pub(crate) fn absent_digest_lines(dir: &str, into: &str) -> Vec<String> {
    vec![
        format!("printf '%s\\n' absent > \"{dir}/absent\""),
        format!("s256 \"{dir}/absent\"; {into}=\"s256-$h\""),
    ]
}

pub(crate) fn snapshot_lines(
    dir: &str,
    lock: &str,
    rust_inputs: &str,
    task_set: &str,
) -> Vec<String> {
    let [lock_name, rust_name, task_name] = StateSnapshot::LINE_NAMES;
    vec![
        format!(
            "printf '%s\\n' \"{lock_name}={lock}\" \"{rust_name}={rust_inputs}\" \"{task_name}={task_set}\" > \"{dir}/snapshot\""
        ),
        format!("s256 \"{dir}/snapshot\"; snap=\"s256-$h\""),
    ]
}
