//! Real checkout, release-helper identity, and authenticated analysis fixtures.

#[path = "../../test_support/git_fixture.rs"]
mod git_fixture;

use super::*;
use crate::analysis_inventory::authority::{AuthenticatedAnalysis, RemoteAnalysisAuthority};
use crate::analysis_inventory::{
    AnalysisIdentity, AnalysisSource, build_payload, resolution_inputs_digest,
};
use velnor_actions_rust::{PackageRecord, TargetRecord, WorkspaceRecord};

pub(super) type TestResult = Result<(), Box<dyn std::error::Error>>;

pub(super) struct Fixture {
    pub(super) repo: tempfile::TempDir,
    pub(super) request: serde_json::Value,
    pub(super) identity: AnalysisIdentity,
    pub(super) text: String,
}

pub(super) fn git(root: &Path, args: &[&str]) -> Result<String, Box<dyn std::error::Error>> {
    let output = git_fixture::command(root)?
        .args(args)
        .env("GIT_AUTHOR_NAME", "Fixture")
        .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
        .env("GIT_COMMITTER_NAME", "Fixture")
        .env("GIT_COMMITTER_EMAIL", "fixture@example.test")
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    Ok(String::from_utf8(output.stdout)?.trim().to_owned())
}

pub(super) fn release_manifest(root: &Path) -> TestResult {
    let version = env!("CARGO_PKG_VERSION");
    let helper =
        crate::cover_identity::generator::current_exe_sha256().ok_or("helper unreadable")?;
    let targets = velnor_actions_contract::targets::SUPPORTED_TARGETS.iter().map(|target| {
        serde_json::json!({
            "target": target,
            "artifact": format!("https://github.com/tailrocks/velnor-new/releases/download/v{version}/velnor-actions-{version}-{target}"),
            "sha256": helper,
        })
    }).collect::<Vec<_>>();
    let value = serde_json::json!({"schema":1,"version":version,
        "repository":"tailrocks/velnor-new","commit":"c".repeat(40),"targets":targets});
    let text = serde_json::to_string(&value)?;
    ReleaseManifest::parse_json(&text, "fixture")?.validate("fixture")?;
    std::fs::write(root.join(".velnor/release-manifest.json"), text)?;
    Ok(())
}

fn record(root: &Path) -> WorkspaceRecord {
    let id = format!("path+file://{}#demo@0.1.0", root.display());
    WorkspaceRecord {
        workspace_root: String::new(),
        members: vec![id.clone()],
        packages: vec![PackageRecord {
            id,
            name: "demo".to_owned(),
            version: "0.1.0".to_owned(),
            manifest: "Cargo.toml".to_owned(),
            external: false,
            in_workspace: true,
            targets: vec![TargetRecord {
                kind: "lib".to_owned(),
                name: "demo".to_owned(),
                test: true,
                doctest: true,
                required_features: Vec::new(),
            }],
            features: Vec::new(),
            has_build_script: false,
        }],
        edges: Vec::new(),
        skipped_edges: Vec::new(),
    }
}

pub(super) fn fixture() -> Result<Fixture, Box<dyn std::error::Error>> {
    let repo = tempfile::tempdir()?;
    let root = repo.path().canonicalize()?;
    std::fs::create_dir_all(root.join(".velnor"))?;
    std::fs::create_dir_all(root.join("src"))?;
    std::fs::write(
        root.join(".velnor/config.toml"),
        "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"main\"\n",
    )?;
    std::fs::write(
        root.join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
    )?;
    std::fs::write(
        root.join("Cargo.lock"),
        "version = 4\n[[package]]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )?;
    std::fs::write(root.join("src/lib.rs"), "pub fn answer() -> u8 { 42 }\n")?;
    std::fs::write(root.join("README.md"), "Documentation.\n")?;
    release_manifest(&root)?;
    git(&root, &["init", "--initial-branch=main"])?;
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/demo.git",
        ],
    )?;
    git(&root, &["add", "."])?;
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "base"],
    )?;
    let base = git(&root, &["rev-parse", "HEAD"])?;
    let inventories = vec![("Cargo.toml".to_owned(), record(&root))];
    let (index, _) = build_file_index(&root, &[])?;
    let cargo_pin = ToolCatalog::pinned().rustup_toolchain().to_owned();
    let identity = AnalysisIdentity {
        helper_sha256: verified_helper(&root)?,
        cargo_identity: crate::analysis_inventory::QUALIFIED_CARGO_TEST_IDENTITY.to_owned(),
        cargo_pin,
        resolution_inputs_digest: resolution_inputs_digest(&root, index.files(), &inventories)?,
        source: AnalysisSource {
            repository: "example/demo".to_owned(),
            head_sha: base.clone(),
            workflow_sha: base.clone(),
            run_id: 1,
            run_attempt: 1,
            branch: "main".to_owned(),
        },
    };
    let text = build_payload(&root, identity.clone(), &inventories)?;
    let request = serde_json::json!({"schema":1,"op":"plan-v1","run_key":"r2-a1",
        "root":root,"repository":"example/demo","event":"local","scope":"affected",
        "base":base,"head":base});
    Ok(Fixture {
        repo,
        request,
        identity,
        text,
    })
}

