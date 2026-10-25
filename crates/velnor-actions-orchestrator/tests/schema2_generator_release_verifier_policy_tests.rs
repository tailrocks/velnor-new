use std::error::Error;

use super::{assert_authenticated_binding, assert_exact_empty_verifier_credentials, unquote};

const MISE_INSTALL_STEP: &str = "Install pinned resource-probe build and qualification tools";
const MISE_INSTALL_TOKEN: &str = "${{ github.token }}";
const BINARY_WORKFLOW: &str = include_str!("snapshots/product-release-binary.yml");

#[test]
fn id_token_permission_is_not_credential_environment() {
    const PERMISSION: &str =
        "permissions:\n  id-token: write\njobs:\n  verify:\n    steps:\n      - name: Verify\n";
    const ENVIRONMENT: &str = "jobs:\n  verify:\n    steps:\n      - name: Unsafe\n        env:\n          id-token: write\n";
    assert!(super::parse_steps(PERMISSION, &[], &[]).is_ok());
    assert!(super::parse_steps(ENVIRONMENT, &[], &[]).is_err());
}

pub(in crate::impl_schema2_routing::schema2_product_release_snapshots) fn assert_release_workflow_verifier(
    workflow: &str,
    step_name: &str,
    globally_scoped_key: &str,
    install_step_name: &str,
    install_token: Option<(&str, &str)>,
) -> Result<(), Box<dyn Error>> {
    let step = named_step(workflow, step_name)?;
    assert_exact_empty_verifier_credentials(&step, &[step_name])?;
    let install = named_step(workflow, install_step_name)?;
    if let Some((key, value)) = install_token {
        assert_authenticated_binding(&install, install_step_name, key, value)?;
    } else {
        super::parse_steps(&install, &[], &[])?;
    }
    if workflow.matches(&format!("{globally_scoped_key}:")).count() != 1 {
        return Err(format!("{globally_scoped_key} must occur once in the workflow").into());
    }
    if !step
        .lines()
        .any(|line| line.trim() == format!("{globally_scoped_key}: \"\""))
    {
        return Err(format!("{globally_scoped_key} must be empty in {step_name:?}").into());
    }
    Ok(())
}

fn named_step(body: &str, expected_name: &str) -> Result<String, Box<dyn Error>> {
    let lines = body.lines().collect::<Vec<_>>();
    let starts = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            line.trim_start()
                .strip_prefix("- name: ")
                .filter(|name| unquote(name) == expected_name)
                .map(|_| index)
        })
        .collect::<Vec<_>>();
    if starts.len() != 1 {
        return Err(format!(
            "expected one {expected_name:?} step, found {}",
            starts.len()
        )
        .into());
    }
    let start = starts[0];
    let indent = lines[start].len() - lines[start].trim_start().len();
    let end = lines[start + 1..]
        .iter()
        .position(|line| {
            let line_indent = line.len() - line.trim_start().len();
            line_indent == indent && line.trim_start().starts_with("- name: ")
        })
        .map_or(lines.len(), |offset| start + 1 + offset);
    Ok(lines[start..end].join("\n"))
}

pub(super) fn replace_named_step(
    body: &str,
    step_name: &str,
    from: &str,
    to: &str,
) -> Result<String, Box<dyn Error>> {
    let step = named_step(body, step_name)?;
    let changed_step = step.replacen(from, to, 1);
    if changed_step == step {
        return Err(format!("{from:?} was not present in {step_name:?}").into());
    }
    if body.matches(&step).count() != 1 {
        return Err(format!("expected one exact {step_name:?} step body").into());
    }
    Ok(body.replacen(&step, &changed_step, 1))
}

pub(super) fn remove_named_step(body: &str, step_name: &str) -> Result<String, Box<dyn Error>> {
    let step = named_step(body, step_name)?;
    let start = body.find(&step).ok_or("step body is missing")?;
    let mut end = start + step.len();
    if body.as_bytes().get(end) == Some(&b'\n') {
        end += 1;
    }
    let mut removed = String::with_capacity(body.len() - (end - start));
    removed.push_str(&body[..start]);
    removed.push_str(&body[end..]);
    Ok(removed)
}

pub(super) fn duplicate_named_step(body: &str, step_name: &str) -> Result<String, Box<dyn Error>> {
    let step = named_step(body, step_name)?;
    Ok(format!("{body}\n{step}\n"))
}

#[test]
fn release_workflow_keeps_registry_token_empty_and_mise_auth_narrow() {
    const WORKFLOW: &str = include_str!("snapshots/product-release-images.yml");
    assert!(
        assert_release_workflow_verifier(
            WORKFLOW,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            MISE_INSTALL_STEP,
            Some(("GITHUB_TOKEN", MISE_INSTALL_TOKEN)),
        )
        .is_ok()
    );
    let live = WORKFLOW.replacen(
        "CARGO_REGISTRY_TOKEN: \"\"",
        "CARGO_REGISTRY_TOKEN: ${{ secrets.REGISTRY_TOKEN }}",
        1,
    );
    assert!(
        assert_release_workflow_verifier(
            &live,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            MISE_INSTALL_STEP,
            Some(("GITHUB_TOKEN", MISE_INSTALL_TOKEN)),
        )
        .is_err()
    );
    let runner_image_step = "      - name: Build Runner and DinD images\n";
    assert_eq!(WORKFLOW.matches(runner_image_step).count(), 1);
    let misplaced = WORKFLOW.replacen(
        runner_image_step,
        "      - name: Build Runner and DinD images\n        env:\n          CARGO_REGISTRY_TOKEN: \"\"\n",
        1,
    );
    assert_ne!(misplaced, WORKFLOW, "the mutation changes one real step");
    assert!(
        assert_release_workflow_verifier(
            &misplaced,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            MISE_INSTALL_STEP,
            Some(("GITHUB_TOKEN", MISE_INSTALL_TOKEN)),
        )
        .is_err()
    );

    let live_mise_token = WORKFLOW.replace(
        "GITHUB_TOKEN: ${{ github.token }}",
        "GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}",
    );
    assert_ne!(live_mise_token, WORKFLOW);
    assert!(
        assert_release_workflow_verifier(
            &live_mise_token,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            MISE_INSTALL_STEP,
            Some(("GITHUB_TOKEN", MISE_INSTALL_TOKEN)),
        )
        .is_err()
    );
    let missing_mise_token = WORKFLOW.replace("GITHUB_TOKEN: ${{ github.token }}\n", "");
    assert_ne!(missing_mise_token, WORKFLOW);
    assert!(
        assert_release_workflow_verifier(
            &missing_mise_token,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            MISE_INSTALL_STEP,
            Some(("GITHUB_TOKEN", MISE_INSTALL_TOKEN)),
        )
        .is_err()
    );
}

#[test]
fn binary_workflow_keeps_mise_install_token_free() {
    assert!(
        assert_release_workflow_verifier(
            BINARY_WORKFLOW,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            "Install pinned Rust and MBX",
            None,
        )
        .is_ok()
    );

    let tokenized = BINARY_WORKFLOW.replacen(
        "      - name: Install pinned Rust and MBX\n",
        "      - name: Install pinned Rust and MBX\n        env:\n          GITHUB_TOKEN: ${{ github.token }}\n",
        1,
    );
    assert_ne!(tokenized, BINARY_WORKFLOW);
    assert!(
        assert_release_workflow_verifier(
            &tokenized,
            "Verify Rust channel manifest",
            "CARGO_REGISTRY_TOKEN",
            "Install pinned Rust and MBX",
            None,
        )
        .is_err()
    );
}
