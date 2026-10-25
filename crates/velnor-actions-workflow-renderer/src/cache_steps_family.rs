use std::collections::BTreeMap;

use velnor_actions_contract::cachekey::MAX_CACHE_KEY_BYTES;
use velnor_actions_contract::workflow::cache_family::CacheFamily;
use velnor_actions_contract::workflow::cache_family_steps::{
    FAMILY_ADMITTED_OUTPUT, FAMILY_SAVE_OUTPUT, FAMILY_WRITER_ENV, FAMILY_WRITER_EXPR,
    family_key_expr, family_output_stem, family_restore_keys_expr, family_save_condition,
    family_size_step_id, is_stack_family,
};
use velnor_actions_contract::{Step, StepId, StepRole};

use crate::RenderError;
use crate::steps::{action_step_with_env, shell_step};

pub const FAMILY_KEYS_NAME: &str = "Compute family cache keys";
const DEFAULT_REF_ENV: &str = "VELNOR_DEFAULT_REF";
const DEFAULT_REF_EXPR: &str =
    "${{ format('refs/heads/{0}', github.event.repository.default_branch) }}";
const GUARD: &str = "case \"$RUNNER_TEMP\" in /*) ;; *) printf '%s\\n' 'RUNNER_TEMP must be absolute' >&2; exit 1 ;; esac";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FamilySnapshot {
    Files(Vec<String>),
    Lines(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FamilyCachePlan {
    pub family: CacheFamily,
    pub locator: Option<String>,
    pub snapshot: FamilySnapshot,
    pub compat: Vec<String>,
}

impl FamilyCachePlan {
    fn check(&self) -> Result<(), RenderError> {
        let ok = is_stack_family(self.family)
            && self.family.spec().is_per_root() == self.locator.is_some()
            && self.locator.as_deref().is_none_or(|locator| {
                !locator.is_empty()
                    && locator
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            })
            && self.compat.iter().all(|line| {
                line.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b" =_-.:${}".contains(&b))
            })
            && match &self.snapshot {
                FamilySnapshot::Files(files) => files.iter().all(|file| safe_relative(file)),
                FamilySnapshot::Lines(lines) => lines.iter().all(|line| {
                    line.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b" =_-.:/@+".contains(&b))
                }),
            };
        ok.then_some(()).ok_or_else(|| {
            RenderError::InvalidWorkflow(format!("family_cache_plan:{}", self.family.spec().layer))
        })
    }

    fn path_input(&self) -> String {
        self.family.spec().path_input(self.locator.as_deref())
    }

    fn shell_dir(&self) -> String {
        self.family
            .spec()
            .shell_paths(self.locator.as_deref())
            .join("")
    }
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && path.split('/').all(|part| !part.is_empty() && part != "..")
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'_' | b'-'))
}

fn key_commands(plan: &FamilyCachePlan) -> Vec<String> {
    let spec = plan.family.spec();
    let stem = family_output_stem(plan.family);
    let mut header = vec![
        format!("'family={}'", spec.layer),
        format!("'schema={}'", spec.schema),
    ];
    header.extend(
        spec.shell_paths(plan.locator.as_deref())
            .iter()
            .map(|p| format!("\"{p}\"")),
    );
    header.extend(plan.compat.iter().map(|line| format!("\"{line}\"")));
    let mut commands = vec![
        format!("printf '%s\\n' {} > \"$d/{stem}-compat\"", header.join(" ")),
        format!("cat \"$d/platform\" >> \"$d/{stem}-compat\""),
        format!("s256 \"$d/{stem}-compat\""),
        "compat=\"s256-$h\"".to_owned(),
        format!(": > \"$d/{stem}-snap\""),
    ];
    match &plan.snapshot {
        FamilySnapshot::Files(files) => {
            for file in files {
                commands.push(format!("f=\"$GITHUB_WORKSPACE/{file}\""));
                commands.push(format!("if [ -f \"$f\" ] && [ ! -L \"$f\" ]; then s256 \"$f\"; else h=absent; fi; printf '%s\\t%s\\n' '{file}' \"$h\" >> \"$d/{stem}-snap\""));
            }
        }
        FamilySnapshot::Lines(lines) => {
            for line in lines {
                commands.push(format!("printf '%s\\n' '{line}' >> \"$d/{stem}-snap\""));
            }
        }
    }
    commands.extend([
        format!("s256 \"$d/{stem}-snap\""),
        "snap=\"s256-$h\"".to_owned(),
        format!("trusted=\"{}trusted-$compat-\"", plan.family.key_prefix()),
        "key=\"$trusted$snap\"".to_owned(),
        format!("[ \"${{#key}}\" -le {MAX_CACHE_KEY_BYTES} ] || {{ printf 'cache key exceeds the GitHub cap: %s\\n' \"$key\" >&2; exit 1; }}"),
        format!("printf '%s\\n' \"{stem}_key=$key\" \"{stem}_restore_keys=$trusted\" >> \"$GITHUB_OUTPUT\""),
    ]);
    commands
}

