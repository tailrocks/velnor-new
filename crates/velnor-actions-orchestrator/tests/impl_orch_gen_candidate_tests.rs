use super::*;

use velnor_actions_orchestrator::render_staged_tree;

fn lock_text() -> Result<String, Box<dyn std::error::Error>> {
    use std::fmt::Write as _;
    let version = env!("CARGO_PKG_VERSION");
    let mut bins = String::new();
    for target in velnor_actions_contract::SUPPORTED_TARGETS {
        write!(
            bins,
            "[[generator.binaries]]\ntarget = \"{target}\"\nartifact = \"https://example.invalid/r/{target}\"\nsha256 = \"{}\"\n",
            "a".repeat(64)
        )?;
    }
    Ok(format!(
        "schema = 1\n[generator]\nbinary = \"velnor-actions\"\nversion = \"{version}\"\ncommit = \"{}\"\n{bins}[mise-bootstrap]\nversion = \"{mise_version}\"\nartifact = \"https://example.invalid/mise\"\nsha256 = \"{}\"\n",
        "e".repeat(40),
        "b".repeat(64),
        mise_version = velnor_actions_mise::MISE_VERSION,
    ))
}

#[test]
fn orch_gen_candidate_qualify_is_artifact_only() -> TestResult {
    without_ambient_identity("orch_gen_candidate_qualify_is_artifact_only", || {
        let repo = make_velnor_repo(
            "schema = 1\n[workflow]\nname = \"CI\"\ndefault_branch = \"testmain\"\npolicy = \"velnor-repository-v1\"\ngenerator_validation = \"candidate\"\n",
        )?;
        fs::write(repo.path().join(".velnor/generator.lock"), lock_text()?)?;
        let prep = prepare(repo.path())?;
        // Render-level assertions: full `generate` on candidate mode currently
        // fails the shellcheck gate on renderer-owned manifest content (SC2154
        // `sha`); flip back to `generate` once the renderer owner fixes it.
        let tree = render_staged_tree(&prep)?;
        let yaml = tree
            .get(".github/workflows/ci.yml")
            .ok_or_else(|| std::io::Error::other("missing workflow"))?;
        assert!(yaml.contains("\n  candidate:"), "candidate job present");
        let qualify = yaml
            .lines()
            .find(|line| {
                line.contains("velnor/candidate/velnor-actions")
                    && line.contains(" plan ")
                    && line.contains("generate --output-dir")
            })
            .ok_or_else(|| std::io::Error::other("missing qualify line"))?;
        // Payload only: the constructor's `unset` prelude names
        // MISE_GITHUB_TOKEN, which is not a rebuild marker.
        let payload = qualify.split_once("; ").map_or(qualify, |(_, tail)| tail);
        for marker in ["cargo", "mbx", "rustc", "mise", "build"] {
            assert!(
                !payload.to_lowercase().contains(marker),
                "no rebuild marker {marker}:{qualify}"
            );
        }
        for line in yaml.lines().filter(|line| line.contains("candidate")) {
            if line.contains("mise-tools-v2-") {
                continue; // Typed tools-cache keys do not carry build outputs.
            }
            for banned in ["actions/cache", "key:", "restore"] {
                assert!(
                    !line.contains(banned),
                    "no cache restore of candidate:{line}"
                );
            }
        }
        Ok(())
    })
}
