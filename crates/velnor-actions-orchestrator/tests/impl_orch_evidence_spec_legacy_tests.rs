//! Canonical and hostile fixtures for the passive evidence-spec scanner mask.

use std::error::Error;
use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::impl_common::TestResult;

#[path = "impl_orch_evidence_spec_legacy_doc_tests.rs"]
mod doc_tests;
#[path = "impl_orch_evidence_spec_legacy_owner_mutation_tests.rs"]
mod owner_mutation_tests;
#[path = "impl_orch_evidence_spec_legacy_path_tests.rs"]
mod path_tests;

const ACTUAL_SOURCE: &str = include_str!("../src/mise_task_suites_emit.rs");

const MINIMAL_SOURCE: &str = r#"
pub(crate) fn suite_workflow_files(prep: &GenerationPreparation) -> Result<Vec<RenderedFile>, OrchestratorError> {
    let mut suites = Vec::new();
    for suite in &prep.config.workflow.task_suites {
        let evidence = if let Some(input) = &suite.evidence_snapshot {
            let fetch_spec = input.resolve_spec(
                &prep.config.workflow.task_suites,
                &suite.id,
                &prep.default_branch,
            )?;
            let helper = crate::pins::consumer_evidence_helper(
                suite.runner.runs_on(),
                env!("CARGO_PKG_VERSION"),
                &prep.discovery,
            )?;
            Some((fetch_spec, helper))
        } else {
            None
        };
        suites.push(MiseTaskSuitePolicy {
            suite: suite.clone(),
            evidence_fetch_spec: evidence.as_ref().map(|(spec, _)| spec.clone()),
            evidence_helper: evidence.map(|(_, helper)| helper),
        });
    }
    render_mise_task_suites(&spec)
}
"#;

struct Fixture {
    temp: TempDir,
    root: PathBuf,
    source_path: PathBuf,
}

fn fixture(source: &str) -> Result<Fixture, Box<dyn Error>> {
    let temp = tempfile::tempdir()?;
    let root = temp.path().join("src");
    fs::create_dir_all(&root)?;
    let source_path = root.join("mise_task_suites_emit.rs");
    fs::write(&source_path, source)?;
    Ok(Fixture {
        temp,
        root: root.canonicalize()?,
        source_path: source_path.canonicalize()?,
    })
}

fn mask(source: &str) -> Result<(Fixture, String), Box<dyn Error>> {
    let fixture = fixture(source)?;
    let masked = super::mask_resolved_evidence_spec_for_legacy_scan(
        &fixture.source_path,
        &fixture.root,
        source,
    )?;
    Ok((fixture, masked))
}

fn assert_three_passive_spans(source: &str, masked: &str) {
    assert_eq!(source.len(), masked.len());
    assert_eq!(
        source.bytes().filter(|byte| *byte == b'\n').count(),
        masked.bytes().filter(|byte| *byte == b'\n').count()
    );
    let mut spans = Vec::new();
    let mut start = None;
    for (index, (before, after)) in source.bytes().zip(masked.bytes()).enumerate() {
        if before != after {
            assert_eq!(after, b' ');
            start.get_or_insert(index);
        } else if let Some(begin) = start.take() {
            spans.push((begin, index));
        }
    }
    if let Some(begin) = start {
        spans.push((begin, source.len()));
    }
    let mut lengths = spans
        .iter()
        .map(|(begin, end)| end - begin)
        .collect::<Vec<_>>();
    lengths.sort_unstable();
    assert_eq!(
        lengths,
        vec![
            "fetch_spec".len(),
            "fetch_spec".len(),
            "evidence_fetch_spec".len()
        ]
    );
    for (begin, end) in spans {
        assert!(matches!(
            &source[begin..end],
            "fetch_spec" | "evidence_fetch_spec"
        ));
        assert!(masked[begin..end].bytes().all(|byte| byte == b' '));
    }
}

fn replace_once(source: &str, before: &str, after: &str) -> String {
    assert_eq!(
        source.matches(before).count(),
        1,
        "fixture marker must be unique: {before}"
    );
    source.replacen(before, after, 1)
}

fn rejected(source: &str) -> TestResult {
    let candidate = fixture(source)?;
    assert!(
        super::mask_resolved_evidence_spec_for_legacy_scan(
            &candidate.source_path,
            &candidate.root,
            source,
        )
        .is_err(),
        "accepted changed passive evidence shape"
    );
    Ok(())
}

fn legacy_fetch_hits(source: &str) -> bool {
    crate::impl_orch_f2a::code_of_text(source)
        .iter()
        .any(|(_, code)| {
            crate::impl_orch_f2a::runtime_acquisition::scrub_bound_acquisition(
                "mise_task_suites_emit.rs",
                code,
            )
            .replace("fetch_inventory", "")
            .replace("FetchFailure", "")
            .replace("fetch_add", "")
            .replace("fetch_steps", "")
            .replace("private_fetch", "")
            .replace("PRIVATE_FETCH", "")
            .replace("fetch_roots", "")
            .replace("fetch-depth", "")
            .replace("unsafe_fetch_root", "")
            .contains("fetch")
        })
}

fn mask_expected_passives(source: &str) -> Result<String, Box<dyn Error>> {
    let binder_start = source
        .find("let fetch_spec =")
        .map(|index| index + "let ".len())
        .ok_or_else(|| test_error("missing fixture binder marker"))?;
    let tuple_start = source
        .find("Some((fetch_spec, helper))")
        .map(|index| index + "Some((".len())
        .ok_or_else(|| test_error("missing fixture tuple marker"))?;
    let field_start = source
        .find("evidence_fetch_spec:")
        .ok_or_else(|| test_error("missing fixture field marker"))?;
    let mut bytes = source.as_bytes().to_vec();
    for (start, token) in [
        (binder_start, "fetch_spec"),
        (tuple_start, "fetch_spec"),
        (field_start, "evidence_fetch_spec"),
    ] {
        let end = start + token.len();
        assert_eq!(source.get(start..end), Some(token));
        bytes[start..end].fill(b' ');
    }
    Ok(String::from_utf8(bytes)?)
}

