//! Dialect evidence: STRONG / WEAK / CONFLICT classification.
//!
//! Filename and toolname signals (E1–E3) classify without content;
//! E4 content signals (`required_version` + legacy CLI refs, and
//! terraform-only version constraints) need bounded file text via
//! [`classify_with_contents`]. `.terraform/` working dirs are shared
//! by both dialects, so they never count as markers.

use std::collections::{BTreeMap, BTreeSet};

use crate::content::signals_for;
use crate::effective::{Dialect, config_shape, effective_set};
use crate::modules::{ModuleSource, SourceClass, classify_literal, resolve_local_target};
use crate::parser::{parse_json, parse_native};

/// Mise tool selecting `OpenTofu` (STRONG E3).
pub const MISE_OPENTOFU_TOOL: &str = "opentofu";
/// Mise tool selecting Terraform (CONFLICT marker).
pub const MISE_TERRAFORM_TOOL: &str = "terraform";
/// Terraform CLI config directory marker (any depth).
const TERRAFORM_D_SEGMENT: &str = ".terraform.d";
/// Sightings kept per signal class before a `more:` overflow entry.
const MAX_SIGHTINGS: usize = 8;

/// Classified dialect evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceLevel {
    /// No tofu evidence (terraform markers alone land here too).
    None,
    /// `.tf`/`.tf.json` only: ambiguous, never silently claimed.
    Weak,
    /// Tofu intent: native spellings or the Mise opentofu tool.
    Strong,
    /// Terraform markers alongside STRONG evidence: hard error.
    Conflict,
}

/// Classified evidence plus advisory root inference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence {
    /// Evidence level.
    pub level: EvidenceLevel,
    /// Stable `class:detail` signals, capped with `more:` overflow.
    pub signals: Vec<String>,
    /// Advisory inferred roots (config spellings, `.` for the root).
    ///
    /// Set only when exactly one non-module directory holds
    /// effective config (local module targets parsed from contents
    /// never infer; malformed contents infer nothing).
    pub inferred: Vec<String>,
}

/// Plan-visible tofu note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TofuNote {
    /// Stack suppressed via `stacks.ignore`.
    Ignored,
    /// Table-less evidence advisory (never a detection claim).
    Advisory(Advisory),
}

/// Table-less evidence advisory for plan output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    /// True for STRONG, false for WEAK.
    pub strong: bool,
    /// Evidence signals.
    pub signals: Vec<String>,
    /// Advisory inferred roots (empty unless unambiguous).
    pub inferred: Vec<String>,
}

/// True when flattened mise.toml values select `tool`.
///
/// Matches `tools.<tool>` and `tools.<tool>.<attr>` keys (scalar and
/// table tool forms). Lock entries are not consulted: lock internals
/// belong to the Mise adapter (a later task owns that API).
#[must_use]
pub fn mise_tool_selected(values: &BTreeMap<String, String>, tool: &str) -> bool {
    let exact = format!("tools.{tool}");
    let nested = format!("tools.{tool}.");
    values
        .keys()
        .any(|key| key == &exact || key.starts_with(&nested))
}

/// Classify repo-wide evidence from index files plus mise selections.
///
/// Filename/toolname signals only; content stays unread. The caller
/// handles the explicit-table (E1) path separately; this classifies
/// the table-less case only.
#[must_use]
pub fn classify(files: &[String], mise_values: &BTreeMap<String, String>) -> Evidence {
    classify_with_contents(files, &BTreeMap::new(), mise_values)
}

