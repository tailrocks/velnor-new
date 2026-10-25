//! Owned-home environment behavior when the catalog omits Rustup selection.

use std::ffi::OsString;
use velnor_actions_mise::{ToolCatalog, ToolHomes};

#[test]
fn catalog_without_export_rustup_toolchain_omits_rustup_toolchain_env() -> Result<(), String> {
    let catalog = ToolCatalog::pinned().with_export_rustup_toolchain(false);
    assert!(!catalog.exports_rustup_toolchain());
    assert_eq!(catalog.rustup_toolchain_env(), None);
    let homes =
        ToolHomes::new("/velnor/rustup", "/velnor/cargo").map_err(|error| error.to_string())?;
    let env = homes.env(&catalog);
    assert_eq!(
        env,
        [
            (
                OsString::from("MISE_RUSTUP_HOME"),
                OsString::from("/velnor/rustup")
            ),
            (
                OsString::from("MISE_CARGO_HOME"),
                OsString::from("/velnor/cargo")
            ),
            (
                OsString::from("RUSTUP_HOME"),
                OsString::from("/velnor/rustup")
            ),
            (
                OsString::from("CARGO_HOME"),
                OsString::from("/velnor/cargo")
            ),
        ]
    );
    Ok(())
}
