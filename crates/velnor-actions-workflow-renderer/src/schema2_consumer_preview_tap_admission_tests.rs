//! The credentialed process must exit before source/updater preparation begins.
use super::*;
fn field<'a>(node: &'a Yaml, key: &str) -> &'a Yaml {
    let Yaml::Map(fields) = node else {
        panic!("map")
    };
    &fields.iter().find(|(name, _)| name == key).expect(key).1
}
fn operation(step: &Yaml, name: &str) -> bool {
    let Yaml::Map(fields) = step else {
        return false;
    };
    let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env") else {
        return false;
    };
    env.iter()
        .any(|(key, value)| key == "VELNOR_INTERNAL_OP" && value == &Yaml::str(name))
}

#[test]
fn both_tap_routes_end_authenticated_admission_before_checkout_and_token_free_prepare() {
    let mut spec = with_tap();
    spec.install_rust_argv[3] = "rust@${VELNOR_TOOL_RUST}".to_owned();
    let policy = spec.homebrew_tap.as_ref().expect("tap");
    for context in [
        TapInvocationContext::Preview,
        TapInvocationContext::Reconcile,
    ] {
        let jobs = tap::jobs(&spec, policy, context).expect("jobs");
        let original = Yaml::Map(vec![("jobs".to_owned(), Yaml::Map(jobs.clone()))]);
        let mut bound = original.clone();
        crate::project_tool_resolver::bind_workflow(&mut bound)
            .expect("no project reads in tap operations");
        assert_eq!(bound, original, "tap admission/checkout order is unchanged");
        let Yaml::Seq(steps) = field(&jobs[0].1, "steps") else {
            panic!("steps")
        };
        let admission = steps
            .iter()
            .position(|step| operation(step, "admit-homebrew-tap-v1"))
            .expect("admit");
        let prepare = steps
            .iter()
            .position(|step| operation(step, "prepare-homebrew-tap-v1"))
            .expect("prepare");
        let checkout = steps
            .iter()
            .position(|step| crate::yaml::render_yaml(step).contains("actions/checkout@"))
            .expect("checkout");
        assert!(admission < checkout && checkout < prepare);
        assert_eq!(
            field(field(&steps[admission], "env"), "GH_TOKEN"),
            &Yaml::str("${{ github.token }}")
        );
        assert_eq!(
            field(
                field(&steps[admission], "env"),
                "VELNOR_RELEASE_STATUS_TOKEN"
            ),
            &Yaml::str("${{ secrets.RELEASE_STATUS_READ_TOKEN }}")
        );
        assert_eq!(field(&steps[admission], "id"), &Yaml::str("tap-admission"));
        let env = field(&steps[prepare], "env");
        let text = crate::yaml::render_yaml(env);
        assert!(
            !text.contains("GH_TOKEN")
                && !text.contains("GITHUB_TOKEN")
                && !text.contains("secrets.")
        );
        assert_eq!(
            field(env, "VELNOR_TAP_ADMISSION_DIGEST"),
            &Yaml::str("${{ steps.tap-admission.outputs.admission_digest }}")
        );
        for step in &steps[prepare..] {
            let text = crate::yaml::render_yaml(step);
            assert!(!text.contains("secrets."));
            if let Yaml::Map(fields) = step
                && let Some((_, Yaml::Map(env))) = fields.iter().find(|(key, _)| key == "env")
            {
                assert!(
                    !env.iter()
                        .any(|(key, _)| matches!(key.as_str(), "GH_TOKEN" | "GITHUB_TOKEN"))
                );
            }
        }
        let script = crate::yaml::render_yaml(&steps[prepare]);
        assert!(
            script.contains(&crate::toolchain_env::credential_unset_prelude()),
            "{script}"
        );
        assert!(script.contains("unset VELNOR_RELEASE_STATUS_TOKEN;"));
        assert!(!crate::yaml::render_yaml(&steps[prepare]).contains("set -C"));
        for (_, job) in &jobs[1..] {
            assert!(crate::yaml::render_yaml(job).contains("GH_TOKEN"));
        }
    }
}
