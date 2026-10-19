//! `Compute cache keys` commands for the dependency seed families.
//!
//! Per workspace root `n` the step publishes `rsdeps_key_<n>` and the
//! trusted restore prefix `rsdeps_restore_keys_<n>`:
//!
//! - compat = `s256` of the family, schema, and paths; `uname -s`/`-m`;
//!   `$ImageOS` (never `ImageVersion`); the absolute workspace and Cargo
//!   home; the root; the four Rust flag variables; and the installed
//!   toolchain's `rustc -vV`.
//! - depset = `s256` of the root `Cargo.lock`, the git blob ids of every
//!   tracked `Cargo.toml` under the root (nested workspace roots excluded)
//!   and of `.cargo/config{,.toml}` under the root and the repository root,
//!   and the seed lane records (task ID, argv, env) of the root's build,
//!   Clippy, and doc tasks from the checked-in task-execution manifest.
//!
//! Keys always carry the `trusted` scope: no pull-request dependency seed
//! exists, so every run restores from the trusted namespace only.

use velnor_actions_contract::workflow::TASK_EXECUTION_MANIFEST_PATH;
use velnor_actions_contract::workflow::cache_family::{
    CacheFamily, RSDEPS_HELPER_KEY_OUTPUT, RSDEPS_HELPER_RESTORE_KEYS_OUTPUT, root_prefix,
    rsdeps_key_output, rsdeps_restore_keys_output, trusted_root_prefix,
};
use velnor_actions_mise::{PinnedTool, ToolCatalog};

use crate::OrchestratorError;

/// Rust flag variables that change compiled dependency units.
const FLAG_VARS: [&str; 4] = [
    "RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
];

/// Inline seed-record digest input (Python is on every hosted runner).
///
/// Prints canonical JSON of `{task_id: {argv, env}}` for the root's build,
/// Clippy, and doc records. Version markers (generator and report-helper
/// versions, execution and task digests) are excluded so a Velnor release
/// alone never invalidates a dependency seed.
const RECORDS_PY: &str = r#"import json, sys
path, root, nested = sys.argv[1], sys.argv[2], sys.argv[3:]
with open(path, encoding="utf-8") as handle:
    handle.readline()
    tasks = json.loads(handle.read())["tasks"]
def owned(manifest):
    if root and not manifest.startswith(root + "/"):
        return False
    return not any(manifest.startswith(other + "/") for other in nested)
selected = {}
for task_id, record in sorted(tasks.items()):
    argv = record["argv"]
    if task_id.split("/")[-2] not in ("build", "clippy", "doc") or "--manifest-path" not in argv:
        continue
    at = argv.index("--manifest-path") + 1
    if at < len(argv) and owned(argv[at]):
        selected[task_id] = {"argv": argv, "env": record["env"]}
print(json.dumps(selected, sort_keys=True, separators=(",", ":")))
"#;

/// Roots of one generation: every seeded root and the optional helper build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SeedKeyInputs {
    /// Seeded workspace roots; the index is the root ordinal `n`.
    pub(crate) roots: Vec<String>,
    /// Every Cargo workspace root, for excluding nested workspaces.
    pub(crate) workspace_roots: Vec<String>,
    /// Bootstrap Plan helper build argv (repository root), when built in Plan.
    pub(crate) helper_build: Option<Vec<String>>,
}

/// Seed key commands, or nothing when no root is seeded.
/// # Errors
///
/// Returns contract errors for unsafe roots or too many roots.
pub(crate) fn seed_commands(
    catalog: &ToolCatalog,
    inputs: &SeedKeyInputs,
) -> Result<Vec<String>, OrchestratorError> {
    if inputs.roots.is_empty() && inputs.helper_build.is_none() {
        return Ok(Vec::new());
    }
    if inputs.roots.len() > velnor_actions_contract::workflow::cache_family::MAX_RSDEPS_ROOTS {
        return Err(contract("too_many_seed_roots"));
    }
    for family in [CacheFamily::Rsdeps, CacheFamily::RsdepsHelper] {
        family
            .validate_key_bound()
            .map_err(|err| contract(&err.to_string()))?;
    }
    let spec = catalog.tool_spec(PinnedTool::Rust);
    let mut script = vec![
        "mkdir -p \"$d/rust-empty\"".to_owned(),
        format!(
            "(cd \"$d/rust-empty\" && mise --no-config --no-env --no-hooks exec {spec} -- rustc -vV) > \"$d/rustc-vv\""
        ),
    ];
    for (ordinal, root) in inputs.roots.iter().enumerate() {
        crate::source_prep::validate_root(root)?;
        let nested = nested_roots(root, &inputs.workspace_roots);
        let records = records_command(root, &nested);
        script.extend(family_commands(
            CacheFamily::Rsdeps,
            root,
            &nested,
            &format!("rsdeps-{ordinal}"),
            &records,
            (
                &rsdeps_key_output(ordinal),
                &rsdeps_restore_keys_output(ordinal),
            ),
        ));
    }
    if let Some(build) = &inputs.helper_build {
        let nested = nested_roots("", &inputs.workspace_roots);
        let quoted = build
            .iter()
            .map(|arg| plain_word(arg))
            .collect::<Result<Vec<_>, _>>()?
            .join(" ");
        script.extend(family_commands(
            CacheFamily::RsdepsHelper,
            "",
            &nested,
            "rsdeps-helper",
            &format!("printf '%s\\n' {quoted}"),
            (RSDEPS_HELPER_KEY_OUTPUT, RSDEPS_HELPER_RESTORE_KEYS_OUTPUT),
        ));
    }
    Ok(script)
}

