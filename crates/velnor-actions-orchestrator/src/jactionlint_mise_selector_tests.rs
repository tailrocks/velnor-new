//! Setup Mise data-directory selector cases for the jactionlint verifier.

use std::error::Error;
use std::fs;
use std::path::Path;

use super::{assert_selected_mise_directory, fake_runner, fake_runner_bin};

#[test]
fn explicit_mise_data_dir_precedes_xdg_data_home() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let explicit = root.path().join("explicit-mise");
    let xdg = root.path().join("xdg");
    let xdg_mise = xdg.join("mise");
    fake_runner(root.path(), &explicit)?;
    fake_runner(root.path(), &xdg_mise)?;
    assert_selected_mise_directory(
        root.path(),
        &explicit,
        Some(&explicit),
        Some(&xdg),
        root.path(),
    )
}

#[test]
fn explicit_data_dir_with_symlinked_ancestor_uses_resolved_setup_binary()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let physical_parent = root.path().join("physical");
    let alias_parent = root.path().join("alias");
    let selected = physical_parent.join("mise-data");
    let selected_through_alias = alias_parent.join("mise-data");
    fake_runner(root.path(), &selected)?;
    std::os::unix::fs::symlink(&physical_parent, &alias_parent)?;

    assert_selected_mise_directory(
        root.path(),
        &selected,
        Some(&selected_through_alias),
        None,
        root.path(),
    )
}

#[test]
fn explicit_data_dir_symlink_with_trailing_separators_uses_resolved_binary()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let selected = root.path().join("mise-data");
    let alias = root.path().join("mise-data-alias");
    fake_runner(root.path(), &selected)?;
    std::os::unix::fs::symlink(&selected, &alias)?;
    let selector = format!("{}///", alias.display());

    assert_selected_mise_directory(
        root.path(),
        &selected,
        Some(Path::new(&selector)),
        None,
        root.path(),
    )
}

#[test]
fn selected_bin_directory_alias_with_spaces_uses_its_physical_binary() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    let selected = root.path().join("mise-data");
    let setup_store = root.path().join("setup store").join("selected-mise");
    fs::create_dir_all(&selected)?;
    fake_runner(root.path(), &setup_store)?;
    std::os::unix::fs::symlink(setup_store.join("bin"), selected.join("bin"))?;

    assert_selected_mise_directory(root.path(), &selected, Some(&selected), None, root.path())
}

#[test]
fn data_directory_selectors_preserve_embedded_and_trailing_newlines() -> Result<(), Box<dyn Error>>
{
    let root = tempfile::tempdir()?;
    for name in ["mise-embedded\nnewline", "mise-trailing\n"] {
        let selected = root.path().join(name);
        fake_runner(root.path(), &selected)?;
        assert_selected_mise_directory(root.path(), &selected, Some(&selected), None, root.path())?;
    }
    Ok(())
}

#[test]
fn bin_directory_aliases_preserve_embedded_and_trailing_newlines() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    for (selected_name, bin_name) in [
        ("selected-embedded", "setup-bin-embedded\nnewline"),
        ("selected-trailing", "setup-bin-trailing\n"),
    ] {
        let selected = root.path().join(selected_name);
        let physical_bin = root.path().join(bin_name);
        fs::create_dir_all(&selected)?;
        fake_runner_bin(root.path(), &physical_bin)?;
        std::os::unix::fs::symlink(&physical_bin, selected.join("bin"))?;
        assert_selected_mise_directory(root.path(), &selected, Some(&selected), None, root.path())?;
    }
    Ok(())
}

#[test]
fn relative_explicit_data_dir_is_resolved_from_the_setup_working_directory()
-> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let selected = root.path().join("relative-mise");
    fake_runner(root.path(), &selected)?;

    assert_selected_mise_directory(
        root.path(),
        &selected,
        Some(Path::new("relative-mise")),
        None,
        root.path(),
    )
}

#[test]
fn xdg_data_home_selects_its_mise_child_without_explicit_data_dir() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let xdg = root.path().join("xdg-data");
    let selected = xdg.join("mise");
    fake_runner(root.path(), &selected)?;
    assert_selected_mise_directory(root.path(), &selected, None, Some(&xdg), root.path())
}

#[test]
fn home_data_directory_is_the_fallback_when_mise_and_xdg_are_unset() -> Result<(), Box<dyn Error>> {
    let root = tempfile::tempdir()?;
    let home = root.path().join("home");
    let selected = home.join(".local/share/mise");
    fake_runner(root.path(), &selected)?;
    assert_selected_mise_directory(root.path(), &selected, None, None, &home)
}
