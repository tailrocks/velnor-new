// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::error::Error;
use std::io;

use velnor_actions_orchestrator::{
    CacheRecipe, CacheRecipeKind, PREVIEW_CACHE_RECIPE_KEY, PREVIEW_PRODUCER_RECIPE_KEY,
    all_recipes, lookup_recipe, parse_kind, recipes_of, validate_catalog, validate_catalog_slice,
    validate_recipe,
};

fn require_recipe(key: &str) -> Result<CacheRecipe, Box<dyn Error>> {
    lookup_recipe(key)
        .copied()
        .ok_or_else(|| io::Error::other(format!("recipe catalog omits {key}")).into())
}

fn require_argv(key: &str) -> Result<Vec<String>, Box<dyn Error>> {
    let recipe = require_recipe(key)?;
    recipe
        .command_argv
        .map(|argv| argv.iter().map(|arg| (*arg).to_owned()).collect())
        .ok_or_else(|| io::Error::other(format!("recipe {key} has no fixed command")).into())
}

fn check_union() -> Result<(), Box<dyn Error>> {
    validate_catalog()?;
    if all_recipes().len() != 14 {
        return Err(io::Error::other(format!(
            "recipe catalog holds {} recipes, want 14",
            all_recipes().len()
        ))
        .into());
    }
    for kind in CacheRecipeKind::ALL {
        if recipes_of(kind).is_empty() {
            return Err(
                io::Error::other(format!("recipe catalog omits kind {}", kind.as_str())).into(),
            );
        }
        if parse_kind(kind.as_str()) != Some(kind) {
            return Err(io::Error::other(format!(
                "recipe kind {} does not round-trip",
                kind.as_str()
            ))
            .into());
        }
    }
    if parse_kind("release").is_some() {
        return Err(io::Error::other("unknown recipe kind parses").into());
    }
    Ok(())
}

fn check_native() -> Result<(), Box<dyn Error>> {
    let host = require_argv("native.cli.linux-x86_64")?;
    if host
        != vec![
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
        ]
    {
        return Err(io::Error::other("native host recipe argv drifted").into());
    }
    let cross = require_argv("native.cli.macos-x86_64-cross")?;
    if cross
        != vec![
            "build",
            "--release",
            "--locked",
            "--package",
            "velnor-actions-cli",
            "--bin",
            "velnor-actions",
            "--target",
            "x86_64-apple-darwin",
        ]
    {
        return Err(io::Error::other("intel cross recipe argv drifted").into());
    }
    let cross_recipe = require_recipe("native.cli.macos-x86_64-cross")?;
    if cross_recipe.targets != ["x86_64-apple-darwin"] {
        return Err(io::Error::other("intel cross recipe target drifted").into());
    }
    if cross_recipe.output_roles != ["target/x86_64-apple-darwin/release"] {
        return Err(io::Error::other("intel cross recipe output root drifted").into());
    }
    Ok(())
}

fn check_stable() -> Result<(), Box<dyn Error>> {
    let runner = require_argv("stable.runner.host")?;
    if !runner.contains(&"crates/velnor-runner/Cargo.toml".to_owned())
        || !runner.contains(&"velnor-host".to_owned())
    {
        return Err(io::Error::other("runner host recipe argv drifted").into());
    }
    let musl = require_recipe("stable.resource-probe.musl")?;
    if musl.targets != ["x86_64-unknown-linux-musl"] {
        return Err(io::Error::other("resource probe musl target drifted").into());
    }
    if musl.output_roles != ["target/x86_64-unknown-linux-musl/release"] {
        return Err(io::Error::other("resource probe musl output missed its target dir").into());
    }
    let gnu = require_recipe("stable.resource-probe-release.gnu")?;
    if gnu.targets != ["x86_64-unknown-linux-gnu"]
        || gnu.output_roles != ["target/x86_64-unknown-linux-gnu/debug"]
    {
        return Err(io::Error::other("resource probe gnu output missed its target dir").into());
    }
    let nextest = require_recipe("stable.resource-probe-release.nextest")?;
    if nextest.command_family != "test" || nextest.nextest_profile != "default" {
        return Err(io::Error::other("resource probe nextest recipe identity drifted").into());
    }
    if nextest.output_roles != ["target/x86_64-unknown-linux-gnu/debug"] {
        return Err(io::Error::other("resource probe nextest output missed its target dir").into());
    }
    let nextest_argv = require_argv("stable.resource-probe-release.nextest")?;
    if !nextest_argv.contains(&"resource_probe_release_public".to_owned())
        || !nextest_argv.contains(&"--no-tests".to_owned())
        || !nextest_argv.contains(&"fail".to_owned())
    {
        return Err(io::Error::other("resource probe nextest argv drifted").into());
    }
    Ok(())
}

