//! Fixed identities for private dependency and Bun asset steps.

/// App installation token minted for private sources (HD-7).
pub const PRIVATE_TOKEN_OUTPUT_EXPR: &str = "${{ steps.deps-token.outputs.token }}";
/// Expression of the job token bound to the npm registry phase (HD-7).
pub const PRIVATE_NPM_READ_EXPR: &str = "${{ github.token }}";
/// Step env key carrying the App installation token (HD-7).
pub const PRIVATE_FETCH_TOKEN_ENV: &str = "VELNOR_PRIVATE_FETCH_TOKEN";
/// Step env key carrying the npm registry read token (HD-7).
pub const PRIVATE_NPM_READ_TOKEN_ENV: &str = "VELNOR_NPM_READ_TOKEN";
/// Internal operation of the Velnor-owned private fetch step (HD-7).
pub const PRIVATE_FETCH_OPERATION: &str = "private-fetch-v1";
/// Internal operation of the token revocation step (HD-7).
pub const PRIVATE_REVOKE_OPERATION: &str = "private-revoke-v1";
/// Internal operation of the Bun registry phase (HD-6, HD-7).
pub const BUN_REGISTRY_INSTALL_OPERATION: &str = "bun-registry-install-v1";
/// Internal operation that exports the verified Bun asset bundle.
pub const BUN_ASSETS_EXPORT_OPERATION: &str = "bun-assets-export-v1";
/// Internal operation preparing the Bun asset download destination.
pub const BUN_ASSETS_PREPARE_OPERATION: &str = "bun-assets-prepare-v1";
/// Internal operation importing and pin-checking the Bun asset archive.
pub const BUN_ASSETS_IMPORT_OPERATION: &str = "bun-assets-import-v1";
/// Trusted, source-free GitHub evidence acquisition operation.
pub const GITHUB_EVIDENCE_FETCH_OPERATION: &str = "github-evidence-fetch-v1";
/// Credential-free verification and import of the same-run evidence bundle.
pub const GITHUB_EVIDENCE_IMPORT_OPERATION: &str = "github-evidence-import-v1";
/// Job token binding for the exact trusted evidence-fetch operation.
pub const GITHUB_EVIDENCE_TOKEN_ENV: &str = "GH_TOKEN";
/// Canonical generator-resolved evidence request carried to the fetch helper.
pub const GITHUB_EVIDENCE_SPEC_ENV: &str = "VELNOR_GITHUB_EVIDENCE_SPEC";
/// Pinned-action prefix of the private token mint step (HD-7).
pub const PRIVATE_TOKEN_MINT_ACTION_PREFIX: &str = "actions/create-github-app-token@";
