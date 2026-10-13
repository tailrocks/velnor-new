//! Gate-6 task-result reuse: qualification, keys, and opaque transport.
//!
//! Typed requests around the cache helpers: reuse qualification mints an
//! unforgeable grant, key derivation requires a matching grant, and the
//! local-artifact transport opens only under Gate-6 fixture evidence.
//! Release events never reuse: every constructor refuses them.

use std::path::{Path, PathBuf};

use crate::cache::{
    CachedTaskDescriptor, TaskCacheMode, artifact_path, qualify_reuse, resolve_task_artifact_dir,
};
use crate::gate6::{Gate6Fixture, qualified_task_run_argv};
use crate::restore::{MissReason, ReuseFallback, ToolAvailability, fallback_for_error};
use velnor_actions_mise_core::command::is_cancel_or_timeout;
use velnor_actions_mise_core::error::MiseError;

/// Event name that never reuses a cached task result.
const RELEASE_EVENT: &str = "release";

/// Nondeterminism or undeclared-state signal disabling reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReuseSignal {
    /// Task reads the network.
    Network,
    /// Task reads a clock.
    Clock,
    /// Task reads randomness.
    Random,
    /// Task reads undeclared state.
    UndeclaredState,
}

/// Reuse qualification inputs for one task kind on one event (REUSE-1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReuseQualification {
    /// Task kind under qualification (`publish`-like kinds never qualify).
    kind: String,
    /// Signals in [`ReuseSignal`] order: network, clock, random, state.
    signals: [bool; 4],
    /// Workflow event under qualification (releases never qualify).
    event: String,
}

impl ReuseQualification {
    /// Qualify `kind` on `event`; reuse signals default to absent.
    #[must_use]
    pub fn new(kind: &str, event: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            signals: [false; 4],
            event: event.to_owned(),
        }
    }

    /// Record one reuse-disabling signal.
    #[must_use]
    pub fn with_signal(mut self, signal: ReuseSignal) -> Self {
        match signal {
            ReuseSignal::Network => self.signals[0] = true,
            ReuseSignal::Clock => self.signals[1] = true,
            ReuseSignal::Random => self.signals[2] = true,
            ReuseSignal::UndeclaredState => self.signals[3] = true,
        }
        self
    }

    /// Task kind under qualification.
    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    /// Workflow event under qualification.
    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    /// True when the task must always run: publishing kinds or any reuse
    /// signal (nondeterminism or undeclared state).
    #[must_use]
    pub fn always_run(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "publish" | "deploy" | "notify" | "service"
        ) || self.signals.contains(&true)
    }

    /// Check reuse and mint the grant proving it.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] for release events, `Off`
    /// mode, unqualified kinds, nondeterminism, or undeclared state.
    pub fn check(&self, mode: TaskCacheMode) -> Result<ReuseGrant, MiseError> {
        if self.event == RELEASE_EVENT || mode == TaskCacheMode::Off {
            return Err(MiseError::CacheNotEligible {
                task: self.kind.clone(),
                reason: MissReason::FORCED_UNCACHED.as_str().to_owned(),
            });
        }
        let [network, clock, random, undeclared_state] = self.signals;
        qualify_reuse(&self.kind, network, clock, random)?;
        if undeclared_state {
            return Err(MiseError::CacheNotEligible {
                task: self.kind.clone(),
                reason: MissReason::TASK_NOT_ELIGIBLE.as_str().to_owned(),
            });
        }
        Ok(ReuseGrant {
            task: self.kind.clone(),
            mode,
        })
    }
}

/// Proof that reuse qualification passed; only [`ReuseQualification::check`]
/// mints it, so downstream constructors cannot be reached unqualified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReuseGrant {
    /// Qualified task kind.
    task: String,
    /// Cache mode the grant was minted for.
    mode: TaskCacheMode,
}

impl ReuseGrant {
    /// Qualified task kind.
    #[must_use]
    pub fn task(&self) -> &str {
        &self.task
    }

    /// Cache mode the grant was minted for.
    #[must_use]
    pub fn mode(&self) -> TaskCacheMode {
        self.mode
    }
}

/// Typed task-reuse request: declared descriptor, no undeclared reads, a
/// grant (REUSE-2). Undeclared inputs fail here, before any execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskReuseRequest {
    /// Declared cache-key inputs.
    descriptor: CachedTaskDescriptor,
    /// Qualification grant backing this request.
    grant: ReuseGrant,
}

impl TaskReuseRequest {
    /// Build the request, rejecting undeclared inputs before execution.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] when `undeclared_reads` is
    /// non-empty, the grant names another task, or the descriptor is invalid.
    pub fn new(
        descriptor: CachedTaskDescriptor,
        undeclared_reads: &[String],
        grant: ReuseGrant,
    ) -> Result<Self, MiseError> {
        if !undeclared_reads.is_empty() || grant.task() != descriptor.task_name {
            return Err(MiseError::CacheNotEligible {
                task: descriptor.task_name.clone(),
                reason: MissReason::TASK_NOT_ELIGIBLE.as_str().to_owned(),
            });
        }
        descriptor.validate()?;
        Ok(Self { descriptor, grant })
    }