impl Fixture {
    pub(super) fn commit_current(&mut self) -> TestResult {
        git(self.repo.path(), &["add", "."])?;
        git(
            self.repo.path(),
            &["-c", "commit.gpgsign=false", "commit", "-m", "generated"],
        )?;
        let head = git(self.repo.path(), &["rev-parse", "HEAD"])?;
        self.request["base"] = head.clone().into();
        self.request["head"] = head.clone().into();
        self.identity.source.head_sha = head.clone();
        self.identity.source.workflow_sha = head;
        self.refresh_analysis()
    }

    pub(super) fn refresh_analysis(&mut self) -> TestResult {
        let root = self.repo.path().canonicalize()?;
        let (index, _) = build_file_index(&root, &[])?;
        let inventories = vec![("Cargo.toml".to_owned(), record(&root))];
        self.identity.resolution_inputs_digest =
            resolution_inputs_digest(&root, index.files(), &inventories)?;
        self.text = build_payload(&root, self.identity.clone(), &inventories)?;
        Ok(())
    }

    pub(super) fn download(&self) -> AuthenticatedAnalysis {
        AuthenticatedAnalysis {
            text: self.text.clone(),
            authority: RemoteAnalysisAuthority::fixture(self.identity.clone(), &self.text),
        }
    }

    pub(super) fn prep(&self) -> Result<crate::prepare::GenerationPreparation, OrchestratorError> {
        let root = self
            .repo
            .path()
            .canonicalize()
            .map_err(|error| internal(&error.to_string()))?;
        let (index, _) = build_file_index(&root, &[])?;
        let download = self.download();
        let inventory =
            parse_authenticated(&root, index.files(), &download.text, &download.authority)
                .map_err(|problem| internal(&problem))?;
        prepare_with_inventory(&root, InventoryProvider::ValidatedInventory(&inventory))
    }

    pub(super) fn normal(&self) -> Result<String, OrchestratorError> {
        let (request, _) = resolved_request(&self.request.to_string(), crate::internal::PLAN_OP)?;
        crate::internal::plan_prepared(request, &self.prep()?)
    }

    pub(super) fn cover_request(&mut self) -> TestResult {
        let response: crate::internal::PlanResponse = serde_json::from_str(&self.normal()?)?;
        let plan = response.plan;
        let base = self.identity.source.head_sha.clone();
        let compat = crate::cover_compat::baseline_compat_for_plan(&plan)?;
        let name = crate::cover_baseline::lookup_artifact_name(&plan, &base)?;
        let tasks = plan.obligations.iter().map(|obligation| {
            let proof = task_proof(obligation, 1)?;
            Ok(serde_json::json!({
                "task_id":obligation.task_id,"task_digest":obligation.task_digest,
                "input_digest":obligation.input_digest,"closure_digest":obligation.closure_digest,
                "proof_run_id":1,"observed_run_id":1,"carried_from":null,"proof":proof,
            }))
        }).collect::<Result<Vec<_>, velnor_actions_contract::ContractError>>()?;
        self.request["baseline_manifest"] = serde_json::json!({
            "schema":crate::internal_plan::snapshot::CANONICAL_SCHEMA_VERSION,"repository_id":velnor_actions_contract::digest_b3(b"github.com/example/demo"),
            "source_commit":base,"ref":"refs/heads/main","event":"push",
            "workflow_ref":"example/demo/.github/workflows/ci.yml@refs/heads/main",
            "run_id":1,"run_attempt":1,"final_status":"passed",
            "generator_version":plan.generator.version,"generator_sha256":plan.generator.sha256,
            "compatibility_id":compat,"artifact_id":crate::cover_compat::baseline_artifact_numeric_id(&name),
            "artifact_name":name,"parent":null,"expires_at_unix":null,"tasks":tasks,
        });
        Ok(())
    }
}

/// Bind baseline proof to the actual planned execution dimensions.
pub(super) fn task_proof(
    obligation: &velnor_actions_contract::PlanObligation,
    run_id: u64,
) -> Result<velnor_actions_contract::ManifestTaskProof, velnor_actions_contract::ContractError> {
    let identity = &obligation.execution_identity;
    velnor_actions_contract::ManifestTaskProof::new(
        &obligation.task_id,
        &obligation.task_digest,
        &obligation.input_digest,
        identity.graph_digest(),
        identity.toolchain_id(),
        identity.mbx_digest(),
        identity.platform_id(),
        identity.profile(),
        run_id,
    )
}