#[test]
fn actual_and_minimal_canonical_sources_mask_only_three_passive_spans() -> TestResult {
    for source in [ACTUAL_SOURCE, MINIMAL_SOURCE] {
        let (_fixture, masked) = mask(source)?;
        assert_eq!(masked, mask_expected_passives(source)?);
        assert_three_passive_spans(source, &masked);
        assert!(!legacy_fetch_hits(&masked));
    }
    Ok(())
}

#[test]
fn comments_spacing_and_trailing_commas_keep_the_same_three_spans() -> TestResult {
    let source = ACTUAL_SOURCE
        .replace("let fetch_spec =", "let /* stable comment */ fetch_spec =")
        .replace(
            "Some((fetch_spec, helper))",
            "Some( ( fetch_spec , helper , ) )",
        )
        .replace("evidence_fetch_spec:", "evidence_fetch_spec :");
    let (_fixture, masked) = mask(&source)?;
    assert_three_passive_spans(&source, &masked);
    assert!(!legacy_fetch_hits(&masked));
    Ok(())
}

#[test]
fn same_line_neighboring_executable_fetch_tokens_remain_visible() -> TestResult {
    for side_effect in [
        "fetch(); suite.clone()",
        "reader.fetch(); suite.clone()",
        "std::process::Command::new(\"cargo\").arg(\"fetch\"); suite.clone()",
        "fetch!(); suite.clone()",
        "let _ = run_github_evidence_fetch; suite.clone()",
    ] {
        let source = compact_policy_with_suite_value(ACTUAL_SOURCE, side_effect)?;
        let (_fixture, masked) = mask(&source)?;
        assert_three_passive_spans(&source, &masked);
        assert!(
            legacy_fetch_hits(&masked),
            "lost executable token: {side_effect}"
        );
        assert!(masked.contains(side_effect));
        assert!(
            crate::impl_orch_f2a::code_of_text(&masked)
                .iter()
                .any(|(_, line)| {
                    line.contains("suites.push(MiseTaskSuitePolicy") && line.contains(side_effect)
                }),
            "neighboring fetch was not retained on the compact constructor line"
        );
    }
    Ok(())
}

#[test]
fn callable_fetch_reference_before_and_after_passive_spans_remains_visible() -> TestResult {
    let before = replace_once(
        ACTUAL_SOURCE,
        "    for suite in &prep.config.workflow.task_suites {",
        "    let _ = run_github_evidence_fetch;\n    for suite in &prep.config.workflow.task_suites {",
    );
    let after = replace_once(
        ACTUAL_SOURCE,
        "    let spec = MiseTaskSuitesRenderSpec {",
        "    let _ = run_github_evidence_fetch;\n    let spec = MiseTaskSuitesRenderSpec {",
    );
    for source in [before, after] {
        let (_fixture, masked) = mask(&source)?;
        assert_three_passive_spans(&source, &masked);
        assert!(legacy_fetch_hits(&masked));
        assert!(masked.contains("run_github_evidence_fetch"));
    }
    Ok(())
}

fn compact_policy_with_suite_value(
    source: &str,
    suite_value: &str,
) -> Result<String, Box<dyn Error>> {
    let start = source
        .find("suites.push(MiseTaskSuitePolicy {")
        .ok_or_else(|| test_error("missing policy constructor"))?;
    let relative_end = source[start..]
        .find("\n        });")
        .ok_or_else(|| test_error("missing policy constructor end"))?;
    let end = start + relative_end + "\n        });".len();
    let policy = source[start..end].replace('\n', " ");
    let policy = replace_once(
        &policy,
        "suite: suite.clone()",
        &format!("suite: {{ {suite_value} }}"),
    );
    Ok(format!("{}{}{}", &source[..start], policy, &source[end..]))
}

fn test_error(message: &'static str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, message)
}

#[test]
fn canonical_parse_and_duplicate_passive_tokens_fail_closed() -> TestResult {
    rejected("pub fn suite_workflow_files(")?;
    let duplicate = replace_once(
        ACTUAL_SOURCE,
        "let helper = crate::pins::consumer_evidence_helper(",
        "let duplicate = fetch_spec;\n            let helper = crate::pins::consumer_evidence_helper(",
    );
    rejected(&duplicate)?;

    let duplicate_field = replace_once(
        ACTUAL_SOURCE,
        "evidence_helper: evidence.map(|(_, helper)| helper),",
        "evidence_fetch_spec: evidence.as_ref().map(|(spec, _)| spec.clone()),\n            evidence_helper: evidence.map(|(_, helper)| helper),",
    );
    rejected(&duplicate_field)?;

    let missing_field = replace_once(
        ACTUAL_SOURCE,
        "            evidence_fetch_spec: evidence.as_ref().map(|(spec, _)| spec.clone()),\n",
        "",
    );
    rejected(&missing_field)?;

    let extra_field_use = replace_once(
        ACTUAL_SOURCE,
        "let resolved =",
        "let _extra = evidence_fetch_spec;\n        let resolved =",
    );
    rejected(&extra_field_use)?;

    let duplicate_function = format!("{ACTUAL_SOURCE}\n{ACTUAL_SOURCE}");
    rejected(&duplicate_function)?;
    Ok(())
}
