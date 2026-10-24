const IMAGE_WORKFLOW: &str = include_str!("snapshots/product-release-images.yml");
const BINARY_WORKFLOW: &str = include_str!("snapshots/product-release-binary.yml");
const RUST_VERIFIER: &[&str] = &["Verify Rust channel manifest"];
const IMAGE_AUTH: &[(&str, &str, &str)] = &[
    (
        "Install pinned Rust for the resource probe",
        "GITHUB_TOKEN",
        "${{ github.token }}",
    ),
    ("Publish GitHub release", "GH_TOKEN", "${{ github.token }}"),
];
const BINARY_AUTH: &[(&str, &str, &str)] =
    &[("Publish GitHub release", "GH_TOKEN", "${{ github.token }}")];

fn accepts(body: &str, authenticated: &[(&str, &str, &str)]) -> bool {
    super::assert_exact_empty_verifier_credentials_with_bindings(body, RUST_VERIFIER, authenticated)
        .is_ok()
}

fn reject_tamper(body: &str, tampered: &str, authenticated: &[(&str, &str, &str)], label: &str) {
    assert_ne!(tampered, body, "tamper fixture is a no-op: {label}");
    assert!(
        !accepts(tampered, authenticated),
        "tampered family workflow credentials were accepted: {label}"
    );
}

fn inject_step_env(body: &str, step_name: &str, binding: &str) -> String {
    let marker = format!("      - name: {step_name}\n");
    let replacement = format!("{marker}        env:\n          {binding}\n");
    body.replacen(&marker, &replacement, 1)
}

#[test]
fn family_workflows_allow_only_their_explicit_authenticated_bindings() {
    for (body, authenticated) in [(IMAGE_WORKFLOW, IMAGE_AUTH), (BINARY_WORKFLOW, BINARY_AUTH)] {
        assert!(accepts(body, authenticated));
        for replacement in [
            "CARGO_REGISTRY_TOKEN: live",
            "CARGO_REGISTRY_TOKEN: \"live\"",
            "CARGO_REGISTRY_TOKEN: ${{ secrets.REGISTRY_TOKEN }}",
            "CARGO_REGISTRY_TOKEN: \"${{ secrets.REGISTRY_TOKEN }}\"",
            "\"CARGO_REGISTRY_TOKEN\": \"live\"",
        ] {
            let tampered = body.replacen("CARGO_REGISTRY_TOKEN: \"\"", replacement, 1);
            reject_tamper(body, &tampered, authenticated, "verifier binding");
        }

        for binding in [
            "CARGO_REGISTRY_TOKEN: live",
            "CARGO_REGISTRY_TOKEN: ${{ secrets.REGISTRY_TOKEN }}",
            "\"CARGO_REGISTRY_TOKEN\": \"live\"",
        ] {
            let tampered = inject_step_env(body, "Setup Mise", binding);
            reject_tamper(
                body,
                &tampered,
                authenticated,
                "misplaced Setup Mise binding",
            );
        }

        for binding in [
            "CARGO_REGISTRY_TOKEN: live",
            "\"CARGO_REGISTRY_TOKEN\": \"${{ secrets.REGISTRY_TOKEN }}\"",
        ] {
            let root_env = format!("env:\n  {binding}\njobs:\n");
            let tampered = body.replacen("jobs:\n", &root_env, 1);
            reject_tamper(body, &tampered, authenticated, "workflow-level binding");
        }

        let inline = body.replacen(
            "      - name: Verify delegated release authority\n",
            "      - name: Verify delegated release authority\n        env: { \"CARGO_REGISTRY_TOKEN\": \"live\" }\n",
            1,
        );
        reject_tamper(body, &inline, authenticated, "inline binding");
    }
}
