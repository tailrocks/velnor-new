// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::constants::{
    FAMILY_BUILD, FAMILY_CLIPPY, FAMILY_DOC, FAMILY_HELPER, FAMILY_NAMED_CHECK,
    FAMILY_NATIVE_PRODUCER, FAMILY_PREVIEW_PRODUCER, FAMILY_TEST,
};
use super::kinds::{CacheRecipe, CacheRecipeError, CacheRecipeKind};

fn invalid(key: &'static str, problem: &'static str) -> CacheRecipeError {
    CacheRecipeError {
        recipe_key: key,
        problem,
    }
}

fn is_sorted_unique(values: &[&str]) -> bool {
    values
        .iter()
        .zip(values.iter().skip(1))
        .all(|(first, second)| first < second)
}

pub fn validate_recipe(recipe: &CacheRecipe) -> Result<(), CacheRecipeError> {
    let key = recipe.recipe_key;
    if key.is_empty() {
        return Err(invalid(key, "empty_recipe_key"));
    }
    if !matches!(
        recipe.command_family,
        FAMILY_BUILD
            | FAMILY_TEST
            | FAMILY_CLIPPY
            | FAMILY_DOC
            | FAMILY_NAMED_CHECK
            | FAMILY_HELPER
            | FAMILY_NATIVE_PRODUCER
            | FAMILY_PREVIEW_PRODUCER
    ) {
        return Err(invalid(key, "unknown_command_family"));
    }
    if recipe.command_builder_id.is_empty() {
        return Err(invalid(key, "empty_command_builder_id"));
    }
    if recipe.lane_id.is_empty() {
        return Err(invalid(key, "empty_lane_id"));
    }
    if let Some(argv) = recipe.command_argv {
        if argv.is_empty() {
            return Err(invalid(key, "empty_command_argv"));
        }
        if recipe.tool_route.is_empty() {
            return Err(invalid(key, "missing_tool_route"));
        }
        if recipe.package.is_empty() {
            return Err(invalid(key, "missing_package"));
        }
        if recipe.cargo_profile.is_empty() {
            return Err(invalid(key, "missing_cargo_profile"));
        }
    } else {
        if !recipe.tool_route.is_empty() {
            return Err(invalid(key, "route_without_command"));
        }
        if !recipe.package.is_empty() {
            return Err(invalid(key, "package_without_command"));
        }
        if !recipe.cargo_profile.is_empty() {
            return Err(invalid(key, "profile_without_command"));
        }
    }
    if recipe.command_family == FAMILY_TEST && recipe.nextest_profile.is_empty() {
        return Err(invalid(key, "missing_nextest_profile"));
    }
    if !is_sorted_unique(recipe.targets) {
        return Err(invalid(key, "targets_not_sorted_unique"));
    }
    if !is_sorted_unique(recipe.features) {
        return Err(invalid(key, "features_not_sorted_unique"));
    }
    if !is_sorted_unique(recipe.source_inputs) {
        return Err(invalid(key, "source_inputs_not_sorted_unique"));
    }
    if !is_sorted_unique(recipe.environment_inputs) {
        return Err(invalid(key, "environment_inputs_not_sorted_unique"));
    }
    if recipe.output_roles.is_empty() {
        return Err(invalid(key, "empty_output_roles"));
    }
    if !is_sorted_unique(recipe.output_roles) {
        return Err(invalid(key, "output_roles_not_sorted_unique"));
    }
    Ok(())
}

pub fn validate_catalog_slice(recipes: &[CacheRecipe]) -> Result<(), CacheRecipeError> {
    if recipes.is_empty() {
        return Err(invalid("", "empty_recipe_catalog"));
    }
    for recipe in recipes {
        validate_recipe(recipe)?;
    }
    for (index, recipe) in recipes.iter().enumerate() {
        if recipes[..index]
            .iter()
            .any(|seen| seen.recipe_key == recipe.recipe_key)
        {
            return Err(invalid(recipe.recipe_key, "duplicate_recipe_key"));
        }
    }
    for kind in CacheRecipeKind::ALL {
        if !recipes.iter().any(|recipe| recipe.kind == kind) {
            return Err(invalid("", "missing_recipe_kind"));
        }
    }
    Ok(())
}