    /// Declared cache-key inputs.
    #[must_use]
    pub fn descriptor(&self) -> &CachedTaskDescriptor {
        &self.descriptor
    }

    /// Qualification grant backing this request.
    #[must_use]
    pub fn grant(&self) -> &ReuseGrant {
        &self.grant
    }

    /// Fixed `mise run --task-cache` argv for this qualified request.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] for bad task names and
    /// [`MiseError::ArtifactEscapesRoot`] for task files outside runner temp.
    pub fn run_argv(&self, task: &str, file: &str) -> Result<Vec<String>, MiseError> {
        qualified_task_run_argv(
            self.grant.task(),
            false,
            false,
            false,
            self.grant.mode(),
            task,
            file,
        )
    }
}

/// Task cache-key inputs digest, derived only under a matching grant
/// (REUSE-3). Mise is the sole authority for task keys; unqualified and
/// release tasks cannot reach derivation because they cannot mint a grant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCacheKey {
    /// Deterministic digest over the declared inputs.
    digest: String,
}

impl TaskCacheKey {
    /// Derive the key-inputs digest; the grant must name the descriptor task.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] for a mismatched grant and
    /// [`MiseError::Contract`] when canonical serialization fails.
    pub fn derive(
        grant: &ReuseGrant,
        descriptor: &CachedTaskDescriptor,
    ) -> Result<Self, MiseError> {
        if grant.task() != descriptor.task_name {
            return Err(MiseError::CacheNotEligible {
                task: descriptor.task_name.clone(),
                reason: MissReason::TASK_NOT_ELIGIBLE.as_str().to_owned(),
            });
        }
        Ok(Self {
            digest: descriptor.cache_inputs_digest()?,
        })
    }

    /// Deterministic `b3-` digest over the declared inputs.
    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Reuse plan: reuse under a grant, or execute with a fallback reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReusePlan {
    /// Reuse the cached result under this grant.
    Reuse(ReuseGrant),
    /// Execute without reuse, reporting this fallback.
    Execute(ReuseFallback),
}

/// Decide reuse for one qualification: missing tools, release paths, and
/// unqualified tasks execute without reuse and report their reason.
#[must_use]
pub fn plan_reuse(
    availability: ToolAvailability,
    qualification: &ReuseQualification,
    mode: TaskCacheMode,
) -> ReusePlan {
    match availability {
        ToolAvailability::Ready => {}
        ToolAvailability::Unqualified => {
            return ReusePlan::Execute(ReuseFallback::execute_with(MissReason::FORCED_UNCACHED));
        }
        ToolAvailability::Missing => {
            return ReusePlan::Execute(ReuseFallback::execute_with(MissReason::CACHE_UNAVAILABLE));
        }
    }
    match qualification.check(mode) {
        Ok(grant) => ReusePlan::Reuse(grant),
        Err(error) => {
            debug_assert!(
                !is_cancel_or_timeout(&error),
                "pure qualification never cancels"
            );
            let reason = fallback_for_error(&error).unwrap_or(MissReason::FORCED_UNCACHED);
            ReusePlan::Execute(ReuseFallback::execute_with(reason))
        }
    }
}

/// Opaque local-artifact transport rooted at `task-artifacts/v2` (REUSE-4).
///
/// Paths only: the transport resolves names under its root and never
/// inspects, lists, or parses cached bytes. Opening requires Gate-6 fixture
/// evidence and a reuse mode; `Off` never opens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskArtifactTransport {
    /// Artifact root (`<parent>/task-artifacts/v2`).
    root: PathBuf,
    /// Mode the transport was opened for.
    mode: TaskCacheMode,
}

impl TaskArtifactTransport {
    /// Open the transport under `parent` for one fixture and mode.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::CacheNotEligible`] for `Off` mode.
    pub fn open(
        fixture: &Gate6Fixture,
        mode: TaskCacheMode,
        parent: &Path,
    ) -> Result<Self, MiseError> {
        debug_assert!(Gate6Fixture::new(fixture.id()).is_ok());
        if mode == TaskCacheMode::Off {
            return Err(MiseError::CacheNotEligible {
                task: "task-artifacts".to_owned(),
                reason: MissReason::FORCED_UNCACHED.as_str().to_owned(),
            });
        }
        let Some(root) = resolve_task_artifact_dir(Some(parent), None) else {
            return Err(MiseError::ArtifactEscapesRoot {
                path: parent.to_string_lossy().into_owned(),
            });
        };
        Ok(Self { root, mode })
    }

    /// Artifact root (`<parent>/task-artifacts/v2`).
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Mode the transport was opened for.
    #[must_use]
    pub fn mode(&self) -> TaskCacheMode {
        self.mode
    }

    /// Resolve one opaque artifact name under the root, refusing escapes.
    ///
    /// # Errors
    ///
    /// Returns [`MiseError::ArtifactEscapesRoot`] for absolute, empty, or
    /// escaping names.
    pub fn resolve(&self, name: &str) -> Result<PathBuf, MiseError> {
        artifact_path(&self.root, name)
    }
}
