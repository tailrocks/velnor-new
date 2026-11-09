// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::{workflow_timeout, yaml::Yaml};

const CALLER: &str = ".github/workflows/product-release-generator.yml";
const AUTHORITY: &str = "tailrocks/velnor-new/.github/workflows/source-preview-publication.yml@72701af69f6f9b693e639814fcf479409da3295c";

fn document(id: &str, reference: Option<Yaml>, extra: Vec<(String, Yaml)>) -> Yaml {
    let mut fields = Vec::new();
    if let Some(value) = reference {
        fields.push(("uses".to_owned(), value));
    }
    fields.extend(extra);
    Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(id.to_owned(), Yaml::Map(fields))]),
    )])
}

#[test]
fn protected_source_preview_uses_its_own_reviewed_budget() {
    let caller = document(
        "source-preview-publication",
        Some(Yaml::str(AUTHORITY)),
        vec![],
    );
    assert!(workflow_timeout::validate_tree(&[(CALLER, &caller)]).is_ok());
    let candidate = Yaml::Map(vec![(
        "jobs".to_owned(),
        Yaml::Map(vec![(
            "different".to_owned(),
            Yaml::Map(vec![("timeout-minutes".to_owned(), Yaml::Int(1))]),
        )]),
    )]);
    assert!(
        workflow_timeout::validate_tree(&[
            (CALLER, &caller),
            (
                ".github/workflows/source-preview-publication.yml",
                &candidate
            )
        ])
        .is_ok()
    );
}

#[test]
fn unrelated_or_ambiguous_reusable_jobs_fail_closed() {
    for (path, id, reference) in [
        (CALLER, "different", AUTHORITY.to_owned()),
        (
            ".github/workflows/other.yml",
            "source-preview-publication",
            AUTHORITY.to_owned(),
        ),
        (
            CALLER,
            "source-preview-publication",
            AUTHORITY.replace("tailrocks/", "other/"),
        ),
        (
            CALLER,
            "source-preview-publication",
            AUTHORITY.replace("source-preview-publication.yml@", "other.yml@"),
        ),
        (
            CALLER,
            "source-preview-publication",
            AUTHORITY.replace(
                "72701af69f6f9b693e639814fcf479409da3295c",
                "72701af69f6f9b693e639814fcf479409da3295d",
            ),
        ),
        (
            CALLER,
            "source-preview-publication",
            AUTHORITY.replace("72701af69f6f9b693e639814fcf479409da3295c", "main"),
        ),
        (
            CALLER,
            "source-preview-publication",
            AUTHORITY.replace(
                "72701af69f6f9b693e639814fcf479409da3295c",
                "${{ github.sha }}",
            ),
        ),
        (
            CALLER,
            "source-preview-publication",
            AUTHORITY.split('@').next().unwrap_or_default().to_owned(),
        ),
    ] {
        assert!(
            workflow_timeout::validate_tree(&[(
                path,
                &document(id, Some(Yaml::str(reference)), vec![])
            )])
            .is_err()
        );
    }
    let id = "source-preview-publication";
    assert!(workflow_timeout::validate_tree(&[(CALLER, &document(id, None, vec![]))]).is_err());
    assert!(
        workflow_timeout::validate_tree(&[(
            CALLER,
            &document(id, Some(Yaml::Map(vec![])), vec![])
        )])
        .is_err()
    );
    for field in ["steps", "runs-on", "timeout-minutes"] {
        assert!(
            workflow_timeout::validate_tree(&[(
                CALLER,
                &document(
                    id,
                    Some(Yaml::str(AUTHORITY)),
                    vec![(field.to_owned(), Yaml::str("ambiguous"))]
                )
            )])
            .is_err()
        );
    }
    let local = document(
        id,
        Some(Yaml::str("./.github/workflows/missing.yml")),
        vec![],
    );
    assert!(workflow_timeout::validate_tree(&[(CALLER, &local)]).is_err());
}
