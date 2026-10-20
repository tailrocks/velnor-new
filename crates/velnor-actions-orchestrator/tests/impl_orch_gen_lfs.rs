//! `[workflow] checkout_lfs` generation case (split from `impl_orch_gen`).

use std::fs;

use tempfile::TempDir;
use velnor_actions_orchestrator::{GenerateOptions, generate, prepare};

use crate::impl_common::{TestResult, config_with_branch, make_repo};

/// `[workflow] checkout_lfs = true` adds `lfs: "true"` to every generated
/// checkout and the generated tree still validates; the default adds none.
#[test]
fn orch_gen_checkout_lfs_flag_marks_every_checkout() -> TestResult {
    let config = "schema = 1\n[workflow]\ndefault_branch = \"testmain\"\ncheckout_lfs = true\n";
    for (config, expect_lfs) in [(config, true), (config_with_branch(), false)] {
        let repo = make_repo(config)?;
        let prep = prepare(repo.path())?;
        let parent = TempDir::new()?;
        let preview = parent.path().join("preview");
        generate(
            &prep,
            &GenerateOptions {
                output_dir: Some(preview.clone()),
            },
        )?;
        let yaml = fs::read_to_string(preview.join(".github/workflows/ci.yml"))?;
        let checkouts = yaml.matches("actions/checkout@").count();
        let lfs = yaml.matches("lfs: \"true\"").count();
        assert!(checkouts > 0, "generated workflow has checkouts");
        assert_eq!(lfs, if expect_lfs { checkouts } else { 0 }, "{yaml}");
    }
    Ok(())
}
