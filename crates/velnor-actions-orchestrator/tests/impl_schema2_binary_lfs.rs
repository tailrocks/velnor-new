//! The workflow-wide LFS setting must reach separately rendered native workflows.
use super::*;

const NATIVE_WORKFLOWS: [&str; 4] = [
    ".github/workflows/binary-preview.yml",
    ".github/workflows/binary-release.yml",
    ".github/workflows/preview-tap-reconcile.yml",
    ".github/workflows/stable-tap-reconcile.yml",
];

fn config() -> String {
    tap_enabled_config(true).replace(
        "inputs = [\"mise.toml\", \"Formula/demo-preview.rb\", \"scripts/package-update.sh\"]",
        "inputs = [\"mise.toml\", \"Formula/demo-preview.rb\", \"Formula/demo.rb\", \"scripts/package-update.sh\"]",
    ).replace(
        "conflicts = [] }]",
        "conflicts = [] }, { channel = \"stable\", formula = \"demo\", formula_class = \"Demo\", destination = \"Formula/demo.rb\", conflicts = [] }]",
    ) + "\n[distribution.homebrew_tap.destinations.stable]\nformula = \"Formula/demo.rb\"\n"
}

#[test]
fn native_preview_stable_and_reconcile_honor_workflow_checkout_lfs() -> TestResult {
    let mut baseline = None;
    for flag in [None, Some(false), Some(true)] {
        let config = flag.map_or_else(config, |enabled| {
            config().replace(
                "[workflow]\n",
                &format!("[workflow]\ncheckout_lfs = {enabled}\n"),
            )
        });
        let repo = tap_enabled_repo(&config)?;
        let tree = render_staged_tree(&prepare(repo.path())?)?;
        let mut native = Vec::new();
        for path in NATIVE_WORKFLOWS {
            let yaml = tree.get(path).ok_or("native workflow missing")?;
            assert!(
                yaml.contains("actions/checkout@"),
                "sourceful fixture: {path}"
            );
            if path == ".github/workflows/binary-preview.yml" {
                assert!(yaml.contains("lfs: ${{ steps.source-select.outputs.checkout_lfs }}"));
                assert!(
                    yaml.contains("lfs: ${{ needs.preview-eligibility.outputs.checkout_lfs }}")
                );
                assert!(!yaml.contains("lfs: \"true\""));
            } else {
                assert_eq!(
                    yaml.contains("lfs: \"true\""),
                    flag == Some(true),
                    "literal LFS propagation: {path}"
                );
            }
            native.push(yaml.to_owned());
        }
        if flag == Some(false) {
            assert_eq!(
                Some(&native),
                baseline.as_ref(),
                "explicit false preserves default native bytes"
            );
        } else if flag.is_none() {
            baseline = Some(native);
        }
    }
    Ok(())
}