pub fn family_keys_step(plans: &[FamilyCachePlan]) -> Result<Step, RenderError> {
    keys_step(plans, false)
}

pub fn family_keys_step_single_writer(plans: &[FamilyCachePlan]) -> Result<Step, RenderError> {
    keys_step(plans, true)
}

fn keys_step(plans: &[FamilyCachePlan], single_writer: bool) -> Result<Step, RenderError> {
    if plans.is_empty() {
        return Err(RenderError::InvalidWorkflow("family_keys_empty".to_owned()));
    }
    let mut script = vec![
        "set -eu".to_owned(),
        GUARD.to_owned(),
        "d=\"$RUNNER_TEMP/velnor/family-keys\"".to_owned(),
        "rm -rf \"$d\"".to_owned(),
        "mkdir -p \"$d\"".to_owned(),
        "s256() { sha256sum \"$1\" > \"$d/sum\"; h=; rest=; read -r h rest < \"$d/sum\" || [ -n \"$h\" ]; case \"$h\" in ''|*[!0-9a-f]*) printf '%s\\n' 'sha256sum returned no digest' >&2; exit 1 ;; esac; [ \"${#h}\" -eq 64 ] || exit 1; }".to_owned(),
        "uname -s > \"$d/platform\"".to_owned(),
        "uname -m >> \"$d/platform\"".to_owned(),
        format!("default_ref=\"${{{DEFAULT_REF_ENV}:-}}\""),
        "trust=none".to_owned(),
        "if [ \"${GITHUB_EVENT_NAME:-}\" = push ] && [ -n \"${default_ref#refs/heads/}\" ] && [ \"${GITHUB_REF:-}\" = \"$default_ref\" ] && [ \"${GITHUB_REF_PROTECTED:-}\" = true ]; then trust=trusted; fi".to_owned(),
        "save=false".to_owned(),
        "[ \"$trust\" != trusted ] || save=true".to_owned(),
    ];
    for plan in plans {
        plan.check()?;
        script.extend(key_commands(plan));
    }
    let mut env = BTreeMap::from([(DEFAULT_REF_ENV.to_owned(), DEFAULT_REF_EXPR.to_owned())]);
    if single_writer {
        script.push(format!(
            "case \"${{{FAMILY_WRITER_ENV}:-}}\" in 1) ;; *) save=false ;; esac"
        ));
        env.insert(FAMILY_WRITER_ENV.to_owned(), FAMILY_WRITER_EXPR.to_owned());
    }
    script.push(format!(
        "printf '%s\\n' \"{FAMILY_SAVE_OUTPUT}=$save\" >> \"$GITHUB_OUTPUT\""
    ));
    let mut step = shell_step(
        FAMILY_KEYS_NAME,
        vec!["sh".to_owned(), "-c".to_owned(), script.join("; ")],
        env,
    )?;
    step.id = Some(StepId::FamilyKeys);
    step.role = Some(StepRole::FamilyKeys);
    Ok(step)
}

