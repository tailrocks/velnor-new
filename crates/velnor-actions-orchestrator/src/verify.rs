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
use velnor_actions_contract::VerifyConfig;
use velnor_actions_mise::{IsolatedCommand, PinnedTool, ToolCatalog};
use velnor_actions_workflow_renderer::render::ValidatorCommand;

use crate::OrchestratorError;
use crate::vectors::{
    CARGO_DENY_VERSION, CARGO_MACHETE_TOOL_SPEC, CARGO_MACHETE_VERSION, LYCHEE_TOOL_SPEC,
    LYCHEE_VERSION,
};

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

/// Resolve an emitted validator install spec to its pinned name and version.
///
/// Supported validator installation pins. The version must equal the pinned
/// const or the emitted shape drifted and the audit fails closed.
#[must_use]
pub(crate) fn validator_install_pin(spec: &str) -> Option<(&'static str, &str, &'static str)> {
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
        "zizmor" if velnor_actions_contract::is_exact_semver(version) => {
            Some(("zizmor", version, "zizmor"))
        }
        "npm:markdownlint-cli2" if version == MARKDOWNLINT_VERSION => Some((
            "markdownlint-cli2",
            MARKDOWNLINT_VERSION,
            "npm:markdownlint-cli2",
        )),
        "node" if version == VERIFY_NODE_VERSION => Some(("node", VERIFY_NODE_VERSION, "node")),
        LYCHEE_TOOL_SPEC if version == LYCHEE_VERSION => {
            Some(("lychee", LYCHEE_VERSION, LYCHEE_TOOL_SPEC))
        }
        _ => None,
    }
}

/// Mise tool spec for the markdownlint vector.
const MARKDOWNLINT_SPEC: &str = "npm:markdownlint-cli2";
/// Mise tool spec for the fixed content checks.
const VERIFY_NODE_SPEC: &str = "node";
/// Mise tool spec for the link-check vector.
const LYCHEE_SPEC: &str = LYCHEE_TOOL_SPEC;

/// Fixed strict-JSON check: every `*.json` outside the skipped trees
/// must parse (rejecting comments, trailing commas, syntax errors)
/// and must not repeat an object key (compared unescaped).
///
/// Single-line `node -e` payload: no newlines, backticks, `$(`, or
/// bare `$` expansions, so it passes the renderer argv gates.
const STRICT_JSON_SCRIPT: &str = r#"const fs = require("fs"), path = require("path"); let fail = 0; const skip = new Set([".git", "node_modules", "target"]); function walk(d) { for (const e of fs.readdirSync(d, { withFileTypes: true })) { const p = path.join(d, e.name); if (e.isDirectory()) { if (!skip.has(e.name)) walk(p); } else if (e.name.endsWith(".json")) check(p); } } function check(f) { const s = fs.readFileSync(f, "utf8"); try { JSON.parse(s); } catch (e) { console.log("invalid-json:" + f + ":" + e.message); fail = 1; return; } const d = dups(s); if (d !== null) { console.log("duplicate-key:" + f + ":" + d); fail = 1; } } function dups(s) { const st = [new Set()]; let i = 0; while (i < s.length) { const c = s[i]; if (c === "\"") { let j = i + 1; while (j < s.length) { if (s[j] === "\\") { j += 2; continue; } if (s[j] === "\"") break; j++; } let k = j + 1; while (k < s.length && /\s/.test(s[k])) k++; if (s[k] === ":") { const t = st[st.length - 1]; const v = JSON.parse(s.slice(i, j + 1)); if (t.has(v)) return v; t.add(v); } i = j + 1; continue; } if (c === "{") st.push(new Set()); else if (c === "}") st.pop(); i++; } return null; } const files = JSON.parse(process.argv[1] || "null"); if (files === null) walk("."); else for (const f of files) { let p = ".", safe = true; for (const part of f.split("/")) { p = path.join(p, part); try { if (fs.lstatSync(p).isSymbolicLink()) safe = false; } catch { safe = false; } } if (!safe || path.extname(f).toLowerCase() !== ".json" || !fs.existsSync(f) || !fs.statSync(f).isFile()) { console.log("invalid-json-path:" + f); fail = 1; } else check(f); } process.exitCode = fail;"#;

