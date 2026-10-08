//! Config-selected verification vectors (`[workflow.verify]`).
//!
//! Fixed `mise exec` vectors plus two fixed `node -e` content checks.
//! Every vector is pinned and general: no repository-specific paths,
//! IDs, or hosts. Every `mise exec` pairs with an explicit `mise
//! install` preparation (the renderer install/exec closure gate); the
//! native-validator `sh -c` probe assembles in [`crate::source_prep`]
//! (the closed `sh` confinement set) and needs no install. Callers
//! pass kinds only.

use velnor_actions_contract::ValidatorKind;
use velnor_actions_mise::{IsolatedCommand, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::OrchestratorError;
use crate::vectors::{CARGO_DENY_VERSION, CARGO_MACHETE_TOOL_SPEC, CARGO_MACHETE_VERSION};

/// Qualified markdownlint-cli2 release (npm, latest).
/// Source: `npm view markdownlint-cli2 versions`; checked 2026-10-07.
/// The isolated `mise exec npm:markdownlint-cli2@0.23.3 --
/// markdownlint-cli2 --help` probe reported v0.23.3.
pub(crate) const MARKDOWNLINT_VERSION: &str = "0.23.3";

/// Qualified Node LTS release for the fixed content checks.
/// Source: `mise ls-remote node`; checked 2026-10-07.
/// The isolated `mise exec node@24.21.0 -- node --version` probe
/// reported v24.21.0.
pub(crate) const VERIFY_NODE_VERSION: &str = "24.21.0";

/// Qualified lychee release (GitHub, latest).
/// Source: `mise ls-remote ubi:lycheeverse/lychee`; checked 2026-10-07.
/// No macOS/aarch64 asset exists, so the probe ran resolution only;
/// the release carries `x86_64-unknown-linux-gnu` for CI runners.
pub(crate) const LYCHEE_VERSION: &str = "0.15.1";

/// Resolve an emitted validator install spec to its pinned name and version.
///
/// Supported validator installation pins. The version must equal the pinned
/// const or the emitted shape drifted and the audit fails closed.
#[must_use]
pub(crate) fn validator_install_pin(
    spec: &str,
) -> Option<(&'static str, &'static str, &'static str)> {
    let (key, version) = spec.split_once('@')?;
    match key {
        "cargo-deny" if version == CARGO_DENY_VERSION => {
            Some(("cargo-deny", CARGO_DENY_VERSION, "cargo-deny"))
        }
        CARGO_MACHETE_TOOL_SPEC if version == CARGO_MACHETE_VERSION => Some((
            "cargo-machete",
            CARGO_MACHETE_VERSION,
            CARGO_MACHETE_TOOL_SPEC,
        )),
        "zizmor" if version == velnor_actions_mise::catalog::ZIZMOR_VERSION => Some((
            "zizmor",
            velnor_actions_mise::catalog::ZIZMOR_VERSION,
            "zizmor",
        )),
        "npm:markdownlint-cli2" if version == MARKDOWNLINT_VERSION => Some((
            "markdownlint-cli2",
            MARKDOWNLINT_VERSION,
            "npm:markdownlint-cli2",
        )),
        "node" if version == VERIFY_NODE_VERSION => Some(("node", VERIFY_NODE_VERSION, "node")),
        "ubi:lycheeverse/lychee" if version == LYCHEE_VERSION => {
            Some(("lychee", LYCHEE_VERSION, "ubi:lycheeverse/lychee"))
        }
        _ => None,
    }
}

/// Mise tool spec for the markdownlint vector.
const MARKDOWNLINT_SPEC: &str = "npm:markdownlint-cli2";
/// Mise tool spec for the fixed content checks.
const VERIFY_NODE_SPEC: &str = "node";
/// Mise tool spec for the link-check vector.
const LYCHEE_SPEC: &str = "ubi:lycheeverse/lychee";

/// Fixed strict-JSON check: every `*.json` outside the skipped trees
/// must parse (rejecting comments, trailing commas, syntax errors)
/// and must not repeat an object key (compared unescaped).
///
/// Single-line `node -e` payload: no newlines, backticks, `$(`, or
/// bare `$` expansions, so it passes the renderer argv gates.
const STRICT_JSON_SCRIPT: &str = r#"const fs = require("fs"), path = require("path"); let fail = 0; const skip = new Set([".git", "node_modules", "target"]); function walk(d) { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) { if (!skip.has(e.name)) walk(p); } else if (e.name.endsWith(".json")) check(p); } } function check(f) { const s = fs.readFileSync(f, "utf8"); try { JSON.parse(s); } catch (e) { console.log("invalid-json:" + f + ":" + e.message); fail = 1; return; } const d = dups(s); if (d !== null) { console.log("duplicate-key:" + f + ":" + d); fail = 1; } } function dups(s) { const st = [new Set()]; let i = 0; while (i < s.length) { const c = s[i]; if (c === "\"") { let j = i + 1; while (j < s.length) { if (s[j] === "\\") { j += 2; continue; } if (s[j] === "\"") break; j++; } let k = j + 1; while (k < s.length && /\s/.test(s[k])) k++; if (s[k] === ":") { const t = st[st.length - 1]; const v = JSON.parse(s.slice(i, j + 1)); if (t.has(v)) return v; t.add(v); } i = j + 1; continue; } if (c === "{") st.push(new Set()); else if (c === "}") st.pop(); i++; } return null; } walk("."); process.exitCode = fail;"#;

/// Fixed frontmatter/ID check over `skills/*/SKILL.md` (Agent Skills
/// spec): `name` is required, 1-64 chars, kebab-case, equal to the
/// parent directory, and unique repo-wide; `description` is required,
/// non-empty, and at most 1024 chars.
///
/// A fixed script because no suitable pinned tool exists: the npm
/// `skills-ref` publisher is unofficial and the official `PyPI`
/// `skills-ref` (alpha) needs the `pipx:` backend, which failed to
/// bootstrap in probing. Same single-line argv discipline as above.
const FRONTMATTER_ID_SCRIPT: &str = r##"const fs = require("fs"), path = require("path"); let fail = 0; const seen = new Map(); const root = "skills"; if (fs.existsSync(root)) { for (const e of fs.readdirSync(root, { withFileTypes: true })) { if (!e.isDirectory()) continue; const dir = path.join(root, e.name); const f = path.join(dir, "SKILL.md"); if (!fs.existsSync(f)) { console.log("missing-skill-file:" + f); fail = 1; continue; } check(e.name, f); } } function check(id, f) { const t = fs.readFileSync(f, "utf8"); const m = t.match(/^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/); if (!m) { console.log("missing-frontmatter:" + f); fail = 1; return; } const fm = parse(m[1], f); if (fm === null) return; const n = fm.get("name"); if (n === undefined) { console.log("missing-name:" + f); fail = 1; } else { if (n.length < 1 || n.length > 64 || !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(n)) { console.log("bad-name:" + f + ":" + n); fail = 1; } if (n !== id) { console.log("name-dir-mismatch:" + f + ":" + n + "!=" + id); fail = 1; } if (seen.has(n)) { console.log("duplicate-name:" + f + ":" + n + ":also:" + seen.get(n)); fail = 1; } else seen.set(n, f); } const d = fm.get("description"); if (d === undefined || d.trim() === "") { console.log("missing-description:" + f); fail = 1; } else if (d.length > 1024) { console.log("long-description:" + f + ":" + d.length); fail = 1; } } function parse(body, f) { const out = new Map(); const lines = body.split("\n"); for (let i = 0; i < lines.length; i++) { const line = lines[i].replace(/\r$/, ""); if (line.trim() === "" || line.trim().startsWith("#")) continue; const kv = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/); if (!kv) { if (/^\s/.test(line)) continue; console.log("bad-frontmatter-line:" + f + ":" + line); fail = 1; return null; } let v = kv[2].trim(); if (!(v.startsWith("\"") || v.charCodeAt(0) === 39)) v = v.replace(/\s+#.*$/, "").trim(); if (v === ">" || v === ">-" || v === "|" || v === "|+") { const parts = []; i++; while (i < lines.length && /^\s+\S/.test(lines[i])) { parts.push(lines[i].trim()); i++; } i--; v = parts.join(" "); } else if (v.length >= 2 && ((v.startsWith("\"") && v.endsWith("\"")) || (v.charCodeAt(0) === 39 && v.charCodeAt(v.length - 1) === 39))) v = v.slice(1, -1); if (!out.has(kv[1])) out.set(kv[1], v); } return out; } process.exitCode = fail;"##;

/// Display name of the markdownlint step.
const MARKDOWNLINT_STEP_NAME: &str = "Run markdownlint";
/// Display name of the strict-JSON step.
const STRICT_JSON_STEP_NAME: &str = "Check strict JSON";
/// Display name of the frontmatter/ID step.
const FRONTMATTER_ID_STEP_NAME: &str = "Check frontmatter IDs";
/// Display name of the link-check step.
const LINK_CHECK_STEP_NAME: &str = "Check links";
/// Display name of the native-validators step.
const NATIVE_VALIDATORS_STEP_NAME: &str = "Run native validators";

/// Resolve enabled `[workflow.verify]` jobs to kinds, canonical order.
///
/// Unknown names fail closed (config validation normally rejects them
/// first; this guards unvalidated callers the same way).
///
/// # Errors
///
/// Returns a contract error for an unknown job name.
pub(crate) fn verify_kinds(jobs: &[String]) -> Result<Vec<ValidatorKind>, OrchestratorError> {
    let mut kinds = Vec::new();
    for kind in ValidatorKind::consumer_verify() {
        if jobs.iter().any(|job| job == kind.job_id()) {
            kinds.push(kind);
        }
    }
    if kinds.len() != jobs.len() {
        let unknown = jobs
            .iter()
            .find(|job| ValidatorKind::from_verify_name(job).is_none())
            .map_or("unknown", String::as_str);
        return Err(OrchestratorError::Contract {
            problem: format!("unknown_verify_job:{unknown}"),
        });
    }
    Ok(kinds)
}

/// Build the support command for one verification kind.
///
/// `Alint` needs none (it renders as a pinned action step); every
/// other consumer kind carries its fixed vector. Velnor-only and
/// always-on kinds fail closed.
///
/// # Errors
///
/// Returns contract or adapter errors for rejected vectors.
pub(crate) fn verify_command(
    kind: ValidatorKind,
    catalog: &ToolCatalog,
) -> Result<Option<ValidatorCommand>, OrchestratorError> {
    match kind {
        ValidatorKind::Alint => Ok(None),
        ValidatorKind::Zizmor => Ok(Some(ValidatorCommand {
            validator: kind,
            name: crate::vectors::ZIZMOR_STEP_NAME.to_owned(),
            argv: crate::vectors::zizmor_argv(catalog)?,
            prepare_argv: verify_install_argv("zizmor", catalog.version(PinnedTool::Zizmor))?,
        })),
        ValidatorKind::Markdownlint => Ok(Some(ValidatorCommand {
            validator: kind,
            name: MARKDOWNLINT_STEP_NAME.to_owned(),
            argv: markdownlint_argv()?,
            prepare_argv: verify_install_argv(MARKDOWNLINT_SPEC, MARKDOWNLINT_VERSION)?,
        })),
        ValidatorKind::StrictJson => Ok(Some(ValidatorCommand {
            validator: kind,
            name: STRICT_JSON_STEP_NAME.to_owned(),
            argv: node_eval_argv(STRICT_JSON_SCRIPT)?,
            prepare_argv: verify_install_argv(VERIFY_NODE_SPEC, VERIFY_NODE_VERSION)?,
        })),
        ValidatorKind::FrontmatterId => Ok(Some(ValidatorCommand {
            validator: kind,
            name: FRONTMATTER_ID_STEP_NAME.to_owned(),
            argv: node_eval_argv(FRONTMATTER_ID_SCRIPT)?,
            prepare_argv: verify_install_argv(VERIFY_NODE_SPEC, VERIFY_NODE_VERSION)?,
        })),
        ValidatorKind::LinkCheck => Ok(Some(ValidatorCommand {
            validator: kind,
            name: LINK_CHECK_STEP_NAME.to_owned(),
            argv: link_check_argv()?,
            prepare_argv: verify_install_argv(LYCHEE_SPEC, LYCHEE_VERSION)?,
        })),
        ValidatorKind::NativeValidators => Ok(Some(ValidatorCommand {
            validator: kind,
            name: NATIVE_VALIDATORS_STEP_NAME.to_owned(),
            argv: crate::source_prep::native_validators_argv(),
            prepare_argv: Vec::new(),
        })),
        ValidatorKind::CargoDeny | ValidatorKind::CargoMachete | ValidatorKind::Actionlint => {
            Err(OrchestratorError::Contract {
                problem: format!("verify_kind_rejected:{}", kind.job_id()),
            })
        }
    }
}

/// Fixed markdownlint vector: pinned tool over authored Markdown.
///
/// The `#`-negated globs drop vendored trees (`!` breaks shells that
/// parse exclamation inside double quotes) and the generator's own
/// `.github` docs (generation-diff already guards generated files;
/// the repo-owned PR template stays covered). The tool
/// auto-discovers a repo `.markdownlint-cli2.jsonc` when present.
fn markdownlint_argv() -> Result<Vec<String>, OrchestratorError> {
    crate::vectors::validator_argv(
        MARKDOWNLINT_SPEC,
        MARKDOWNLINT_VERSION,
        "markdownlint-cli2",
        &[
            "**/*.{md,markdown}",
            "#**/node_modules/**",
            "#.github/AGENTS.md",
        ],
    )
}

/// Fixed `node -e` vector for one single-line content script.
fn node_eval_argv(script: &str) -> Result<Vec<String>, OrchestratorError> {
    crate::vectors::validator_argv(
        VERIFY_NODE_SPEC,
        VERIFY_NODE_VERSION,
        "node",
        &["-e", script],
    )
}

/// Fixed lychee vector: pinned checker over repo Markdown.
fn link_check_argv() -> Result<Vec<String>, OrchestratorError> {
    crate::vectors::validator_argv(
        LYCHEE_SPEC,
        LYCHEE_VERSION,
        "lychee",
        &["--no-progress", "**/*.md", "**/*.markdown"],
    )
}

/// Explicit pinned install pairing one verify `exec` vector.
///
/// The spec must match the `exec` spec exactly; the renderer
/// install/exec closure gate and the lock audit both key on it.
fn verify_install_argv(spec: &str, version: &str) -> Result<Vec<String>, OrchestratorError> {
    let install = IsolatedCommand::mise_install(&[format!("{spec}@{version}")]).map_err(|err| {
        OrchestratorError::Contract {
            problem: err.to_string(),
        }
    })?;
    crate::utf8::strings_of(install.argv())
        .map_err(|problem| OrchestratorError::Contract { problem })
}

/// Append fixed commands for the selection, skipping kinds that
/// already carry one (the Velnor policy shares the zizmor vector).
///
/// # Errors
///
/// Returns contract or adapter errors for rejected vectors.
pub(crate) fn push_verify_commands(
    commands: &mut Vec<ValidatorCommand>,
    verify: &[ValidatorKind],
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    for kind in verify {
        if commands.iter().any(|command| command.validator == *kind) {
            continue;
        }
        if let Some(command) = verify_command(*kind, catalog)? {
            commands.push(command);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod verify_tests;
