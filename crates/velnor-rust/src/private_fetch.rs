// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{
    Access, CacheKey, CargoCommand, DenyCheck, Gate, GateNeed, HelperCommand, Job, PermissionScope,
    RepoFacts, RepoPath, RootSlug, Step, StepKind, TokenMint, When,
};

use crate::plan::step;

// helper-gate: first in tools-v0.1.111 via 00cab67428
pub const TOKEN_FIRST_HELPER: (u64, u64, u64) = (0, 1, 111);
const HELPER_MINT: (u64, u64, u64) = (0, 1, 112);

pub(crate) fn tokens_first(facts: &RepoFacts) -> bool {
    facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(TOKEN_FIRST_HELPER))
}

pub(crate) fn token_mint(facts: &RepoFacts) -> TokenMint {
    if facts
        .helper
        .as_ref()
        .is_some_and(|helper| helper.version().at_least(HELPER_MINT))
    {
        TokenMint::Helper
    } else {
        TokenMint::Action
    }
}

pub(crate) fn after_trusted_setup(steps: &[Step]) -> usize {
    steps
        .iter()
        .position(|step| {
            !matches!(
                step.kind,
                StepKind::Checkout { .. }
                    | StepKind::InstallHelper(_)
                    | StepKind::Helper(HelperCommand::Plan)
                    | StepKind::PublicSources { .. }
                    | StepKind::RepositoryPrivacy
                    | StepKind::PrivateCheck { .. }
                    | StepKind::ReleaseAssets { .. }
                    | StepKind::MbxRemote { .. }
                    | StepKind::MbxPin
                    | StepKind::MbxPrivate
                    | StepKind::MbxStore
                    | StepKind::MbxSeed
                    | StepKind::CratesMirror(_)
            )
        })
        .unwrap_or(steps.len())
}

pub(crate) fn insert(facts: &RepoFacts, jobs: &mut [Job]) {
    let Some(sources) = facts.config.private_sources.as_ref() else {
        return;
    };
    let early = tokens_first(facts);
    for job in jobs {
        let Some(first) = job.steps.iter().position(compiles) else {
            continue;
        };
        let first = job.steps[..first]
            .iter()
            .position(|step| matches!(step.kind, StepKind::TestSetup { .. }))
            .unwrap_or(first);
        let mut manifests: Vec<RepoPath> = Vec::new();
        for step in job.steps.iter().filter(|step| compiles(step)) {
            let manifest = manifest(step);
            if !manifests.contains(&manifest) {
                manifests.push(manifest);
            }
        }
        let cached: Vec<RootSlug> = job.steps[..first]
            .iter()
            .filter_map(|step| match &step.kind {
                StepKind::CacheRestore(CacheKey::Sources { lane, .. }) => Some(lane.slug().clone()),
                _ => None,
            })
            .collect();
        let when = status(job.steps.iter().filter(|step| compiles(step)));
        let gate = shared_gate(job.steps.iter().filter(|step| runs_repository_code(step)));
        let mut kinds = advisory_fetch(&job.steps);
        if early {
            job.steps.splice(
                first..first,
                kinds.into_iter().map(|kind| Step {
                    gate: gate.clone(),
                    when,
                    ..step(kind)
                }),
            );
            kinds = Vec::new();
        }
        if !job
            .steps
            .iter()
            .any(|step| matches!(step.kind, StepKind::PrivateCheck { .. }))
        {
            kinds.push(StepKind::PrivateCheck {
                cached: if early { Vec::new() } else { cached.clone() },
            });
        }
        kinds.extend([
            StepKind::PrivateToken {
                sources: sources.clone(),
                cached: cached.clone(),
                mint: token_mint(facts),
            },
            StepKind::PrivateFetch { manifests, cached },
        ]);
        if early {
            let at = after_trusted_setup(&job.steps[..first]);
            let gate = job.steps.get(at).and_then(|step| step.gate.clone());
            let restores = hoist_sources_restores(&mut job.steps, at, first);
            let mut phase: Vec<Step> = kinds
                .into_iter()
                .map(|kind| Step {
                    gate: gate.clone(),
                    ..step(kind)
                })
                .collect();
            let token = phase.len() - 2;
            phase.splice(token..token, restores);
            job.steps.splice(at..at, phase);
        } else {
            let phase = kinds.into_iter().map(|kind| Step {
                gate: gate.clone(),
                when,
                ..step(kind)
            });
            job.steps.splice(first..first, phase);
        }
        job.permissions
            .0
            .insert(PermissionScope::PullRequests, Access::Read);
    }
}