/// Classify with E4 content signals from bounded `contents` text.
///
/// `contents` maps repo-relative config paths to bounded text;
/// malformed entries contribute no signals (no table, no claim).
/// `required_version` TOGETHER with a legacy ref is STRONG;
/// terraform-only pins are markers (CONFLICT when STRONG holds).
#[must_use]
pub fn classify_with_contents(
    files: &[String],
    contents: &BTreeMap<String, String>,
    mise_values: &BTreeMap<String, String>,
) -> Evidence {
    let mut native = Vec::new();
    let mut legacy = Vec::new();
    for path in files {
        let name = path.rsplit('/').next().unwrap_or(path);
        if let Some(shape) = config_shape(name) {
            if shape.tofu_spelling {
                native.push(path.clone());
            } else {
                legacy.push(path.clone());
            }
        }
    }
    let mut versioned = Vec::new();
    let mut legacy_refs = Vec::new();
    let mut pinned = Vec::new();
    for (path, text) in contents {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(shape) = config_shape(name) else {
            continue;
        };
        let Ok(signals) = signals_for(text, shape.dialect) else {
            continue;
        };
        if !signals.required_versions.is_empty() {
            versioned.push(path.clone());
        }
        if signals.has_legacy_ref {
            legacy_refs.push(path.clone());
        }
        if signals.terraform_only {
            pinned.push(path.clone());
        }
    }
    let strong_tool = mise_tool_selected(mise_values, MISE_OPENTOFU_TOOL);
    let terraform_tool = mise_tool_selected(mise_values, MISE_TERRAFORM_TOOL);
    let terraform_dirs: Vec<String> = files
        .iter()
        .filter(|path| {
            path.split('/')
                .any(|segment| segment == TERRAFORM_D_SEGMENT)
        })
        .cloned()
        .collect();
    let strong =
        !native.is_empty() || strong_tool || (!versioned.is_empty() && !legacy_refs.is_empty());
    let markers = terraform_tool || !terraform_dirs.is_empty() || !pinned.is_empty();
    let level = if markers && strong {
        EvidenceLevel::Conflict
    } else if strong {
        EvidenceLevel::Strong
    } else if markers {
        // Markers without STRONG: terraform's territory, not tofu's.
        EvidenceLevel::None
    } else if !legacy.is_empty() {
        EvidenceLevel::Weak
    } else {
        EvidenceLevel::None
    };
    let mut signals = Vec::new();
    push_sightings(&mut signals, "tofu-spelling", &native);
    if strong_tool {
        signals.push("mise-tool:opentofu".to_owned());
    }
    push_sightings(&mut signals, "content:required-version", &versioned);
    push_sightings(&mut signals, "content:legacy-ref", &legacy_refs);
    if matches!(level, EvidenceLevel::Weak) {
        push_sightings(&mut signals, "legacy-only", &legacy);
    }
    if terraform_tool {
        signals.push("terraform-marker:mise-tool:terraform".to_owned());
    }
    push_sightings(&mut signals, "terraform-marker:path", &terraform_dirs);
    push_sightings(&mut signals, "terraform-marker:required-version", &pinned);
    Evidence {
        level,
        signals,
        inferred: infer_roots(files, contents),
    }
}

/// Plan note for displayable levels; CONFLICT errors, `None` stays silent.
#[must_use]
pub fn plan_note(evidence: &Evidence) -> Option<TofuNote> {
    match evidence.level {
        EvidenceLevel::Weak => Some(TofuNote::Advisory(Advisory {
            strong: false,
            signals: evidence.signals.clone(),
            inferred: evidence.inferred.clone(),
        })),
        EvidenceLevel::Strong => Some(TofuNote::Advisory(Advisory {
            strong: true,
            signals: evidence.signals.clone(),
            inferred: evidence.inferred.clone(),
        })),
        EvidenceLevel::None | EvidenceLevel::Conflict => None,
    }
}

/// Push capped `class:detail` sightings plus a `more:` overflow entry.
fn push_sightings(signals: &mut Vec<String>, class: &str, paths: &[String]) {
    for path in paths.iter().take(MAX_SIGHTINGS) {
        signals.push(format!("{class}:{path}"));
    }
    if paths.len() > MAX_SIGHTINGS {
        signals.push(format!("more:{class}:{}", paths.len() - MAX_SIGHTINGS));
    }
}

/// Advisory roots: the single non-module effective-config dir, if any.
///
/// Local module targets parsed from `contents` never infer: only a
/// lone surviving caller dir advises. Malformed contents or escaping
/// sources abandon inference (no table, no claim).
fn infer_roots(files: &[String], contents: &BTreeMap<String, String>) -> Vec<String> {
    let survivors = effective_set(files);
    let dirs: BTreeSet<&str> = survivors
        .iter()
        .map(|path| path.rsplit_once('/').map_or("", |(dir, _)| dir))
        .collect();
    let Some(targets) = module_targets(contents) else {
        return Vec::new();
    };
    let remaining: Vec<&&str> = dirs.iter().filter(|dir| !targets.contains(**dir)).collect();
    if remaining.len() == 1 {
        let dir = remaining[0];
        return vec![if dir.is_empty() {
            ".".to_owned()
        } else {
            (*dir).to_owned()
        }];
    }
    Vec::new()
}

/// Local module targets across `contents`; `None` abandons inference.
fn module_targets(contents: &BTreeMap<String, String>) -> Option<BTreeSet<String>> {
    let mut targets = BTreeSet::new();
    for (path, text) in contents {
        let name = path.rsplit('/').next().unwrap_or(path);
        let Some(shape) = config_shape(name) else {
            continue;
        };
        let model = match shape.dialect {
            Dialect::Native => parse_native(text),
            Dialect::Json => parse_json(text),
        }
        .ok()?;
        let caller = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        for decl in &model.modules {
            let ModuleSource::Literal(source) = &decl.source else {
                continue;
            };
            if classify_literal(source) != SourceClass::Local {
                continue;
            }
            targets.insert(resolve_local_target(caller, source)?);
        }
    }
    Some(targets)
}
