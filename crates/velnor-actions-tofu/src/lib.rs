//! `OpenTofu` stack adapter: registration, roots, and dialect evidence.
//!
//! T09 owns strict minimal `[stacks.tofu]` roots plus STRONG / WEAK /
//! CONFLICT evidence. T10 adds the bounded structural parser (S8),
//! file families, effective precedence, independent fmt scope (S2),
//! and E4 content signals, and converts configured roots to detector
//! records, inventories, toolchain inputs, and closures. Module
//! edges (T11) and task proposals (T12) stay out. This crate launches
//! no processes, builds no tool invocations, and renders no workflow
//! text.

pub mod closure;
pub mod content;
pub mod detect;
pub mod effective;
pub mod evidence;
pub mod family;
pub mod fmt_scope;
pub mod kinds;
pub mod parser;
pub mod parser_json;
pub mod roots;
pub mod units;
pub mod version;

pub use closure::resolve_closure_at_root;
pub use content::{ContentSignals, signals_for};
pub use detect::{detected_projects_for_units, discover_stack_candidates, manifest_for_unit_root};
pub use effective::{Dialect, config_shape, dir_has_effective_config, effective_set};
pub use evidence::{
    Advisory, Evidence, EvidenceLevel, MISE_OPENTOFU_TOOL, MISE_TERRAFORM_TOOL, TofuNote, classify,
    classify_with_contents, mise_tool_selected, plan_note,
};
pub use family::{Family, LOCKFILE_NAME, family_of, is_auto_var, is_override_stem};
pub use fmt_scope::{fmt_scope_for_root, fmt_set, is_excluded_name, is_fmt_file, under_hidden_dir};
pub use kinds::TofuTaskKind;
pub use parser::{
    BlockModel, FileModel, MAX_DEPTH, MAX_DIAGNOSTIC_CHARS, MAX_FILE_BYTES, MAX_FILES_PER_UNIT,
    MAX_NODES, ParseError, has_legacy_ref_text, parse_json, parse_native, strip_template_spans,
};
pub use roots::qualify_roots;
pub use units::{AnalyzedUnit, UnitError, analyze_files, files_for_prefix};
pub use version::{OPENTOFU_FLOOR, admits_opentofu, is_terraform_only};

/// Stable identifier for the `OpenTofu` stack.
pub const STACK_ID: &str = "tofu";
