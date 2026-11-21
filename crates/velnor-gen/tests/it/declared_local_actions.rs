// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::declared_fixture::{TestResult, refusal, rendered_text, workflow};

const PUSH: &str = "[workflow.on.push]\nbranches = [\"main\"]\n";

const ALLOWED: &[&str] = &[
    "./qualification/actions/post-fail",
    "./qualification/actions/docker",
    "./qualification/actions/composite",
];

const CHECKOUT: &str = "[[workflow.job.step]]\nname = \"Check out\"\nuses = \"checkout\"\n\n";

fn local_after(before: &str, extra: &str) -> String {
    workflow(
        "",
        PUSH,
        &format!(
            "[[workflow.job]]\nid = \"check\"\npermissions = {{ contents = \"read\" }}\n\n{before}[[workflow.job.step]]\nname = \"Local\"\n{extra}\n"
        ),
    )
}

fn local(extra: &str) -> String {
    local_after(CHECKOUT, extra)
}

#[test]
fn an_allowed_local_action_renders_with_the_self_repository_note() -> TestResult {
    for path in ALLOWED {
        let text = rendered_text(&local(&format!("uses = \"{path}\"")), "ci")?;
        assert!(
            text.contains(&format!(
                "uses: '{path}'  # zizmor: ignore[self-repository]\n"
            )),
            "{path}\n{text}"
        );
    }
    Ok(())
}

#[test]
fn a_local_action_outside_the_allow_list_is_refused_and_the_message_names_the_allowed_ones()
-> TestResult {
    for path in [
        "./.github/actions/anything",
        "./qualification/actions/other",
        "./qualification/actions",
        "./qualification/actions/composite/",
        "./qualification/actions/composite/../docker",
        "./qualification/actions/Composite",
        "../qualification/actions/composite",
        "/qualification/actions/composite",
        "qualification/actions/composite",
        "owner/repo/qualification/actions/composite@main",
    ] {
        let message = refusal(&local(&format!("uses = \"{path}\"")))?;
        assert!(message.contains(path), "{path}: {message}");
        for allowed in ALLOWED.iter().copied() {
            assert!(message.contains(allowed), "{path}: {message}");
        }
    }
    Ok(())
}

#[test]
fn a_local_action_takes_no_inputs() -> TestResult {
    let message = refusal(&local(&format!(
        "uses = \"{}\"\nwith = {{ anything = \"x\" }}",
        ALLOWED[2]
    )))?;
    assert!(message.contains("with"), "{message}");
    Ok(())
}

#[test]
fn a_local_action_needs_an_earlier_unconditional_checkout_into_the_workspace() -> TestResult {
    let uses = format!("uses = \"{}\"", ALLOWED[2]);
    let conditional = "[[workflow.job.step]]\nname = \"Check out\"\nuses = \"checkout\"\nif = \"github.event_name == 'push'\"\n\n";
    let elsewhere = "[[workflow.job.step]]\nname = \"Check out\"\nuses = \"checkout\"\nwith = { path = \"other\" }\n\n";
    for (what, before) in [
        ("no checkout", ""),
        ("conditional checkout", conditional),
        ("checkout into a path", elsewhere),
    ] {
        let message = refusal(&local_after(before, &uses))?;
        assert!(message.contains("checkout"), "{what}: {message}");
    }
    Ok(())
}
