// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::{
    GeneratedWorkflowKind, NativeAuthorityError, NativeAuthorityInput, invalid, validate_root,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub enum GeneratedSourceFact {
    BunMember {
        name: String,
        workspace_root: String,
        scripts: Vec<String>,
        private_assets: bool,
    },
    Dbt {
        root: String,
    },
    Ansible {
        root: String,
    },
}

#[derive(Debug, Clone)]
pub(in crate::native_authority) struct GeneratedOwner {
    pub(in crate::native_authority) kind: GeneratedWorkflowKind,
    pub(in crate::native_authority) member: Option<String>,
    pub(in crate::native_authority) suite: Option<String>,
    pub(in crate::native_authority) jobs: BTreeSet<String>,
    pub(in crate::native_authority) required: BTreeSet<String>,
    pub(in crate::native_authority) assets: Option<String>,
    pub(in crate::native_authority) report: bool,
}

pub(in crate::native_authority) fn inventory(
    input: &NativeAuthorityInput<'_>,
) -> Result<BTreeMap<String, GeneratedOwner>, NativeAuthorityError> {
    let mut owners = BTreeMap::new();
    for fact in input.generated_sources {
        match fact {
            GeneratedSourceFact::BunMember {
                name,
                workspace_root,
                scripts,
                private_assets,
            } => bun(
                input,
                &mut owners,
                name,
                workspace_root,
                scripts,
                *private_assets,
            )?,
            GeneratedSourceFact::Dbt { root } => {
                let config = input
                    .config
                    .stacks
                    .dbt
                    .as_ref()
                    .ok_or_else(|| invalid("native_dbt_source_without_config"))?;
                admit_root(root, &config.projects)?;
                let id = format!(
                    "{}{}",
                    velnor_actions_contract::config::DBT_JOB_PREFIX,
                    velnor_actions_dbt::slug(root)
                );
                insert(
                    &mut owners,
                    OwnerSpec {
                        id,
                        kind: GeneratedWorkflowKind::Dbt,
                        member: None,
                        suite: None,
                        required: true,
                        report: false,
                        assets: None,
                    },
                )?;
            }
            GeneratedSourceFact::Ansible { root } => {
                let config = input
                    .config
                    .stacks
                    .ansible
                    .as_ref()
                    .ok_or_else(|| invalid("native_ansible_source_without_config"))?;
                admit_root(root, &config.roots)?;
                let id = format!(
                    "{}{}",
                    velnor_actions_contract::config::ANSIBLE_JOB_PREFIX,
                    velnor_actions_contract::slugify_segment(root)
                );
                insert(
                    &mut owners,
                    OwnerSpec {
                        id,
                        kind: GeneratedWorkflowKind::Ansible,
                        member: None,
                        suite: None,
                        required: true,
                        report: false,
                        assets: None,
                    },
                )?;
            }
        }
    }
    if let Some(config) = &input.config.stacks.bun {
        for member in config.members.keys() {
            if !input.generated_sources.iter().any(|source| matches!(source, GeneratedSourceFact::BunMember { name, .. } if name == member)) { return Err(invalid("native_bun_configured_member_missing")); }
        }
    }
    complete_roots(input)?;
    Ok(owners)
}

fn bun(
    input: &NativeAuthorityInput<'_>,
    owners: &mut BTreeMap<String, GeneratedOwner>,
    name: &str,
    root: &str,
    scripts: &[String],
    private_assets: bool,
) -> Result<(), NativeAuthorityError> {
    let config = input
        .config
        .stacks
        .bun
        .as_ref()
        .ok_or_else(|| invalid("native_bun_source_without_config"))?;
    admit_root(root, &config.roots)?;
    let member = velnor_actions_bun::Member {
        name: name.to_owned(),
        root: root.to_owned(),
        key: String::new(),
        dir: root.to_owned(),
        scripts: scripts.iter().cloned().collect(),
        commands: BTreeMap::new(),
        depends_on: BTreeSet::new(),
        playwright: None,
    };
    let id = velnor_actions_bun::job_id(&member);
    if velnor_actions_bun::member_job_scripts(&member, config)
        .map_err(|error| invalid(error.code()))?
        .is_some()
    {
        insert(
            owners,
            OwnerSpec {
                id: id.clone(),
                kind: GeneratedWorkflowKind::BunMember,
                member: Some(name.to_owned()),
                suite: None,
                required: true,
                report: false,
                assets: None,
            },
        )?;
    }
    let suites = config.members.get(name).map(|member| &member.suites);
    let assets =
        private_assets.then(|| format!("{}{}", id, crate::verification_jobs::BUN_ASSETS_SUFFIX));
    if private_assets {
        if input.config.private_sources.is_none() || suites.is_none_or(BTreeMap::is_empty) {
            return Err(invalid("native_bun_assets_without_sources"));
        }
        insert_assets(
            owners,
            assets
                .clone()
                .ok_or_else(|| invalid("native_bun_assets_identity"))?,
            name,
        )?;
    }
    for (suite_id, suite) in suites.into_iter().flatten() {
        let id = velnor_actions_bun::suite_job_id(&member, suite_id);
        let report = suite.playwright.as_ref().is_some_and(|playwright| {
            playwright.report == velnor_actions_contract::config::BunSuiteReport::Merge
        });
        insert(
            owners,
            OwnerSpec {
                id,
                kind: GeneratedWorkflowKind::BunSuite,
                member: Some(name.to_owned()),
                suite: Some(suite_id.clone()),
                required: suite.required,
                report,
                assets: assets.clone(),
            },
        )?;
    }
    Ok(())
}