fn hoist_sources_restores(steps: &mut Vec<Step>, from: usize, to: usize) -> Vec<Step> {
    let (mut index, mut end, mut hoisted) = (from, to, Vec::new());
    while index < end {
        if matches!(
            steps[index].kind,
            StepKind::SourcesLocks(_) | StepKind::CacheRestore(CacheKey::Sources { .. })
        ) {
            hoisted.push(steps.remove(index));
            end -= 1;
        } else {
            index += 1;
        }
    }
    hoisted
}

/// The phase carries the status gate of the compile steps it serves. Under the implicit
/// `success()` an earlier failure (a Format diff) would skip the token and the fetch while the
/// `!cancelled()` compile steps still ran, offline, and buried the real error under a
/// "you are in the offline mode" failure. The order is unchanged, so the token still comes
/// before any repository code and is never in the environment of a compile step.
pub(crate) fn status<'a>(mut compiling: impl Iterator<Item = &'a Step>) -> When {
    if compiling.any(|step| step.when == When::NotCancelled) {
        When::NotCancelled
    } else {
        When::Success
    }
}

/// cargo-deny runs `--offline` once private sources are configured, so the advisory DB
/// must be fetched first. The DB is public: this step gets no token and no private config.
fn advisory_fetch(steps: &[Step]) -> Vec<StepKind> {
    let mut configs: Vec<Option<RepoPath>> = Vec::new();
    let denies = steps.iter().filter_map(|step| match &step.kind {
        StepKind::Cargo(CargoCommand::Deny { config, checks, .. })
            if checks
                .as_ref()
                .is_none_or(|checks| checks.contains(&DenyCheck::Advisories)) =>
        {
            Some(config.clone())
        }
        _ => None,
    });
    for config in denies {
        if !configs.contains(&config) {
            configs.push(config);
        }
    }
    if configs.is_empty() {
        Vec::new()
    } else {
        vec![StepKind::AdvisoryFetch { configs }]
    }
}

pub(crate) fn shared_gate<'a>(steps: impl Iterator<Item = &'a Step>) -> Option<Gate> {
    let mut gates = steps.map(|step| step.gate.as_ref());
    let first = gates.next()??;
    let mut shared = first.clone();
    for gate in gates {
        let gate = gate?;
        if *gate == shared {
            continue;
        }
        let selected = |gate: &Gate| {
            gate.slug == shared.slug
                && gate.from == shared.from
                && matches!(
                    gate.need,
                    GateNeed::Selected
                        | GateNeed::Doctests
                        | GateNeed::Docs
                        | GateNeed::PackageArgs
                )
        };
        if !(selected(gate) && selected(&shared)) {
            return None;
        }
        shared = Gate {
            need: GateNeed::Selected,
            packages: Vec::new(),
            unselected: false,
            ..shared
        };
    }
    Some(shared)
}

pub(crate) fn any_gate(first: Option<&Gate>, second: Option<&Gate>) -> Option<Gate> {
    let (first, second) = (first?, second?);
    if first == second {
        return Some(first.clone());
    }
    let selects = |gate: &Gate| {
        matches!(
            gate.need,
            GateNeed::Selected | GateNeed::Doctests | GateNeed::Docs | GateNeed::PackageArgs
        )
    };
    if first.from != second.from || !selects(first) || !selects(second) {
        return None;
    }
    let mut roots: Vec<RootSlug> = Vec::new();
    for slug in first.roots().chain(second.roots()) {
        if !roots.contains(slug) {
            roots.push(slug.clone());
        }
    }
    let mut rest = roots.into_iter();
    Some(Gate {
        slug: rest.next()?,
        need: GateNeed::Selected,
        packages: Vec::new(),
        unselected: false,
        also: rest.collect(),
        ..first.clone()
    })
}

fn manifest(step: &Step) -> RepoPath {
    if let StepKind::Cargo(CargoCommand::Deny { manifest, .. }) = &step.kind {
        return manifest.clone();
    }
    step.directory.as_ref().map_or_else(
        || RepoPath::from_static("Cargo.toml"),
        |directory| directory.child("Cargo.toml"),
    )
}

fn runs_repository_code(step: &Step) -> bool {
    compiles(step) || matches!(step.kind, StepKind::TestSetup { .. })
}

pub(crate) fn compiles(step: &Step) -> bool {
    match &step.kind {
        StepKind::Cargo(command) => !matches!(
            command,
            CargoCommand::FmtCheck | CargoCommand::Machete | CargoCommand::NextestRunArchive { .. }
        ),
        StepKind::GeneratorCheck { from_source } => *from_source,
        _ => false,
    }
}
