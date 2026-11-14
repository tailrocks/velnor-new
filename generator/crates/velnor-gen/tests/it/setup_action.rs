// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::collections::BTreeSet;
use std::error::Error;
use std::fs;
use std::path::Path;

use velnor_render::SETUP_VELNOR_ACTION;

type TestResult = Result<(), Box<dyn Error>>;

const REPOSITORY: &str = "tailrocks/velnor-new/";
const INSTALL_STEP: &str = "      - name: Install velnor-ci";

fn indented_keys(text: &str, parent: &str, indent: usize) -> BTreeSet<String> {
    let pad = " ".repeat(indent);
    let header = format!("{pad}{parent}:");
    text.lines()
        .skip_while(|line| *line != header)
        .skip(1)
        .take_while(|line| line.starts_with(&format!("{pad} ")))
        .filter_map(|line| {
            let rest = line.strip_prefix(&format!("{pad}  "))?;
            let (key, _) = rest.split_once(':')?;
            (!key.starts_with(' ')).then(|| key.to_owned())
        })
        .collect()
}

#[test]
fn the_action_declares_exactly_the_inputs_the_generated_install_step_passes() -> TestResult {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let action_dir = SETUP_VELNOR_ACTION
        .strip_prefix(REPOSITORY)
        .ok_or("the setup action is not in this repository")?;
    let action = fs::read_to_string(root.join(action_dir).join("action.yml"))?;
    assert!(action.contains("using: composite"));

    let workflow = fs::read_to_string(
        root.join("generator/samples/helper-lock/expected/.github/workflows/rust-ci.yml"),
    )?;
    let step: String = workflow
        .lines()
        .skip_while(|line| !line.starts_with(INSTALL_STEP))
        .enumerate()
        .take_while(|(index, line)| *index == 0 || line.starts_with("        "))
        .flat_map(|(_, line)| [line, "\n"])
        .collect();
    assert!(
        step.contains(&format!("uses: {SETUP_VELNOR_ACTION}@")),
        "{step}"
    );

    let declared = indented_keys(&action, "inputs", 0);
    let passed = indented_keys(&step, "with", 8);
    assert_eq!(passed.len(), 4, "{passed:?}");
    assert_eq!(declared, passed);
    Ok(())
}
