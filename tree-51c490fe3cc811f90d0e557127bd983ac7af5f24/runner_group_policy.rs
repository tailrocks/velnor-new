//! Bounded readers for GitHub organization and enterprise runner-group policies.

mod inventory;
mod model;
mod pages;
mod reader;

pub use inventory::{find_enterprise_runner_group_policy, find_organization_runner_group_policy};
pub use model::{
    ActionsRunnerGroupPolicy, RunnerGroupAccess, RunnerGroupPolicySnapshot, RunnerGroupScope,
    SelectedOrganization, SelectedRepository,
};
pub use reader::{get_enterprise_runner_group_policy, get_organization_runner_group_policy};
