//! Gate-6-gated task-cache enablement: qualified runs and fixture tokens.
//!
//! Only qualified tasks may use the experimental artifact cache, and the
//! task TOML cache field is a schema change enabled only with Gate-6
//! qualification fixtures. Both gates live here so callers cannot reach
//! the invocation shapes or the renderer without passing them.

use crate::cache::{
    QualifiedTaskDef, TaskCacheMode, qualify_reuse, render_task_toml, task_run_argv,
};
use crate::error::MiseError;

/// Fixed `mise run --task-cache <mode> <task> --file <path>` argv for a
/// qualified task only.
///
/// Reuse qualification runs first, so an unqualified task kind or a
/// nondeterministic task can never reach the invocation shape.
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] for unqualified tasks and
/// [`MiseError::ArtifactEscapesRoot`] for task files outside runner temp.
pub fn qualified_task_run_argv(
    kind: &str,
    network: bool,
    clock: bool,
    random: bool,
    mode: TaskCacheMode,
    task: &str,
    file: &str,
) -> Result<Vec<String>, MiseError> {
    qualify_reuse(kind, network, clock, random)?;
    task_run_argv(mode, task, file)
}

/// Gate-6 qualification fixture token.
///
/// Names the fixture evidence enabling task-cache TOML rendering; only the
/// `gate6/<name>` shape builds. Qualification suites own the fixture ids;
/// the token keeps every render call site auditable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Gate6Fixture {
    /// Fixture id in `gate6/<name>` shape.
    id: String,
}

impl Gate6Fixture {
    /// Build a token for one fixture id.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] unless the id has the
    /// `gate6/<name>` shape with a non-empty slash-free name.
    pub fn new(id: &str) -> Result<Self, MiseError> {
        let name = id.strip_prefix("gate6/").unwrap_or("");
        let valid = !name.is_empty()
            && !name.contains('/')
            && !name.contains("..")
            && !name.bytes().any(|byte| byte.is_ascii_whitespace());
        if valid {
            Ok(Self { id: id.to_owned() })
        } else {
            Err(MiseError::CacheNotEligible {
                task: id.to_owned(),
                reason: "bad_gate6_fixture".to_owned(),
            })
        }
    }

    /// Fixture id in `gate6/<name>` shape.
    #[must_use]
    pub fn id(&self) -> &str {
        &self.id
    }
}

/// Render a versioned task TOML only with Gate-6 fixture evidence.
///
/// The sole public rendering path: callers name the qualifying fixture,
/// and the marker plus fixed fields render exactly as specified.
///
/// # Errors
///
/// Returns [`MiseError::CacheNotEligible`] for incomplete definitions.
pub fn render_gated_task_toml(
    version: &str,
    def: &QualifiedTaskDef,
    fixture: &Gate6Fixture,
) -> Result<String, MiseError> {
    debug_assert!(Gate6Fixture::new(fixture.id()).is_ok());
    render_task_toml(version, def)
}