/// Roots strictly below `root` (their members belong to their own seed).
pub(crate) fn nested_roots(root: &str, roots: &[String]) -> Vec<String> {
    let prefix = root_prefix(root);
    roots
        .iter()
        .filter(|other| other.as_str() != root && other.starts_with(&prefix))
        .cloned()
        .collect()
}

fn family_commands(
    family: CacheFamily,
    root: &str,
    nested: &[String],
    tag: &str,
    records: &str,
    outputs: (&str, &str),
) -> Vec<String> {
    let spec = family.spec();
    let prefix = root_prefix(root);
    let compat = format!("$d/{tag}-compat");
    let depset = format!("$d/{tag}-depset");
    let mut lines = vec![
        format!("family={}", spec.layer),
        format!("schema={}", spec.schema),
    ];
    lines.extend(spec.shell_paths(Some(&prefix)));
    let quoted = lines
        .iter()
        .map(|line| format!("\"{line}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let flags = FLAG_VARS.map(|var| format!("{var}=%s\\n")).concat();
    let flag_values = FLAG_VARS
        .iter()
        .map(|var| format!("\"${{{var}:-}}\""))
        .collect::<Vec<_>>()
        .join(" ");
    let lock = crate::discover::workspace_lock(root);
    let trusted = trusted_root_prefix(family, root);
    vec![
        format!("printf '%s\\n' {quoted} > \"{compat}\""),
        format!("cat \"$d/platform\" >> \"{compat}\""),
        format!(
            "printf '%s\\n' \"image_os=${{ImageOS:-}}\" \"workspace=$GITHUB_WORKSPACE\" \"cargo_home=$RUNNER_TEMP/velnor/cargo\" \"root={root}\" >> \"{compat}\""
        ),
        format!("printf '{flags}' {flag_values} >> \"{compat}\""),
        format!("cat \"$d/rustc-vv\" >> \"{compat}\""),
        format!("s256 \"{compat}\""),
        "rc=\"s256-$h\"".to_owned(),
        format!("s256 \"$GITHUB_WORKSPACE/{lock}\""),
        format!("printf 'lock\\t%s\\n' \"$h\" > \"{depset}\""),
        format!(
            "git -C \"$GITHUB_WORKSPACE\" ls-files -s -- {} >> \"{depset}\"",
            manifest_pathspecs(&prefix, nested)
        ),
        format!("{records} >> \"{depset}\""),
        format!("s256 \"{depset}\""),
        format!("p=\"{trusted}$rc-\""),
        "k=\"${p}s256-$h\"".to_owned(),
        "check_key \"$k\"".to_owned(),
        format!(
            "printf '%s\\n' \"{}=$k\" \"{}=$p\" >> \"$GITHUB_OUTPUT\"",
            outputs.0, outputs.1
        ),
        format!("printf 'velnor: {} key=%s\\n' \"$k\"", spec.layer),
    ]
}

/// Tracked manifests and Cargo configs of one root, nested roots excluded.
fn manifest_pathspecs(prefix: &str, nested: &[String]) -> String {
    let mut specs = vec![
        format!("':(glob){prefix}**/Cargo.toml'"),
        format!("':(glob){prefix}.cargo/config'"),
        format!("':(glob){prefix}.cargo/config.toml'"),
    ];
    if !prefix.is_empty() {
        specs.push("':(glob).cargo/config'".to_owned());
        specs.push("':(glob).cargo/config.toml'".to_owned());
    }
    specs.extend(
        nested
            .iter()
            .map(|other| format!("':(exclude,glob){other}/**'")),
    );
    specs.join(" ")
}

/// `python3 -I -c 'exec("…")' <manifest> <root> <nested…>`.
fn records_command(root: &str, nested: &[String]) -> String {
    let code = RECORDS_PY
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n");
    let mut args = vec![
        format!("\"$GITHUB_WORKSPACE/{TASK_EXECUTION_MANIFEST_PATH}\""),
        format!("\"{root}\""),
    ];
    args.extend(nested.iter().map(|other| format!("\"{other}\"")));
    format!("python3 -I -c 'exec(\"{code}\")' {}", args.join(" "))
}

/// One fixed helper-build argv word, unquoted so `${VELNOR_TOOL_*}` expands.
fn plain_word(arg: &str) -> Result<&str, OrchestratorError> {
    let plain = !arg.is_empty()
        && arg.bytes().all(|b| {
            b.is_ascii_alphanumeric()
                || matches!(
                    b,
                    b'@' | b'.' | b'_' | b'+' | b'/' | b':' | b'=' | b'-' | b'$' | b'{' | b'}'
                )
        });
    if plain {
        Ok(arg)
    } else {
        Err(contract("helper_argv_word"))
    }
}

fn contract(problem: &str) -> OrchestratorError {
    OrchestratorError::Contract {
        problem: problem.to_owned(),
    }
}

#[cfg(test)]
#[path = "cache_keys_seed_tests.rs"]
mod tests;