/// Frontmatter/ID check over `skills/*/SKILL.md` (Agent Skills spec).
/// It supports block and plain multiline scalars plus single-line
/// quoted scalars; unsupported quoted multiline and structured required
/// values fail closed. `name` is required, 1-64 chars, kebab-case, equal to the
/// parent directory, and unique repo-wide; `description` is required,
/// non-empty, and at most 1024 chars.
///
/// A fixed script because no suitable pinned tool exists: the npm
/// `skills-ref` publisher is unofficial and the official `PyPI`
/// `skills-ref` (alpha) needs the `pipx:` backend, which failed to
/// bootstrap in probing. Same single-line argv discipline as above.
const FRONTMATTER_ID_SCRIPT: &str = r##"const fs = require("fs"), path = require("path"); let fail = 0; const q = String.fromCharCode(39), seen = new Map(); const root = "skills"; if (fs.existsSync(root)) { for (const e of fs.readdirSync(root, { withFileTypes: true })) { if (!e.isDirectory()) continue; const dir = path.join(root, e.name); const f = path.join(dir, "SKILL.md"); if (!fs.existsSync(f)) { console.log("missing-skill-file:" + f); fail = 1; continue; } check(e.name, f); } } function check(id, f) { const t = fs.readFileSync(f, "utf8"); const m = t.match(/^---\r?\n([\s\S]*?)\r?\n---(\r?\n|$)/); if (!m) { console.log("missing-frontmatter:" + f); fail = 1; return; } const fm = parse(m[1], f); if (fm === null) return; const n = fm.get("name"); if (n === undefined) { console.log("missing-name:" + f); fail = 1; } else { if (n.length < 1 || n.length > 64 || !/^[a-z0-9]+(-[a-z0-9]+)*$/.test(n)) { console.log("bad-name:" + f + ":" + n); fail = 1; } if (n !== id) { console.log("name-dir-mismatch:" + f + ":" + n + "!=" + id); fail = 1; } if (seen.has(n)) { console.log("duplicate-name:" + f + ":" + n + ":also:" + seen.get(n)); fail = 1; } else seen.set(n, f); } const d = fm.get("description"); if (d === undefined || d.trim() === "") { console.log("missing-description:" + f); fail = 1; } else if (d.length > 1024) { console.log("long-description:" + f + ":" + d.length); fail = 1; } } function decodeYamlDoubleQuoted(raw) { const simple = { "0": "\0", "a": "\x07", "b": "\b", "t": "\t", "n": "\n", "v": "\x0b", "f": "\f", "r": "\r", "e": "\x1b", " ": " ", "\"": "\"", "/": "/", "\\": "\\", "N": "\u0085", "_": "\u00a0", "L": "\u2028", "P": "\u2029" }; let out = ""; for (let i = 0; i < raw.length; i++) { if (raw[i] !== "\\") { out += raw[i]; continue; } const escape = raw[++i]; if (Object.prototype.hasOwnProperty.call(simple, escape)) { out += simple[escape]; continue; } const count = escape === "x" ? 2 : escape === "u" ? 4 : escape === "U" ? 8 : 0; const hex = raw.slice(i + 1, i + 1 + count); if (!count || hex.length !== count || !/^[0-9a-fA-F]+$/.test(hex)) return null; const codepoint = Number.parseInt(hex, 16); if (codepoint > 0x10ffff || (codepoint >= 0xd800 && codepoint <= 0xdfff)) return null; out += String.fromCodePoint(codepoint); i += count; } return out; } function quotedScalarEnd(value) { const quote = value[0]; if (quote.charCodeAt(0) !== 34 && quote !== q) return -1; for (let end = 1; end < value.length; end++) { if (quote.charCodeAt(0) === 34 && value[end].charCodeAt(0) === 92) { end++; continue; } if (value[end] === quote) { if (quote === q && value[end + 1] === q) { end++; continue; } return end; } } return -1; } function quotedMapping(value) { const end = quotedScalarEnd(value); return end >= 0 && value.slice(end + 1).trimStart().startsWith(":"); } function decodeYamlQuotedNode(value) { const quote = value[0], end = quotedScalarEnd(value), rawTail = value.slice(end + 1), tail = rawTail.trim(); if (end < 0 || (tail !== "" && !/^\s+#/.test(rawTail))) return undefined; if (quote.charCodeAt(0) === 34) return decodeYamlDoubleQuoted(value.slice(1, end)); return value.slice(1, end).split(q + q).join(q); } function unsupportedEmptyNode(value) { if (quotedMapping(value)) return true; if (value[0] === q || value.charCodeAt(0) === 34) return false; return /^(?:-(?:\s|$)|\?(?:\s|$)|:(?:\s|$)|[\[\]{},!&*|>@%])/.test(value) || /^[^:#][^:]*:(?:\s|$)/.test(value); } function implicitYamlNonString(value) { return /^(?:null|~|true|false|[-+]?(?:0b[01](?:_?[01])*|0o[0-7](?:_?[0-7])*|0x[0-9a-f](?:_?[0-9a-f])*|(?:[0-9](?:_?[0-9])*(?:\.(?:[0-9](?:_?[0-9])*)?)?|\.[0-9](?:_?[0-9])*)(?:[eE][-+]?[0-9](?:_?[0-9])*)?|\.(?:inf|nan)))$/i.test(value); } function parse(body, f) { const out = new Map(); const lines = body.split("\n"); for (let i = 0; i < lines.length; i++) { const line = lines[i].replace(/\r$/, ""); if (line.trim() === "" || line.trim().startsWith("#")) continue; const kv = line.match(/^([A-Za-z0-9_-]+):\s*(.*)$/); if (!kv) { if (/^\s/.test(line)) continue; console.log("bad-frontmatter-line:" + f + ":" + line); fail = 1; return null; } const key = kv[1]; let value = kv[2].trim(); if (!value.startsWith("\"") && !value.startsWith(q)) value = value.replace(/\s+#.*$/, "").trim(); if (out.has(key)) { console.log("duplicate-frontmatter-key:" + f + ":" + key); fail = 1; return null; } const block = value.match(/^([>|])(?:([1-9])([+-])?|([+-])([1-9])?)?$/); if (block) { const keyIndent = (line.match(/^\s*/) || [""])[0].length; const indent = block[2] || block[5]; const chomp = block[3] || block[4]; let end = i + 1, contentIndent = indent ? keyIndent + Number(indent) : null; for (let j = i + 1; j < lines.length; j++) { const raw = lines[j].replace(/\r$/, ""); const spaces = (raw.match(/^ */) || [""])[0].length; if (raw.trim() !== "" && spaces <= keyIndent) break; if (contentIndent === null && raw.trim() !== "") contentIndent = spaces; end = j + 1; } contentIndent ??= keyIndent + 1; if (lines.slice(i + 1, end).some(raw => raw.trim() !== "" && (raw.match(/^ */) || [""])[0].length < contentIndent)) { console.log("bad-frontmatter-indent:" + f + ":" + key); fail = 1; return null; } const rows = lines.slice(i + 1, end).map(raw => { raw = raw.replace(/\r$/, ""); return raw.trim() === "" ? "" : raw.slice(contentIndent); }); let trailing = 0; while (rows.length && rows[rows.length - 1] === "") { rows.pop(); trailing++; } let text = "", pending = 0, previousMore = false; for (const row of rows) { if (row === "") { pending++; continue; } const more = row.startsWith(" "); if (text || pending) text += block[1] === ">" && pending === 0 && !more && !previousMore ? " " : "\n".repeat(text ? (block[1] === "|" ? pending + 1 : Math.max(1, pending)) : pending); text += row; pending = 0; previousMore = more; } if (chomp === "+") text += "\n".repeat(trailing + (rows.length ? 1 : 0)); else if (chomp !== "-" && rows.length) text += "\n"; value = text; i = end - 1; } else if (value.startsWith("\"") || value.startsWith(q)) { const quote = value[0]; let end = 1; if (quote === "\"") { for (; end < value.length; end++) { if (value[end] === "\\") { end++; continue; } if (value[end] === quote) break; } } else { for (; end < value.length; end++) { if (value[end] === q) { if (value[end + 1] === q) { end++; continue; } break; } } } const rawTail = value.slice(end + 1), tail = rawTail.trim(); if (end >= value.length || (tail !== "" && !/^\s+#/.test(rawTail))) { console.log("bad-frontmatter-scalar:" + f + ":" + key); fail = 1; return null; } if (quote === "\"") { value = decodeYamlDoubleQuoted(value.slice(1, end)); if (value === null) { console.log("bad-frontmatter-scalar:" + f + ":" + key); fail = 1; return null; } } else value = value.slice(1, end).split(q + q).join(q); } else { value = value.replace(/\s+#.*$/, "").trim(); if ((key === "name" || key === "description") && (value.startsWith("[") || value.startsWith("{"))) { console.log("non-string-frontmatter-scalar:" + f + ":" + key); fail = 1; return null; } if ((key === "name" || key === "description") && (/^(?:[&*!])/.test(value))) { console.log("unsupported-frontmatter-alias-or-tag:" + f + ":" + key); fail = 1; return null; } let quotedEmptyChild = false; if ((key === "name" || key === "description") && value === "") { const child = lines.slice(i + 1).map((raw, offset) => ({ raw: raw.replace(/\r$/, ""), index: i + 1 + offset })).find(entry => entry.raw.trim() !== "" && !entry.raw.trim().startsWith("#")); if (child && /^\s+/.test(child.raw)) { const node = child.raw.trim(); if (unsupportedEmptyNode(node)) { console.log("non-string-frontmatter-scalar:" + f + ":" + key); fail = 1; return null; } if (node.charCodeAt(0) === 34 || node[0] === q) { value = decodeYamlQuotedNode(node); if (value === undefined || value === null) { console.log("bad-frontmatter-scalar:" + f + ":" + key); fail = 1; return null; } i = child.index; quotedEmptyChild = true; } } } const continuation = []; for (let j = i + 1; j < lines.length; j++) { const raw = lines[j].replace(/\r$/, ""); if (raw.trim() === "") { continuation.push(""); i = j; continue; } if (!/^\s+/.test(raw)) break; if (/^\s*#/.test(raw)) { i = j; continue; } const part = raw.trim().replace(/\s+#.*$/, "").trim(); if (part !== "") continuation.push(part); i = j; } if (continuation.length) { const parts = [value, ...continuation]; value = ""; let pending = 0; for (const part of parts) { if (part === "") { pending++; continue; } if (value) value += pending ? "\n".repeat(pending) : " "; value += part; pending = 0; } } if ((key === "name" || key === "description") && !quotedEmptyChild && implicitYamlNonString(value)) { console.log("non-string-frontmatter-scalar:" + f + ":" + key); fail = 1; return null; } } out.set(key, value); } return out; } process.exitCode = fail;"##;

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
    options: &VerifyConfig,
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
            argv: strict_json_argv(options.strict_json_files.as_deref())?,
            prepare_argv: verify_install_argv(VERIFY_NODE_SPEC, VERIFY_NODE_VERSION)?,
        })),
        ValidatorKind::FrontmatterId => Ok(Some(ValidatorCommand {
            validator: kind,
            name: FRONTMATTER_ID_STEP_NAME.to_owned(),
            argv: node_eval_argv(FRONTMATTER_ID_SCRIPT, &[])?,
            prepare_argv: verify_install_argv(VERIFY_NODE_SPEC, VERIFY_NODE_VERSION)?,
        })),
        ValidatorKind::LinkCheck => Ok(Some(ValidatorCommand {
            validator: kind,
            name: LINK_CHECK_STEP_NAME.to_owned(),
            argv: link_check_argv(&options.lychee_args)?,
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
            "#.github/CLAUDE.md",
        ],
    )
}

