/// Build a git fixture: config plus one virtual workspace (uncommitted).
///
/// The root manifest carries `[workspace]` only, so no member owns it;
/// member `a` lives under `crates/a`.
pub(crate) fn make_virtual_repo(config: &str) -> Result<TempDir, Box<dyn std::error::Error>> {
    let dir = make_repo(config)?;
    let root = dir.path();
    fs::write(
        root.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/a\"]\n",
    )?;
    fs::remove_dir_all(root.join("src"))?;
    let member = root.join("crates/a");
    fs::create_dir_all(member.join("src"))?;
    fs::write(
        member.join("Cargo.toml"),
        "[package]\nname = \"a\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )?;
    fs::write(member.join("src/lib.rs"), "pub fn f() {}\n")?;
    Ok(dir)
}