fn check_preview_helpers() -> Result<(), Box<dyn Error>> {
    let preview = require_recipe(PREVIEW_CACHE_RECIPE_KEY)?;
    if preview.command_argv.is_some() {
        return Err(io::Error::other("preview producer command is config-supplied").into());
    }
    for name in [
        "VELNOR_RELEASE_SOURCE_REF",
        "VELNOR_RELEASE_COMMIT_COUNT",
        "CARGO_BUILD_JOBS",
    ] {
        if !preview.environment_inputs.contains(&name) {
            return Err(
                io::Error::other(format!("preview producer lost handoff input {name}")).into(),
            );
        }
    }
    if PREVIEW_PRODUCER_RECIPE_KEY != "preview.producer.task"
        || PREVIEW_CACHE_RECIPE_KEY != PREVIEW_PRODUCER_RECIPE_KEY
    {
        return Err(io::Error::other("preview producer recipe key drifted").into());
    }
    let preseed = require_recipe("helper.preseed.local-build")?;
    if !preseed.source_bound_helper || preseed.output_roles != ["target/release/velnor-actions"] {
        return Err(io::Error::other("preseed helper identity drifted").into());
    }
    let staged = require_recipe("helper.native-image.actions-binary")?;
    if !staged.source_bound_helper
        || !staged
            .environment_inputs
            .contains(&"VELNOR_NATIVE_IMAGE_ACTIONS")
    {
        return Err(io::Error::other("native image helper identity drifted").into());
    }
    Ok(())
}

fn check_fixed_commands() -> Result<(), Box<dyn Error>> {
    for recipe in all_recipes() {
        if recipe.command_builder_id.is_empty() || recipe.lane_id.is_empty() {
            return Err(io::Error::other(format!(
                "recipe {} lost its owner binding",
                recipe.recipe_key
            ))
            .into());
        }
        if recipe.command_argv.is_some() {
            if recipe.package.is_empty() || recipe.cargo_profile != "release" {
                return Err(io::Error::other(format!(
                    "recipe {} lost its profile or package",
                    recipe.recipe_key
                ))
                .into());
            }
            if !recipe.default_features || !recipe.features.is_empty() {
                return Err(io::Error::other(format!(
                    "recipe {} merged feature semantics",
                    recipe.recipe_key
                ))
                .into());
            }
        }
    }
    Ok(())
}

#[test]
fn gc1_t07_preserves_contract() -> Result<(), Box<dyn Error>> {
    check_union()?;
    check_native()?;
    check_stable()?;
    check_preview_helpers()?;
    check_fixed_commands()?;
    Ok(())
}

fn invalid_recipe_cases(valid: CacheRecipe) -> [(&'static str, CacheRecipe); 9] {
    [
        (
            "empty key",
            CacheRecipe {
                recipe_key: "",
                ..valid
            },
        ),
        (
            "unknown family",
            CacheRecipe {
                command_family: "release",
                ..valid
            },
        ),
        (
            "empty builder",
            CacheRecipe {
                command_builder_id: "",
                ..valid
            },
        ),
        (
            "empty lane",
            CacheRecipe {
                lane_id: "",
                ..valid
            },
        ),
        (
            "empty argv",
            CacheRecipe {
                command_argv: Some(&[]),
                ..valid
            },
        ),
        (
            "route without command",
            CacheRecipe {
                command_argv: None,
                tool_route: "mise-exec[rust,mr-boxington]:mbx",
                package: "",
                cargo_profile: "",
                ..valid
            },
        ),
        (
            "unsorted targets",
            CacheRecipe {
                targets: &["x86_64-apple-darwin", "aarch64-apple-darwin"],
                ..valid
            },
        ),
        (
            "duplicate features",
            CacheRecipe {
                features: &["serde", "serde"],
                ..valid
            },
        ),
        (
            "empty outputs",
            CacheRecipe {
                output_roles: &[],
                ..valid
            },
        ),
    ]
}

fn check_invalid_recipes(valid: CacheRecipe) -> Result<(), Box<dyn Error>> {
    for (label, recipe) in invalid_recipe_cases(valid) {
        if validate_recipe(&recipe).is_ok() {
            return Err(io::Error::other(format!("invalid recipe accepted: {label}")).into());
        }
    }
    let probe = require_recipe("stable.resource-probe-release.nextest")?;
    let no_profile = CacheRecipe {
        nextest_profile: "",
        ..probe
    };
    if validate_recipe(&no_profile).is_ok() {
        return Err(io::Error::other("test recipe without nextest profile accepted").into());
    }
    Ok(())
}

fn check_invalid_catalogs(valid: CacheRecipe) -> Result<(), Box<dyn Error>> {
    if validate_catalog_slice(&[]).is_ok() {
        return Err(io::Error::other("empty recipe catalog accepted").into());
    }
    let mut duplicated: Vec<CacheRecipe> = all_recipes().to_vec();
    duplicated.push(valid);
    if validate_catalog_slice(&duplicated).is_ok() {
        return Err(io::Error::other("duplicate recipe key accepted").into());
    }
    let natives: Vec<CacheRecipe> = recipes_of(CacheRecipeKind::Native)
        .into_iter()
        .copied()
        .collect();
    if validate_catalog_slice(&natives).is_ok() {
        return Err(io::Error::other("partial kind union accepted").into());
    }
    Ok(())
}

#[test]
fn gc1_t07_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    let valid = require_recipe("native.cli.linux-x86_64")?;
    check_invalid_recipes(valid)?;
    check_invalid_catalogs(valid)?;
    Ok(())
}
