//! Unix execution probes for emitted verifier composite scripts.

use std::collections::BTreeMap;

use velnor_actions_contract::workflow::StepRole;

use super::{
    field, render_verifier_composite, scalar, verifier_composite_document, verifier_shell_step,
};
use crate::yaml::Yaml;

#[cfg(unix)]
fn make_executable(path: &std::path::Path, body: &str) {
    use std::{fs, os::unix::fs::PermissionsExt};

    fs::write(path, body).expect("write executable test helper");
    let mut permissions = fs::metadata(path).expect("helper metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).expect("executable test helper");
}

#[cfg(unix)]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(unix)]
pub(super) fn poisoned_helper_dir(
    root: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::fs;

    let poison = root.join("poison");
    fs::create_dir_all(&poison).expect("poison directory");
    let log = root.join("poison.log");
    let digest = if std::path::Path::new("/usr/bin/sha256sum").exists() {
        "sha256sum"
    } else {
        "shasum"
    };
    for helper in ["awk", "mkdir", digest] {
        make_executable(
            &poison.join(helper),
            &format!(
                "#!/bin/sh\nprintf '%s\\n' '{}' >> {}\nexit 97\n",
                helper,
                shell_quote(&log.display().to_string())
            ),
        );
    }
    (poison, log)
}

#[cfg(unix)]
pub(super) fn verifier_body(reference: &str, digest: &str) -> String {
    let credential_checks = crate::toolchain_env::CREDENTIAL_UNSET_VARS
        .iter()
        .map(|name| format!("test -z \"${{{name}-}}\""))
        .collect::<Vec<_>>()
        .join("; ");
    let digest_args = if digest == "shasum" { "-a 256" } else { "" };
    format!(
        "set -eu; {credential_checks}; mkdir -p \"$VERIFIER_TEST_OUT\"; printf 'ok\\n' | awk '{{ print }}' > \"$VERIFIER_TEST_OUT/awk\"; printf 'ok\\n' | {digest} {digest_args} > \"$VERIFIER_TEST_OUT/digest\"; printf '%s\\n' \"${{{reference}}}\" > \"$VERIFIER_TEST_OUT/reference\""
    )
}

#[cfg(unix)]
fn assert_emitted_env(verifier: &Yaml) -> BTreeMap<String, String> {
    let emitted = super::scalar_map(super::field(verifier, "env"));
    assert_eq!(
        emitted.get("PATH").map(String::as_str),
        Some(crate::VERIFY_PATH)
    );
    for key in crate::toolchain_env::STEP_CREDENTIAL_DENYLIST
        .into_iter()
        .chain(crate::toolchain_env::STEP_ENDPOINT_DENYLIST)
    {
        assert_eq!(emitted.get(key).map(String::as_str), Some(""), "{key}");
    }
    assert_eq!(emitted.len(), 12, "11 scrub values and trusted PATH");
    emitted
}

#[cfg(unix)]
pub(super) fn execute_emitted_verifier(
    root: &std::path::Path,
    poison: &std::path::Path,
    name: &str,
    role: StepRole,
    uses: &str,
    reference: &str,
    body: &str,
) {
    use std::{fs, process::Command};

    let unset = crate::toolchain_env::credential_unset_prelude();
    let step = verifier_shell_step(
        role,
        name,
        vec!["sh".to_owned(), "-c".to_owned(), format!("{unset} {body}")],
        BTreeMap::new(),
    )
    .expect("canonical verifier shell");
    assert_eq!(step.role, Some(role));
    let file = render_verifier_composite(uses, &step, "0.1.40")
        .expect("canonical emitted verifier action");
    let action_name = "Velnor test verifier";
    let document =
        verifier_composite_document(action_name, &step).expect("structured emitted action");
    let runs = field(&document, "runs");
    let Yaml::Seq(steps) = field(runs, "steps") else {
        panic!("composite steps must be a sequence");
    };
    assert_eq!(steps.len(), 2, "resolver precedes the verifier step");
    assert_eq!(
        scalar(field(&steps[0], "name")),
        "Resolve project tool versions"
    );
    let verifier = &steps[1];
    assert_eq!(scalar(field(verifier, "name")), name);
    assert_eq!(
        scalar(field(verifier, "shell")),
        crate::VERIFY_VERIFIER_SHELL
    );
    let run = scalar(field(verifier, "run"));
    let emitted_env = assert_emitted_env(verifier);
    assert!(
        file.bytes
            .contains(&format!("shell: {}", crate::VERIFY_VERIFIER_SHELL))
    );
    assert!(file.bytes.contains("unset ACTIONS_ID_TOKEN_REQUEST_TOKEN"));
    assert!(file.bytes.contains("function_env_scan_complete=0"));
    let script_path = root.join(format!("{}.sh", name.replace(' ', "-")));
    fs::write(&script_path, run).expect("write emitted run body");
    let output_dir = root.join(format!("{}-out", name.replace(' ', "-")));
    let output = Command::new("/bin/bash")
        .args(["--noprofile", "--norc", "-p", "-e", "-o", "pipefail"])
        .arg(script_path)
        .env("PATH", poison)
        .envs(emitted_env.iter())
        .env("VERIFIER_TEST_OUT", &output_dir)
        .env(reference, "1.99.1")
        .envs(
            crate::toolchain_env::CREDENTIAL_UNSET_VARS
                .iter()
                .map(|key| (*key, "ambient-secret")),
        )
        .output()
        .expect("run emitted verifier action script");
    assert!(
        output.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output_dir.join("awk").is_file());
    assert!(output_dir.join("digest").is_file());
    assert!(output_dir.join("reference").is_file());
}
