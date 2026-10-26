// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

#[path = "cache_recipes/catalog.rs"]
mod catalog;
#[path = "cache_recipes/constants.rs"]
mod constants;
#[path = "cache_recipes/kinds.rs"]
mod kinds;
#[path = "cache_recipes/validate.rs"]
mod validate;

pub use catalog::{all_recipes, lookup_recipe, recipes_of, validate_catalog};
pub use constants::PREVIEW_PRODUCER_RECIPE_KEY;
pub use kinds::{CacheRecipe, CacheRecipeError, CacheRecipeKind, parse_kind};
pub use validate::{validate_catalog_slice, validate_recipe};