/// Fixed `node -e` vector for one single-line content script.
fn node_eval_argv(script: &str, args: &[String]) -> Result<Vec<String>, OrchestratorError> {
    let mut command = vec!["-e".to_owned(), script.to_owned()];
    command.extend_from_slice(args);
    crate::vectors::validator_argv_owned(VERIFY_NODE_SPEC, VERIFY_NODE_VERSION, "node", &command)
}

fn strict_json_argv(files: Option<&[String]>) -> Result<Vec<String>, OrchestratorError> {
    let files = files
        .map(serde_json::to_string)
        .transpose()
        .map_err(|err| OrchestratorError::Contract {
            problem: format!("strict_json_scope_encoding:{err}"),
        })?;
    node_eval_argv(
        STRICT_JSON_SCRIPT,
        &[files.unwrap_or_else(|| "null".to_owned())],
    )
}

/// Fixed lychee vector: pinned checker over repo Markdown.
fn link_check_argv(extra_args: &[String]) -> Result<Vec<String>, OrchestratorError> {
    let mut args = vec!["--no-progress".to_owned()];
    args.extend_from_slice(extra_args);
    args.extend(["**/*.md".to_owned(), "**/*.markdown".to_owned()]);
    crate::vectors::validator_argv_owned(LYCHEE_SPEC, LYCHEE_VERSION, "lychee", &args)
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
    options: &VerifyConfig,
    catalog: &ToolCatalog,
) -> Result<(), OrchestratorError> {
    for kind in verify {
        if commands.iter().any(|command| command.validator == *kind) {
            continue;
        }
        if let Some(command) = verify_command(*kind, options, catalog)? {
            commands.push(command);
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "verify_tests.rs"]
mod verify_tests;
