//! Exact compiled per-root ownership prerequisite; no repository shell authority.
use crate::RenderError;
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, Job, SourceBoundOperation, SourceProducer, SourceProducerRole, StepKind,
    ToolCacheDomain,
};

pub(super) fn validate(
    job: &Job,
    meta: &SourceProducer,
    records: &[CompiledSourceHelper],
) -> Result<(), RenderError> {
    let owners: Vec<_> = job
        .steps
        .iter()
        .enumerate()
        .filter(|(_, step)| {
            matches!(&step.kind, StepKind::SourceBoundHelper { invocation, .. }
            if invocation.descriptor().operation() == SourceBoundOperation::TofuRootOwnership)
        })
        .collect();
    if meta.role != SourceProducerRole::Tofu {
        return if owners.is_empty() {
            Ok(())
        } else {
            Err(invalid())
        };
    }
    let [(at, step)] = owners.as_slice() else {
        return Err(invalid());
    };
    let StepKind::SourceBoundHelper { invocation, env } = &step.kind else {
        return Err(invalid());
    };
    let [key] = invocation.args() else {
        return Err(invalid());
    };
    let Some(hex) = key.strip_prefix("dir-") else {
        return Err(invalid());
    };
    if hex.len() % 2 != 0
        || !hex
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'a'..=b'f'))
    {
        return Err(invalid());
    }
    let locator =
        velnor_actions_contract::digest_b3(format!("tofu-private-root-v1\n{key}").as_bytes());
    let home = format!("${{{{ runner.temp }}}}/velnor/tofu-provider-producer/{locator}/home");
    let expected = environment(&home);
    let restore = job
        .steps
        .iter()
        .position(|step| step.id.as_ref() == Some(&meta.restore_step));
    let proof_home = job.steps.iter().find_map(|step| {
        if step.id.as_ref() != Some(&meta.verification_step) {
            return None;
        }
        match &step.kind {
            StepKind::SourceBoundHelper { env, .. } => env.get("HOME"),
            _ => None,
        }
    });
    if *at != 4
        || restore.is_none_or(|restore| restore <= *at)
        || step
            .id
            .as_ref()
            .is_none_or(|id| id.as_str() != "velnor-tofu-root-ownership")
        || step.condition.is_some()
        || env != &expected
        || proof_home != Some(&home)
        || !invocation.installed_selectors().is_empty()
        || !records
            .iter()
            .any(|record| record.invocation() == invocation && record.environment() == env)
    {
        return Err(invalid());
    }
    Ok(())
}

fn invalid() -> RenderError {
    RenderError::InvalidWorkflow("source_producer_tofu_ownership_changed".to_owned())
}

fn environment(home: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("HOME".to_owned(), home.to_owned()),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        (
            "MISE_DATA_DIR".to_owned(),
            ToolCacheDomain::TofuBootstrap.root().to_owned(),
        ),
        ("MISE_NO_CONFIG".to_owned(), "1".to_owned()),
        ("MISE_NO_ENV".to_owned(), "1".to_owned()),
        ("MISE_NO_HOOKS".to_owned(), "1".to_owned()),
        ("MISE_LOCKFILE".to_owned(), "0".to_owned()),
        ("MISE_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("MISE_EXEC_AUTO_INSTALL".to_owned(), "false".to_owned()),
        ("RUSTUP_AUTO_INSTALL".to_owned(), "0".to_owned()),
    ])
}
