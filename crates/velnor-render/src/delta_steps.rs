// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::release_steps::shell_word;
use crate::rules::{RenderedStep, Value};
use velnor_model::{CacheRoot, PackageName};

pub(crate) fn delta_step(root: &CacheRoot, verb: &str) -> RenderedStep {
    let slug = root.slug();
    let (id, args) = match verb {
        "prepare" => (Some(format!("delta_base_{slug}")), paths(root)),
        "stage" => (Some("delta".to_owned()), paths(root)),
        _ => (None, paths(root)),
    };
    RenderedStep {
        id,
        name: format!("velnor-ci delta {verb}"),
        env: vec![(
            "BASE_CACHE_KEY".to_owned(),
            Value::Expr(format!(
                "${{{{ steps.target_{slug}.outputs.cache-matched-key }}}}"
            )),
        )],
        run: Some(format!("velnor-ci delta {verb}{args}")),
        ..RenderedStep::default()
    }
}

fn paths(root: &CacheRoot) -> String {
    format!(
        " --target-dir {} --staging-dir {}",
        shell_word(root.target_dir().as_str()),
        shell_word(&root.delta_dir())
    )
}

pub(crate) fn delta_stage(root: &CacheRoot, packages: &[PackageName]) -> RenderedStep {
    let mut step = delta_step(root, "stage");
    step.env.push((
        "EXPECTED_SOURCE_IDENTITY".to_owned(),
        Value::Expr(format!(
            "${{{{ steps.delta_base_{}.outputs.source_identity }}}}",
            root.slug()
        )),
    ));
    if !packages.is_empty() {
        let names: Vec<&str> = packages.iter().map(PackageName::as_str).collect();
        step.env.push((
            "VELNOR_DELTA_PACKAGES".to_owned(),
            Value::Text(names.join("\n")),
        ));
    }
    step
}
