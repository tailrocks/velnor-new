//! F2 follow-up scans: reuse tokens, retries, publish verbs, offline deps.
//!
//! Split from `impl_orch_f2a.rs` by the 400-line repo-size gate.

use std::path::PathBuf;

use crate::impl_common::{TestResult, passing_reports, plan_for_source_change};
use crate::impl_orch_f2a::{code_of_text, product_src_files, runtime_acquisition, token_hits};
use crate::impl_orch_f2a::{is_exact_runtime_source, mask_api_reexport_for_legacy_scan};
use crate::impl_orch_plansel::merge_status;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EvidenceModuleMount {
    name: String,
    path: Option<String>,
    visibility: String,
    external: bool,
}

type EvidenceMount = (&'static str, &'static str, &'static str);
type EvidenceMountSet = (&'static str, Vec<EvidenceMount>);

#[derive(Default)]
struct EvidenceModuleMounts(Vec<EvidenceModuleMount>);

impl<'ast> syn::visit::Visit<'ast> for EvidenceModuleMounts {
    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        let path = module.attrs.iter().find_map(|attribute| {
            if !attribute.path().is_ident("path") {
                return None;
            }
            let syn::Meta::NameValue(value) = &attribute.meta else {
                return Some("<invalid>".to_owned());
            };
            let syn::Expr::Lit(literal) = &value.value else {
                return Some("<invalid>".to_owned());
            };
            let syn::Lit::Str(path) = &literal.lit else {
                return Some("<invalid>".to_owned());
            };
            Some(path.value())
        });
        let visibility = match &module.vis {
            syn::Visibility::Inherited => "private".to_owned(),
            syn::Visibility::Public(_) => "public".to_owned(),
            syn::Visibility::Restricted(value) => format!(
                "restricted:{}",
                value
                    .path
                    .segments
                    .iter()
                    .map(|part| part.ident.to_string())
                    .collect::<Vec<_>>()
                    .join("::")
            ),
        };
        self.0.push(EvidenceModuleMount {
            name: module.ident.to_string(),
            path,
            visibility,
            external: module.content.is_none(),
        });
        syn::visit::visit_item_mod(self, module);
    }
}

/// Pin every production mount in the evidence runtime closure.
pub(crate) fn validate_evidence_module_mounts(
    root: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    for (file, expected) in evidence_mount_sets() {
        let source = std::fs::read_to_string(root.join(file))?;
        let production = crate::impl_orch_f2a_offline::production_source_of_text(&source)?;
        let syntax = syn::parse_file(&production)?;
        let mut mounts = EvidenceModuleMounts::default();
        syn::visit::Visit::visit_file(&mut mounts, &syntax);
        mounts.0.sort();
        let mut expected = expected
            .iter()
            .map(|(name, path, visibility)| EvidenceModuleMount {
                name: (*name).to_owned(),
                path: Some((*path).to_owned()),
                visibility: (*visibility).to_owned(),
                external: true,
            })
            .collect::<Vec<_>>();
        expected.sort();
        if mounts.0 != expected {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("evidence module mounts in {file} changed"),
            )
            .into());
        }
    }
    Ok(())
}

fn evidence_mount_sets() -> Vec<EvidenceMountSet> {
    vec![
        (
            "api.rs",
            vec![
                ("github_evidence", "github_evidence.rs", "private"),
                (
                    "github_evidence_http",
                    "github_evidence_http.rs",
                    "restricted:crate",
                ),
            ],
        ),
        (
            "github_evidence.rs",
            vec![
                ("collect", "github_evidence_collect.rs", "private"),
                ("import", "github_evidence_import.rs", "private"),
            ],
        ),
        (
            "github_evidence_collect.rs",
            vec![
                ("api", "github_evidence_collect_api.rs", "private"),
                (
                    "artifacts",
                    "github_evidence_collect_artifacts.rs",
                    "private",
                ),
                ("bundle", "github_evidence_collect_bundle.rs", "private"),
                ("context", "github_evidence_collect_context.rs", "private"),
                ("time", "github_evidence_collect_time.rs", "private"),
                ("validate", "github_evidence_collect_validate.rs", "private"),
                (
                    "workflows",
                    "github_evidence_collect_workflows.rs",
                    "private",
                ),
            ],
        ),
        (
            "github_evidence_http.rs",
            vec![("route", "github_evidence_http_route.rs", "private")],
        ),
        (
            "github_evidence_import.rs",
            vec![("files", "github_evidence_import_files.rs", "private")],
        ),
    ]
}

/// `TaskStatus` variants in declaration order.
fn task_status_variants() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../velnor-actions-contract/src/workflow/report.rs");
    let text = std::fs::read_to_string(&path)?;
    let mut variants = Vec::new();
    let mut in_enum = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "pub enum TaskStatus {" {
            in_enum = true;
            continue;
        }
        if in_enum {
            if trimmed == "}" {
                break;
            }
            if let Some(name) = trimmed.strip_suffix(',').filter(|name| {
                name.chars()
                    .next()
                    .is_some_and(|ch| ch.is_ascii_uppercase())
                    && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
            }) {
                variants.push(name.to_owned());
            }
        }
    }
    Ok(variants)
}

#[test]
fn reference_copies_neither_omissions_nor_retries() -> TestResult {
    for token in [
        "retry_pass",
        "first_attempt",
        "slow_test",
        "excluded_slow",
        "skip_slow",
        "omitted_test",
    ] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token}: {:?}",
            token_hits(token)?
        );
    }
    assert_eq!(
        task_status_variants()?,
        [
            "Reused",
            "Executed",
            "EmptyPartition",
            "NotSelected",
            "Failed",
            "Cancelled"
        ]
    );
    Ok(())
}

