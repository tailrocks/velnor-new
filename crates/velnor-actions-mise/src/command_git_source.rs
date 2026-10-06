//! Source bindings and the sole private-directory lifetime for Discovery Git.
use std::io;
use std::path::Path;

use super::IsolatedCommand;
use super::index::PrivateGitIndex;
use super::index::repository::RepositoryContext;
use super::index::repository_format::{CommonRepository, IndexObjectFormat};
use super::private_root::{BoundCwd, PrivateGitRoot};
use super::read::ReadInvocation;

#[derive(Debug)]
pub(super) enum PreparedSource {
    Indexed(PrivateGitIndex),
    Unindexed {
        root: PrivateGitRoot,
        cwd: BoundCwd,
        source: Option<RepositoryContext>,
        common: Option<CommonRepository>,
    },
}

impl PreparedSource {
    pub(super) fn prepare(owner: &IsolatedCommand, read: &ReadInvocation) -> io::Result<Self> {
        if read.uses_index {
            return PrivateGitIndex::prepare(owner.cwd.as_ref()).map(Self::Indexed);
        }
        let cwd = BoundCwd::from_owner(owner.cwd.as_ref())?;
        let source = match RepositoryContext::resolve(owner.cwd.as_ref()) {
            Ok(source) => Some(source),
            Err(error) if read.allows_non_repo && repository_absent(&error) => None,
            Err(error) => return Err(error),
        };
        let common = source.as_ref().map(CommonRepository::capture).transpose()?;
        let root = PrivateGitRoot::create_no_index(&cwd, source.as_ref(), common.as_ref())?;
        Ok(Self::Unindexed {
            root,
            cwd,
            source,
            common,
        })
    }

    pub(super) fn root(&self) -> &PrivateGitRoot {
        match self {
            Self::Indexed(index) => index.root(),
            Self::Unindexed { root, .. } => root,
        }
    }

    pub(super) fn cwd(&self) -> &Path {
        match self {
            Self::Indexed(index) => index.context().cwd(),
            Self::Unindexed { cwd, .. } => cwd.path(),
        }
    }

    pub(super) fn bound_cwd(&self) -> &BoundCwd {
        match self {
            Self::Indexed(index) => index.bound_cwd(),
            Self::Unindexed { cwd, .. } => cwd,
        }
    }

    pub(super) fn repository(&self) -> Option<&RepositoryContext> {
        match self {
            Self::Indexed(index) => Some(index.context()),
            Self::Unindexed { source, .. } => source.as_ref(),
        }
    }

    pub(super) fn common(&self) -> Option<&Path> {
        match self {
            Self::Indexed(index) => Some(index.common_path()),
            Self::Unindexed { common, .. } => common.as_ref().map(CommonRepository::path),
        }
    }

    pub(super) fn common_repository(&self) -> Option<&CommonRepository> {
        match self {
            Self::Indexed(index) => Some(index.common_repository()),
            Self::Unindexed { common, .. } => common.as_ref(),
        }
    }

    pub(super) fn index(&self) -> Option<&Path> {
        match self {
            Self::Indexed(index) => index.expected_format().map(|_| index.path()),
            Self::Unindexed { .. } => None,
        }
    }

    pub(super) fn expected_format(&self) -> Option<IndexObjectFormat> {
        match self {
            Self::Indexed(index) => index.expected_format(),
            Self::Unindexed { .. } => None,
        }
    }

    pub(super) fn verify_binding(&self) -> io::Result<()> {
        self.root().verify_binding()?;
        match self {
            Self::Indexed(index) => index.verify_source(),
            Self::Unindexed {
                cwd,
                source,
                common,
                ..
            } => {
                cwd.verify_binding()?;
                if let Some(source) = source {
                    source.verify_binding()?;
                }
                if let Some(common) = common {
                    common.verify()?;
                }
                Ok(())
            }
        }
    }

    pub(super) fn finish(self) -> io::Result<()> {
        match self {
            Self::Indexed(index) => index.finish(),
            Self::Unindexed { root, .. } => root.finish(),
        }
    }

    pub(super) fn retain_unreaped(self) {
        match self {
            Self::Indexed(index) => index.retain_unreaped(),
            Self::Unindexed { root, .. } => root.retain_unreaped(),
        }
    }
}

fn repository_absent(error: &io::Error) -> bool {
    error.kind() == io::ErrorKind::NotFound
        && error.to_string() == "private_git_repository:repository_not_found"
}
