// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

fn test_git() -> &'static crate::crate_release_git::ReleaseGit {
    static GIT: std::sync::OnceLock<crate::crate_release_git::ReleaseGit> =
        std::sync::OnceLock::new();
    GIT.get_or_init(crate::crate_release_git::ReleaseGit::host_fixture)
}

use crate as velnor_actions_orchestrator;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::{
    cell::{Cell, RefCell},
    fs,
    path::Path,
    process::Command,
};
use velnor_actions_contract::config::CrateReleaseIntent;
use velnor_actions_orchestrator::{
    OrchestratorError,
    crate_release_controller::{
        CrateReleaseAction, CrateReleaseRepairWrite,
        prepare_crate_release as raw_prepare_crate_release, reconcile_crate_release,
        repair_crate_release, verify_crate_release_admission,
    },
    crate_release_github::{GitHubAdmissionRead, GitHubAdmissionRequest},
    crate_release_registry::{CrateRegistryRead, CrateRegistryResponse},
};

#[cfg(unix)]
#[path = "crate_release_coordinator_rehearsal.rs"]
mod coordinator_rehearsal;

#[path = "crate_release_controller_fixtures_github.rs"]
mod fixtures_github;
use fixtures_github::{
    Github, RefCreatedHook, fixture_no_baseline, fixture_verify_release_package,
    prepare_crate_release,
};
#[path = "crate_release_controller_fixtures_registry.rs"]
mod fixtures_registry;
use fixtures_registry::{
    NewRegistry, Registry, exercise_repair, fixture_archive, git, make_fixture,
};
#[path = "crate_release_controller_observations_setup.rs"]
mod observations_setup;
#[path = "crate_release_controller_owner_identity.rs"]
mod owner_identity;
#[path = "crate_release_controller_reconcile_repairs.rs"]
mod reconcile_repairs;
#[path = "crate_release_controller_recovery.rs"]
mod recovery;
use observations_setup::{JournalRegistryFake, err};
#[path = "crate_release_controller_observation_validation.rs"]
mod observation_validation;
use observation_validation::{
    fixture_journal_plan, reject_observed_approval_revocation, reject_tampered_source,
    reject_wrong_authentication, test_hex, verify_metadata_observations, verify_upload_result,
};
#[path = "crate_release_controller_prior_observation.rs"]
mod prior_observation;
use prior_observation::verify_reconciled_candidate;
#[path = "crate_release_controller_archive_preflight.rs"]
mod archive_preflight;
#[path = "crate_release_controller_upload_authorization.rs"]
mod upload_authorization;
#[path = "crate_release_controller_workspace_observation.rs"]
mod workspace_observation;
pub(super) use workspace_observation::configure_test_registry;
use workspace_observation::{
    FIXTURE_SPARSE_INDEX, WorkspaceGithub, WorkspaceRegistry, make_workspace_order_fixture,
};
#[path = "crate_release_controller_admission_fixtures.rs"]
mod admission_fixtures;
use admission_fixtures::{SparseFixture, reject_invalid_metadata_prerequisites};
#[path = "crate_release_controller_coordinator_completion.rs"]
mod coordinator_completion;
#[path = "crate_release_controller_coordinator_recovery.rs"]
mod coordinator_recovery;
#[path = "crate_release_controller_metadata_preflight.rs"]
mod metadata_preflight;
#[path = "crate_release_controller_metadata_repair.rs"]
mod metadata_repair;
use coordinator_recovery::{MixedGithub, MixedRegistry, reject_bad_completion_evidence};
#[path = "crate_release_controller_mixed_workspace.rs"]
mod mixed_workspace;
use mixed_workspace::set_workspace_journal_packages;

#[path = "crate_release_controller_historical_fixtures.rs"]
mod historical_fixtures;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use historical_fixtures::assert_two_link_recovery;
use historical_fixtures::{
    HistoricalAuthority, commit_history_intent, fixture_upload_intention, initialize_history_repo,
};
#[path = "crate_release_controller_historical_authority.rs"]
mod historical_authority;
#[path = "crate_release_controller_new_crate_ownership.rs"]
mod new_crate_ownership;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
#[path = "crate_release_controller_signed_history.rs"]
pub(crate) mod signed_history;
use new_crate_ownership::journal_existing_owners;
#[path = "crate_release_controller_creation_recovery.rs"]
mod creation_recovery;
#[path = "crate_release_controller_workspace_creation.rs"]
mod workspace_creation;
use creation_recovery::{CreationWorkspaceGithub, CreationWorkspaceRegistry};
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
use creation_recovery::{
    reject_untrusted_prior_creation, verify_created_crate_recovers_without_reupload,
};

#[path = "crate_release_controller_workspace_publication.rs"]
mod workspace_publication;

#[path = "crate_release_controller_frozen_completion.rs"]
mod frozen_completion;