#[test]
fn nonzero_retries_rejected_while_retry_state_absent() -> TestResult {
    assert!(
        !task_status_variants()?
            .iter()
            .any(|name| name.contains("Retry")),
        "no retry-passed state while retries are disabled"
    );
    let (_repo, plan) = plan_for_source_change()?;
    let reports = passing_reports(&plan)?;
    let extra = serde_json::json!({"limits": {"compiler_budget": 2, "test_budget": 2, "max_parallel": 2, "capacity": 8, "shards": 1, "retries": 1}});
    assert_eq!(
        merge_status(&plan, &reports, &[], &extra)?,
        velnor_actions_contract::FinalStatus::PlanningFailed
    );
    Ok(())
}

#[test]
fn merge_consumes_no_publish_verbs() -> TestResult {
    for token in ["release_upload", "put_object", "gh api", "workflows/upload"] {
        assert!(
            token_hits(token)?.is_empty(),
            "{token}: {:?}",
            token_hits(token)?
        );
    }
    Ok(())
}

/// Analysis, planning and generation never call the credentialed fetch: only
/// the module mount, public re-export, and runtime acquisition modules name it.
#[test]
fn private_fetch_is_unreachable_from_analysis() -> TestResult {
    for path in product_src_files()? {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if matches!(
            name.as_str(),
            "private_fetch.rs"
                | "bun_registry_install.rs"
                | "private_fetch_http.rs"
                | "private_fetch_root.rs"
                | "private_fetch_session.rs"
                | "private_fetch_owned_env.rs"
                | "private_revoke.rs"
                | "bun_assets_handoff.rs"
                | "api.rs"
                | "config.rs"
        ) {
            continue;
        }
        let source = std::fs::read_to_string(&path)?;
        assert!(
            !source.contains("private_fetch") && !source.contains("run_private_"),
            "{} reaches the private fetch outside its operation",
            path.display()
        );
    }
    Ok(())
}

#[test]
fn offline_deps_fail_closed_without_fetch() -> TestResult {
    let source_files = product_src_files()?;
    assert!(
        source_files
            .iter()
            .any(|path| path.file_name().is_some_and(|name| name == "vectors.rs")),
        "production vectors remain under the offline/fetch scan"
    );
    assert!(
        source_files
            .iter()
            .all(|path| path.file_name().is_none_or(|name| {
                !name.to_string_lossy().ends_with("_tests.rs")
                    && !name.to_string_lossy().ends_with("_fixtures.rs")
            })),
        "test-only companions must not enter product scans"
    );
    for path in source_files {
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        if is_exact_runtime_source(&path, &crate::impl_orch_f2a::orch_src())? {
            continue;
        }
        let source = std::fs::read_to_string(&path)?;
        let source =
            mask_api_reexport_for_legacy_scan(&path, &crate::impl_orch_f2a::orch_src(), &source)?;
        let source = runtime_acquisition::mask_resolved_evidence_spec_for_legacy_scan(
            &path,
            &crate::impl_orch_f2a::orch_src(),
            &source,
        )?;
        // HD-7: the credentialed private-source fetch is a dedicated runtime
        // operation (`private-fetch-v1`) behind its own CLI gate; its three
        // modules the Bun registry phase (`bun-registry-install-v1`) and the Playwright browser install
        // (`playwright-browsers-install-v1`, with its system-dependency half) are the only product
        // files exempt from the fetch-verb scan,
        // and `private_fetch_is_unreachable_from_analysis` pins that nothing
        // else in the crate calls into them.
        if matches!(
            name.as_str(),
            "private_fetch.rs"
                | "private_fetch_session.rs"
                | "private_fetch_owned_env.rs"
                | "private_revoke.rs"
                | "bun_registry_install.rs"
                | "playwright_install.rs"
                | "playwright_system_deps.rs"
        ) {
            continue;
        }
        for (line, code) in code_of_text(&source) {
            if name == "source_prep.rs" {
                assert!(
                    !code.contains(".run("),
                    "source_prep emits fetch steps, never executes: {}:{line}",
                    path.display()
                );
                continue;
            }
            let scrubbed = runtime_acquisition::scrub_bound_acquisition(&name, &code)
                .replace("fetch_inventory", "")
                .replace("FetchFailure", "")
                .replace("fetch_add", "")
                // Gate-1 emission threading: plan-job `cargo fetch` step
                // builders plus their lockful-root inputs. The argv literal
                // lives in source_prep.rs (scoped above, execution-free);
                // analysis-time fetching stays forbidden.
                .replace("fetch_steps", "")
                // HD-7 operation names re-exported by `api.rs` and mounted
                // under `config.rs`; the modules themselves are exempt above.
                .replace("private_fetch", "")
                .replace("PRIVATE_FETCH", "")
                .replace("fetch_roots", "")
                // Plan checkout input emission: `fetch-depth: 0` is a
                // workflow input literal (history for HEAD^2 + base diff),
                // never an analysis-time fetch execution.
                .replace("fetch-depth", "")
                // Contract error-code literal asserted by cache-key
                // rejection tests, never an execution.
                .replace("unsafe_fetch_root", "");
            assert!(
                !scrubbed.contains("fetch"),
                "fetch verb at {}:{line}: {code}",
                path.display()
            );
        }
    }
    Ok(())
}
