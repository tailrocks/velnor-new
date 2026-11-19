// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_calls::CONFIG;
use crate::declared_fixture::{TestResult, job, refusal, rendered_file};
use crate::yaml_tree::Node;

const LOCAL: &str = "./.github/workflows/hand-written.yml";
const PINNED: &str =
    "tailrocks/velnor-new/.github/workflows/x.yml@0123456789abcdef0123456789abcdef01234567";

/// A push workflow `caller` whose job `release` calls `uses` with `extra` job keys.
fn caller(uses: &str, extra: &str) -> String {
    format!(
        "[[workflow]]\nfile = \"caller\"\nname = \"Caller\"\n\n[workflow.on.push]\nbranches = [\"main\"]\n\n[[workflow.job]]\nid = \"release\"\nuses = \"{uses}\"\npermissions = {{ contents = \"read\" }}\n{extra}\n"
    )
}

/// Secrets never reach a callee velnor-gen cannot read (a `./.github/workflows` file or a
/// pinned other-repository workflow), so the call cannot be given `secrets` in any form.
#[test]
fn an_opaque_callee_cannot_be_given_secrets_inherit_or_a_secrets_map() -> TestResult {
    for uses in [LOCAL, PINNED] {
        for extra in [
            "secrets = \"inherit\"",
            "secrets = { TOKEN = \"${{ secrets.TOKEN }}\" }",
            "secrets = {}",
        ] {
            let message = refusal(&caller(uses, extra))?;
            assert!(
                message.contains("secrets"),
                "{uses} {extra}: refused for another reason: {message}"
            );
        }
        let with_secret = caller(uses, "with = { token = \"${{ secrets.TOKEN }}\" }");
        let message = refusal(&with_secret)?;
        assert!(message.contains("secrets.TOKEN"), "{uses}: {message}");
    }
    Ok(())
}

#[test]
fn a_declared_callee_cannot_be_given_secrets_either() -> TestResult {
    let inherit = CONFIG.replace(
        "uses = \"leaf\"\n",
        "uses = \"leaf\"\nsecrets = \"inherit\"\n",
    );
    let message = refusal(&inherit)?;
    assert!(message.contains("secrets"), "{message}");
    Ok(())
}

#[test]
fn a_rendered_call_job_never_has_a_secrets_key() -> TestResult {
    for uses in [LOCAL, PINNED] {
        let tree = rendered_file(&caller(uses, ""), "caller")?;
        let release = job(&tree, "release")?;
        assert_eq!(release.get("uses").and_then(Node::text), Some(uses));
        assert!(!release.has_key_anywhere("secrets"), "{uses}");
    }
    let tree = rendered_file(CONFIG, "caller")?;
    assert!(!job(&tree, "release")?.has_key_anywhere("secrets"));
    Ok(())
}