pub fn family_restore_step(plan: &FamilyCachePlan) -> Result<Step, RenderError> {
    plan.check()?;
    let mut step = action_step_with_env(
        &format!("Restore {} cache", plan.family.spec().layer),
        crate::cache_steps::TOOLS_RESTORE_ACTION_USES,
        BTreeMap::from([
            ("key".to_owned(), family_key_expr(plan.family)),
            (
                "restore-keys".to_owned(),
                family_restore_keys_expr(plan.family),
            ),
            ("path".to_owned(), plan.path_input()),
        ]),
        BTreeMap::new(),
    )?;
    step.role = Some(StepRole::FamilyRestore);
    Ok(step)
}

pub fn family_size_step(plan: &FamilyCachePlan) -> Result<Step, RenderError> {
    plan.check()?;
    let ceiling = plan.family.spec().ceiling_bytes;
    let layer = plan.family.spec().layer;
    let script = [
        "set -eu".to_owned(),
        GUARD.to_owned(),
        format!("dir=\"{}\"", plan.shell_dir()),
        "out=${GITHUB_OUTPUT-}".to_owned(),
        format!("refuse() {{ if [ -n \"$out\" ]; then printf '%s\\n' '{FAMILY_ADMITTED_OUTPUT}=false' >> \"$out\"; fi; printf '%s\\n' \"::warning::$1: {layer} cache not saved\"; exit 0; }}"),
        "if [ ! -d \"$dir\" ] || [ -L \"$dir\" ]; then refuse missing_directory; fi".to_owned(),
        "mkdir -p \"$RUNNER_TEMP/velnor\"".to_owned(),
        "size=\"$RUNNER_TEMP/velnor/family-size\"".to_owned(),
        format!("if command -v zstd >/dev/null 2>&1; then tar -C \"${{dir%/*}}\" -cf - \"${{dir##*/}}\" | zstd -T0 -q | wc -c > \"$size\"; limit={ceiling}; else du -sk \"$dir\" > \"$size\"; limit={}; fi", ceiling / 1024),
        "n=; rest=; read -r n rest < \"$size\" || [ -n \"$n\" ]".to_owned(),
        "case \"$n\" in ''|*[!0-9]*) refuse size_unreadable ;; esac".to_owned(),
        "if [ \"$n\" -gt \"$limit\" ]; then refuse over_ceiling; fi".to_owned(),
        format!("if [ -n \"$out\" ]; then printf '%s\\n' '{FAMILY_ADMITTED_OUTPUT}=true' >> \"$out\"; fi"),
    ]
    .join("; ");
    let mut step = shell_step(
        &format!("Check {layer} cache size"),
        vec!["sh".to_owned(), "-c".to_owned(), script],
        BTreeMap::new(),
    )?;
    step.id = Some(size_step_id(plan.family)?);
    step.role = Some(StepRole::FamilySizeCheck);
    Ok(step)
}

fn size_step_id(family: CacheFamily) -> Result<StepId, RenderError> {
    [
        StepId::FamilySizeBunInstall,
        StepId::FamilySizePlaywrightBrowsers,
        StepId::FamilySizeDbtPackages,
        StepId::FamilySizePythonVenv,
        StepId::FamilySizeAnsibleCollections,
    ]
    .into_iter()
    .find(|id| id.as_str() == family_size_step_id(family))
    .ok_or_else(|| RenderError::InvalidWorkflow("family_size_step_id".to_owned()))
}

pub fn family_save_step(plan: &FamilyCachePlan) -> Result<Step, RenderError> {
    plan.check()?;
    let mut step = action_step_with_env(
        &format!("Save {} cache", plan.family.spec().layer),
        crate::cache_steps::TOOLS_SAVE_USES,
        BTreeMap::from([
            ("key".to_owned(), family_key_expr(plan.family)),
            ("path".to_owned(), plan.path_input()),
        ]),
        BTreeMap::new(),
    )?;
    step.condition = Some(family_save_condition(plan.family));
    step.role = Some(StepRole::FamilySave);
    Ok(step)
}
