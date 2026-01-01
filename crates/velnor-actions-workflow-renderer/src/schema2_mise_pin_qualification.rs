//! Hosted-only qualification for exact Mise release assets.

use super::{MisePinQualificationPins, RunnerSpec};
use crate::RenderError;
use crate::yaml::Yaml;
use velnor_actions_contract::ReleaseTarget;

const MODE: &str = "inputs.mode == 'mise-pin'";
const LINUX_X64: ReleaseTarget = ReleaseTarget::LinuxX86_64;
const MACOS_X64: ReleaseTarget = ReleaseTarget::MacosX86_64;
const CHECKOUT_USES: &str = super::features::CHECKOUT_USES;

/// Emit read-only jobs that verify the pinned release on Linux and macOS x64.
///
/// # Errors
/// Invalid action, version, or digest pins fail closed.
pub(super) fn jobs(pins: &MisePinQualificationPins) -> Result<Vec<(String, Yaml)>, RenderError> {
    pins.linux_x86_64_setup.validate()?;
    pins.macos_x86_64_setup.validate()?;
    if pins.linux_x86_64_setup.version != pins.macos_x86_64_setup.version {
        return Err(RenderError::BadCommand(
            "mise_pin_qualification_version_mismatch".to_owned(),
        ));
    }

    Ok(vec![
        job(
            "mise-pin-linux-x64",
            "Mise pin / hosted / Linux x64",
            "ubuntu-26.04",
            LINUX_X64,
            &pins.linux_x86_64_setup,
        )?,
        job(
            "mise-pin-macos-x64",
            "Mise pin / hosted / macOS x64",
            "macos-15-intel",
            MACOS_X64,
            &pins.macos_x86_64_setup,
        )?,
    ])
}

fn job(
    id: &str,
    name: &str,
    runner: &str,
    target: ReleaseTarget,
    setup: &crate::setup::MiseSetup,
) -> Result<(String, Yaml), RenderError> {
    let hosted = RunnerSpec::hosted_release_target(runner, target)?;
    let mut fields = super::features::lane_base(name, &hosted, 20);
    fields.insert(1, ("if".to_owned(), Yaml::str(MODE)));
    fields.push(("permissions".to_owned(), mapping(&[("contents", "read")])));
    Ok(super::features::finish(
        id,
        fields,
        vec![
            checkout_step(),
            mise_setup_step(setup)?,
            verify_step(target, setup)?,
        ],
    ))
}

fn checkout_step() -> Yaml {
    Yaml::Map(vec![
        ("name".to_owned(), Yaml::str("Check out selected ref")),
        ("uses".to_owned(), Yaml::str(CHECKOUT_USES)),
        (
            "with".to_owned(),
            mapping(&[
                ("ref", "${{ github.sha }}"),
                ("persist-credentials", "false"),
            ]),
        ),
    ])
}

fn mise_setup_step(setup: &crate::setup::MiseSetup) -> Result<Yaml, RenderError> {
    let step = crate::setup::mise_setup_step(setup)?;
    crate::steps_plain::plain_step_to_yaml(&step)
}

fn verify_step(
    target: ReleaseTarget,
    setup: &crate::setup::MiseSetup,
) -> Result<Yaml, RenderError> {
    let run = verification_script(target)?;
    Ok(Yaml::Map(vec![
        (
            "name".to_owned(),
            Yaml::str("Verify selected ref, version, and binary digest"),
        ),
        ("shell".to_owned(), Yaml::str("bash")),
        (
            "env".to_owned(),
            mapping(&[
                ("MISE_VERSION", &setup.version),
                ("MISE_SHA256", &setup.sha256),
            ]),
        ),
        ("run".to_owned(), Yaml::str(run)),
    ]))
}

fn verification_script(target: ReleaseTarget) -> Result<String, RenderError> {
    let (platform, digest) = match target {
        ReleaseTarget::LinuxX86_64 => ("linux-x64", "sha256sum"),
        ReleaseTarget::MacosX86_64 => ("macos-x64", "shasum -a 256"),
        ReleaseTarget::MacosArm64 => {
            return Err(RenderError::BadCommand(format!(
                "mise_pin_qualification_unsupported_target:{}",
                target.triple()
            )));
        }
    };
    let run = format!(
        "set -euo pipefail\ntest \"$(git rev-parse HEAD)\" = \"$GITHUB_SHA\"\nversion=\"$(mise --version)\"\ncase \"$version\" in \"$MISE_VERSION {platform} (\"[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]\")\") ;; *) echo \"unexpected Mise version: $version\" >&2; exit 1 ;; esac\nbinary=\"$(command -v mise)\"\nactual=\"$({digest} \"$binary\" | awk '{{print $1}}')\"\ntest \"$actual\" = \"$MISE_SHA256\""
    );
    Ok(run)
}

fn mapping(pairs: &[(&str, &str)]) -> Yaml {
    Yaml::Map(
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), Yaml::str(*value)))
            .collect(),
    )
}

#[cfg(test)]
#[path = "schema2_mise_pin_qualification_tests.rs"]
mod tests;
