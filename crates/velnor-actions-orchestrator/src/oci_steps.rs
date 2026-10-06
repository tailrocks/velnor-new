//! Fixed native OCI action and shell steps.

use super::{OciRenderContext, scripts};
use std::collections::BTreeMap;
use velnor_actions_contract::{
    CompiledSourceHelper, Step, StepId, StepKind,
    config::{OciImage, OciReleaseConfig, RegistryAuthentication},
};
use velnor_actions_workflow_renderer::RenderError;

const AMD64_RUNNER: &str = "ubuntu-24.04";
const DOCKER_CONFIG: &str = "${{ runner.temp }}/velnor/oci-docker";

/// Build one fixed action step from typed scalar inputs.
pub(super) fn action(name: &str, uses: &str, with: Vec<(&str, String)>) -> Step {
    action_with_env(name, uses, with, Vec::new())
}

fn action_with_env(
    name: &str,
    uses: &str,
    with: Vec<(&str, String)>,
    env: Vec<(&str, String)>,
) -> Step {
    Step {
        id: None,
        name: name.to_owned(),
        condition: None,
        kind: StepKind::Action {
            uses: uses.to_owned(),
            with: strings(with),
            env: strings(env),
        },
    }
}

fn strings(entries: Vec<(&str, String)>) -> BTreeMap<String, String> {
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

fn docker_env() -> Vec<(&'static str, String)> {
    vec![("DOCKER_CONFIG", DOCKER_CONFIG.to_owned())]
}

fn set_id(step: &mut Step, id: &str) -> Result<(), RenderError> {
    step.id = Some(StepId::new(id).map_err(RenderError::Contract)?);
    Ok(())
}

pub(super) fn shell(
    context: &OciRenderContext,
    name: &str,
    id: &str,
    script: Vec<String>,
    env: Vec<(&str, String)>,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Step, RenderError> {
    super::bound_steps::render(context, name, id, &script, env, runner, records)
}

pub(super) fn checkout(context: &OciRenderContext, lfs: bool) -> Step {
    action(
        "Checkout exact source",
        &context.pins.checkout,
        vec![
            ("ref", "${{ github.sha }}".to_owned()),
            ("fetch-depth", "0".to_owned()),
            ("persist-credentials", "false".to_owned()),
            ("lfs", lfs.to_string()),
        ],
    )
}

pub(super) fn login(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
) -> Result<Step, RenderError> {
    let RegistryAuthentication::NamedSecrets {
        username_secret,
        password_secret,
    } = &config.authentication
    else {
        return Err(RenderError::InvalidWorkflow(
            "oci_authentication_unqualified".into(),
        ));
    };
    let mut step = action_with_env(
        "Registry login",
        &context.pins.login,
        vec![
            ("registry", config.registry.clone()),
            ("username", format!("${{{{ secrets.{username_secret} }}}}")),
            ("password", format!("${{{{ secrets.{password_secret} }}}}")),
        ],
        docker_env(),
    );
    set_id(&mut step, "registry_login")?;
    Ok(step)
}

pub(super) fn setup(context: &OciRenderContext) -> Step {
    action_with_env(
        "Set up native Buildx",
        &context.pins.buildx,
        vec![
            ("version", context.pins.buildx_version.clone()),
            ("driver", "docker-container".into()),
            (
                "driver-opts",
                format!("image={}", context.pins.buildkit_image),
            ),
        ],
        docker_env(),
    )
}

fn source_gate(
    context: &OciRenderContext,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Step, RenderError> {
    shell(
        context,
        "Recheck current protected source",
        "source",
        scripts::source_script(&context.repository, &context.default_branch),
        source_environment(),
        AMD64_RUNNER,
        records,
    )
}

fn source_environment() -> Vec<(&'static str, String)> {
    vec![
        ("GH_TOKEN", "${{ github.token }}".to_owned()),
        ("REF", "${{ github.ref }}".to_owned()),
        ("SOURCE_SHA", "${{ github.sha }}".to_owned()),
        ("REPOSITORY", "${{ github.repository }}".to_owned()),
    ]
}

pub(super) fn identity(image: &OciImage) -> Vec<(&'static str, String)> {
    vec![
        ("IMAGE", image.image.clone()),
        ("IMAGE_ID", image.id.clone()),
        ("VERSION", "${{ needs.verify.outputs.version }}".to_owned()),
        ("SOURCE_SHA", "${{ github.sha }}".to_owned()),
    ]
}

pub(super) fn admission(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    let mut env = identity(image);
    env.extend([
        (
            "RECOVERY_JSON",
            "${{ github.event_name == 'workflow_dispatch' && inputs.recovery || '{}' }}".to_owned(),
        ),
        ("ALL_IDS", all_image_ids(config)),
    ]);
    let mut result = vec![checkout(context, false)];
    result.extend(super::bound_steps::setup(context, AMD64_RUNNER, records)?);
    result.extend([setup(context), login(config, context)?]);
    result.push(shell(
        context,
        "Admit immutable version",
        "admit",
        scripts::admission_script(),
        env,
        AMD64_RUNNER,
        records,
    )?);
    Ok(result)
}

fn all_image_ids(config: &OciReleaseConfig) -> String {
    config
        .images
        .iter()
        .map(|image| image.id.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

pub(super) fn build(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    arch: &str,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    let mut result = vec![checkout(context, image.lfs)];
    result.extend(native_tools(context, runner, records)?);
    result.push(setup(context));
    result.extend(super::platform_steps::source_gates(
        context, arch, runner, records,
    )?);
    result.push(build_action(context, image, arch)?);
    result.push(login(config, context)?);
    result.push(super::platform_steps::publish(
        context, image, arch, runner, records,
    )?);
    result.extend(super::platform_steps::records(
        context, image, arch, runner, records,
    )?);
    result.push(upload_digest(context, image, arch)?);
    Ok(result)
}

fn native_tools(
    context: &OciRenderContext,
    runner: &str,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    super::bound_steps::setup(context, runner, records)
}

pub(super) fn build_action(
    context: &OciRenderContext,
    image: &OciImage,
    arch: &str,
) -> Result<Step, RenderError> {
    let mut step = action_with_env(
        "Build exact native platform archive",
        &context.pins.build,
        vec![
            ("context", image.context.clone()),
            ("file", image.dockerfile.clone()),
            ("platforms", format!("linux/{arch}")),
            ("tags", image.image.clone()),
            (
                "outputs",
                format!(
                    "type=oci,dest={},oci-mediatypes=true",
                    super::platform_steps::archive_path(image, arch)
                ),
            ),
            ("push", "false".into()),
            ("provenance", "mode=max".to_owned()),
            ("sbom", format!("generator={}", context.pins.sbom_image)),
            ("build-args", build_args(image)),
            ("labels", labels(context)),
        ],
        docker_env(),
    );
    set_id(&mut step, "build")?;
    Ok(step)
}

fn build_args(image: &OciImage) -> String {
    let mut args = String::from("VERSION=${{ needs.verify.outputs.version }}");
    for (name, value) in &image.build_args {
        args.push_str(&format!("\n{name}={value}"));
    }
    args
}

fn labels(context: &OciRenderContext) -> String {
    format!(
        "org.opencontainers.image.version=${{{{ needs.verify.outputs.version }}}}\norg.opencontainers.image.revision=${{{{ github.sha }}}}\norg.opencontainers.image.source=https://github.com/{}",
        context.repository
    )
}

fn upload_digest(
    context: &OciRenderContext,
    image: &OciImage,
    arch: &str,
) -> Result<Step, RenderError> {
    let mut step = action(
        "Upload immutable digest record",
        &context.pins.upload,
        vec![
            (
                "name",
                format!(
                    "oci-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-{}-{arch}",
                    image.id
                ),
            ),
            ("path", format!("digests/{}-{arch}.json", image.id)),
            ("if-no-files-found", "error".to_owned()),
            ("retention-days", "2".to_owned()),
        ],
    );
    set_id(&mut step, "digestproof")?;
    Ok(step)
}

pub(super) fn assemble(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
    records: &mut Vec<CompiledSourceHelper>,
) -> Result<Vec<Step>, RenderError> {
    let mut steps = vec![checkout(context, false)];
    steps.extend(super::bound_steps::setup(context, AMD64_RUNNER, records)?);
    steps.extend([setup(context), login(config, context)?]);
    steps.extend(super::transport_steps::downloads(context, image, records)?);
    steps.push(source_gate(context, records)?);
    steps.push(shell(
        context,
        "Assemble and verify immutable index",
        "assemble",
        scripts::assembly_script(&context.repository, &context.default_branch),
        assembly_environment(config, image),
        AMD64_RUNNER,
        records,
    )?);
    steps.push(index_upload(context, image)?);
    Ok(steps)
}

fn assembly_environment(
    config: &OciReleaseConfig,
    image: &OciImage,
) -> Vec<(&'static str, String)> {
    let mut env = identity(image);
    env.extend([
        ("PLATFORMS", platforms(image)),
        ("REGISTRY", config.registry.clone()),
        (
            "EXISTING",
            format!("${{{{ needs.admit-{}.outputs.existing }}}}", image.id),
        ),
        (
            "EXPECTED_INDEX_DIGEST",
            format!("${{{{ needs.admit-{}.outputs.index_digest }}}}", image.id),
        ),
    ]);
    env.extend(source_environment());
    env
}

fn platforms(image: &OciImage) -> String {
    image
        .platforms
        .iter()
        .map(|platform| platform.arch())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join(",")
}

fn index_upload(context: &OciRenderContext, image: &OciImage) -> Result<Step, RenderError> {
    let mut step = action(
        "Upload verified immutable index identity",
        &context.pins.upload,
        vec![
            (
                "name",
                format!(
                    "oci-index-${{{{ github.run_id }}}}-${{{{ github.run_attempt }}}}-{}",
                    image.id
                ),
            ),
            ("path", "index-proof.json".to_owned()),
            ("if-no-files-found", "error".to_owned()),
            ("retention-days", "90".to_owned()),
        ],
    );
    set_id(&mut step, "indexproof")?;
    Ok(step)
}

pub(super) fn attest_index(
    config: &OciReleaseConfig,
    context: &OciRenderContext,
    image: &OciImage,
) -> Step {
    let name = if image.image.starts_with(&format!("{}/", config.registry)) {
        image.image.clone()
    } else {
        format!("{}/{}", config.registry, image.image)
    };
    action(
        "Attest verified immutable index source",
        &context.pins.attest,
        vec![
            ("subject-name", name),
            (
                "subject-digest",
                "${{ steps.receipt.outputs.index_digest }}".to_owned(),
            ),
            ("push-to-registry", "false".to_owned()),
        ],
    )
}