fn insert_assets(
    owners: &mut BTreeMap<String, GeneratedOwner>,
    id: String,
    name: &str,
) -> Result<(), NativeAuthorityError> {
    insert(
        owners,
        OwnerSpec {
            id,
            kind: GeneratedWorkflowKind::BunAssets,
            member: Some(name.to_owned()),
            suite: None,
            required: false,
            report: false,
            assets: None,
        },
    )
}

fn admit_root(
    root: &str,
    roots: &[velnor_actions_contract::Utf8RepoRelDir],
) -> Result<(), NativeAuthorityError> {
    validate_root(if root == "." { "" } else { root })?;
    if !roots.iter().any(|configured| {
        configured.as_str() == root || configured.as_str() == "." && root.is_empty()
    }) {
        return Err(invalid("native_generated_root_membership"));
    }
    Ok(())
}

struct OwnerSpec {
    id: String,
    kind: GeneratedWorkflowKind,
    member: Option<String>,
    suite: Option<String>,
    required: bool,
    report: bool,
    assets: Option<String>,
}

fn insert(
    owners: &mut BTreeMap<String, GeneratedOwner>,
    spec: OwnerSpec,
) -> Result<(), NativeAuthorityError> {
    let OwnerSpec {
        id,
        kind,
        member,
        suite,
        required,
        report,
        assets,
    } = spec;
    let mut jobs = BTreeSet::from([id.clone()]);
    if report {
        jobs.insert(format!(
            "{id}{}",
            crate::verification_jobs::BUN_REPORT_SUFFIX
        ));
    }
    let required = if required {
        BTreeSet::from([id.clone()])
    } else {
        BTreeSet::new()
    };
    if owners
        .insert(
            id,
            GeneratedOwner {
                kind,
                member,
                suite,
                jobs,
                required,
                assets,
                report,
            },
        )
        .is_some()
    {
        return Err(invalid("native_generated_owner_duplicate"));
    }
    Ok(())
}

fn complete_roots(input: &NativeAuthorityInput<'_>) -> Result<(), NativeAuthorityError> {
    let normalized = |root: &str| {
        if root == "." {
            String::new()
        } else {
            root.to_owned()
        }
    };
    let mut bun = BTreeSet::new();
    let mut dbt = BTreeSet::new();
    let mut ansible = BTreeSet::new();
    for source in input.generated_sources {
        match source {
            GeneratedSourceFact::BunMember { workspace_root, .. } => {
                bun.insert(normalized(workspace_root));
            }
            GeneratedSourceFact::Dbt { root } => {
                dbt.insert(normalized(root));
            }
            GeneratedSourceFact::Ansible { root } => {
                ansible.insert(normalized(root));
            }
        }
    }
    let expected_bun = input
        .config
        .stacks
        .bun
        .as_ref()
        .map(|config| {
            config
                .roots
                .iter()
                .map(|root| normalized(root.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let expected_dbt = input
        .config
        .stacks
        .dbt
        .as_ref()
        .map(|config| {
            config
                .projects
                .iter()
                .map(|root| normalized(root.as_str()))
                .collect()
        })
        .unwrap_or_default();
    let expected_ansible = input
        .config
        .stacks
        .ansible
        .as_ref()
        .map(|config| {
            config
                .roots
                .iter()
                .map(|root| normalized(root.as_str()))
                .collect()
        })
        .unwrap_or_default();
    if bun != expected_bun || dbt != expected_dbt || ansible != expected_ansible {
        return Err(invalid("native_generated_source_inventory_incomplete"));
    }
    Ok(())
}
