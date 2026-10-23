use super::{files, problem, verifiers};

fn action_call(name: &str, uses: &str) -> String {
    format!("      - name: {name}\n        uses: {uses} # zizmor: ignore[self-repository]\n")
}

fn workflow(call: &str) -> String {
    format!("jobs:\n  verify:\n    steps:\n{call}")
}

fn assert_rejected(case: &str, changed: &str) {
    let found = problem(&files(".github/workflows/verify.yml", changed));
    assert!(
        found.contains("noncanonical verifier call or body"),
        "{case} must fail closed: {found}"
    );
}

fn assert_mutations<const N: usize>(cases: [(&'static str, &str, String); N]) {
    for (case, baseline, changed) in cases {
        assert_eq!(
            problem(&files(".github/workflows/verify.yml", &workflow(baseline))),
            "",
            "{case} control must start from a valid canonical workflow"
        );
        assert_ne!(
            changed.as_str(),
            baseline,
            "{case} control must change the fixture"
        );
        assert_rejected(case, &workflow(&changed));
    }
}

#[test]
fn jactionlint_calls_reject_reference_and_annotation_mutations() {
    let verifier = verifiers().jactionlint;
    let uses = verifier.uses;
    let canonical = action_call("Check executable digest", uses);
    let recognized_label = action_call(verifier.name, uses);
    let other_ref = format!("{uses}-unregistered");
    let cases = [
        (
            "wrong reference",
            recognized_label.as_str(),
            recognized_label.replace(uses, &other_ref),
        ),
        (
            "wrong repository annotation",
            recognized_label.as_str(),
            recognized_label.replace("ignore[self-repository]", "ignore[other]"),
        ),
        (
            "missing repository annotation",
            canonical.as_str(),
            canonical.replace(" # zizmor: ignore[self-repository]", ""),
        ),
    ];
    assert_mutations(cases);
}

#[test]
fn jactionlint_calls_reject_untrusted_step_fields() {
    let uses = verifiers().jactionlint.uses;
    let canonical = action_call("Check executable digest", uses);
    let cases = [
        (
            "caller inputs",
            canonical.as_str(),
            canonical.replace(
                " # zizmor: ignore[self-repository]\n",
                " # zizmor: ignore[self-repository]\n        with:\n          version: latest\n",
            ),
        ),
        (
            "caller environment",
            canonical.as_str(),
            canonical.replace(
                " # zizmor: ignore[self-repository]\n",
                " # zizmor: ignore[self-repository]\n        env:\n          GH_TOKEN: untrusted\n",
            ),
        ),
        (
            "caller step id",
            canonical.as_str(),
            canonical.replace("      - name:", "      - id: verifier\n        name:"),
        ),
        (
            "caller condition",
            canonical.as_str(),
            canonical.replace("      - name:", "      - if: always()\n        name:"),
        ),
        (
            "shell override",
            canonical.as_str(),
            canonical.replace(
                " # zizmor: ignore[self-repository]\n",
                " # zizmor: ignore[self-repository]\n        shell: bash\n",
            ),
        ),
        (
            "unknown field",
            canonical.as_str(),
            canonical.replace(
                " # zizmor: ignore[self-repository]\n",
                " # zizmor: ignore[self-repository]\n        timeout-minutes: 1\n",
            ),
        ),
        (
            "executable body",
            canonical.as_str(),
            canonical.replace(
                " # zizmor: ignore[self-repository]\n",
                " # zizmor: ignore[self-repository]\n        run: echo untrusted\n",
            ),
        ),
    ];
    assert_mutations(cases);
}

#[test]
fn jactionlint_calls_reject_malformed_call_shapes() {
    let verifier = verifiers().jactionlint;
    let uses = verifier.uses;
    let canonical = action_call("Check executable digest", uses);
    let recognized_label = action_call(verifier.name, uses);
    let cases = [
        (
            "duplicate name",
            canonical.as_str(),
            canonical.replace(
                "      - name:",
                "      - name: duplicate\n        name:",
            ),
        ),
        (
            "missing name",
            canonical.as_str(),
            canonical.replace("      - name: Check executable digest\n", "      -\n"),
        ),
        (
            "missing reference",
            recognized_label.as_str(),
            recognized_label
                .replace(&format!("        uses: {uses}"), "        run: echo missing"),
        ),
        (
            "duplicate reference",
            canonical.as_str(),
            canonical.replace(
                " # zizmor: ignore[self-repository]\n",
                &format!(
                    " # zizmor: ignore[self-repository]\n        uses: {uses} # zizmor: ignore[self-repository]\n"
                ),
            ),
        ),
    ];
    assert_mutations(cases);
}

#[test]
fn jactionlint_call_rejects_a_quoted_empty_display_name() {
    let uses = verifiers().jactionlint.uses;
    let empty = action_call("\"\"", uses);
    assert_rejected("quoted empty name", &workflow(&empty));
}
