use std::path::{Path, PathBuf};

use crate::cache::{
    CachedTaskDescriptor, TaskCacheMode, artifact_path, qualify_reuse, resolve_task_artifact_dir,
};
use crate::command::is_cancel_or_timeout;
use crate::error::MiseError;
use crate::gate6::{Gate6Fixture, qualified_task_run_argv};
use crate::restore::{MissReason, ReuseFallback, ToolAvailability, fallback_for_error};

const RELEASE_EVENT: &str = "release";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReuseSignal {
    Network,
    Clock,
    Random,
    UndeclaredState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReuseQualification {
    kind: String,
    signals: [bool; 4],
    event: String,
}

impl ReuseQualification {
    #[must_use]
    pub fn new(kind: &str, event: &str) -> Self {
        Self {
            kind: kind.to_owned(),
            signals: [false; 4],
            event: event.to_owned(),
        }
    }

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

    #[must_use]
    pub fn kind(&self) -> &str {
        &self.kind
    }

    #[must_use]
    pub fn event(&self) -> &str {
        &self.event
    }

    #[must_use]
    pub fn always_run(&self) -> bool {
        matches!(
            self.kind.as_str(),
            "publish" | "deploy" | "notify" | "service"
        ) || self.signals.contains(&true)
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReuseGrant {
    task: String,
    mode: TaskCacheMode,
}

impl ReuseGrant {
    #[must_use]
    pub fn task(&self) -> &str {
        &self.task
    }

    #[must_use]
    pub fn mode(&self) -> TaskCacheMode {
        self.mode
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskReuseRequest {
    descriptor: CachedTaskDescriptor,
    grant: ReuseGrant,
}

impl TaskReuseRequest {
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

    #[must_use]
    pub fn descriptor(&self) -> &CachedTaskDescriptor {
        &self.descriptor
    }

    #[must_use]
    pub fn grant(&self) -> &ReuseGrant {
        &self.grant
    }

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCacheKey {
    digest: String,
}

impl TaskCacheKey {
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

    #[must_use]
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReusePlan {
    Reuse(ReuseGrant),
    Execute(ReuseFallback),
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskArtifactTransport {
    root: PathBuf,
    mode: TaskCacheMode,
}

impl TaskArtifactTransport {
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

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    #[must_use]
    pub fn mode(&self) -> TaskCacheMode {
        self.mode
    }

    pub fn resolve(&self, name: &str) -> Result<PathBuf, MiseError> {
        artifact_path(&self.root, name)
    }
}
