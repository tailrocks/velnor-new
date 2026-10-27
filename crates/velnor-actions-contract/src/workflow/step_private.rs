// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

pub const PRIVATE_TOKEN_OUTPUT_EXPR: &str = "${{ steps.deps-token.outputs.token }}";

pub const PRIVATE_NPM_READ_EXPR: &str = "${{ github.token }}";

pub const PRIVATE_FETCH_TOKEN_ENV: &str = "VELNOR_PRIVATE_FETCH_TOKEN";

pub const PRIVATE_NPM_READ_TOKEN_ENV: &str = "VELNOR_NPM_READ_TOKEN";

pub const PRIVATE_FETCH_OPERATION: &str = "private-fetch-v1";

pub const PRIVATE_REVOKE_OPERATION: &str = "private-revoke-v1";

pub const BUN_REGISTRY_INSTALL_OPERATION: &str = "bun-registry-install-v1";

pub const BUN_ASSETS_EXPORT_OPERATION: &str = "bun-assets-export-v1";

pub const BUN_ASSETS_PREPARE_OPERATION: &str = "bun-assets-prepare-v1";

pub const BUN_ASSETS_IMPORT_OPERATION: &str = "bun-assets-import-v1";

pub const BUN_ASSETS_REPOSITORY_CHECK_OPERATION: &str = "bun-assets-repository-check-v1";

pub const GITHUB_EVIDENCE_FETCH_OPERATION: &str = "github-evidence-fetch-v1";

pub const GITHUB_EVIDENCE_IMPORT_OPERATION: &str = "github-evidence-import-v1";

pub const GITHUB_EVIDENCE_TOKEN_ENV: &str = "GH_TOKEN";

pub const GITHUB_EVIDENCE_SPEC_ENV: &str = "VELNOR_GITHUB_EVIDENCE_SPEC";

pub const PRIVATE_TOKEN_MINT_ACTION_PREFIX: &str = "actions/create-github-app-token@";
