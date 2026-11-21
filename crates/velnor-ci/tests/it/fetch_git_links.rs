// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use std::fs;
use std::os::unix::fs::symlink;
use std::path::Path;

use super::fetch::{Fixture, TOKEN, TestResult, script, serve};

fn cargo_adding_a_pack(fixture: &Fixture, extra: &str) -> TestResult {
    let root = fixture.repo.root();
    script(
        &root.join("trusted/bin/cargo"),
        &format!(
            "db=\"$CARGO_HOME/git/db/old-1\"\nmkdir -p \"$db/objects/pack\"\necho fresh > \"$db/objects/pack/fresh.pack\"\n{extra}",
        ),
    )?;
    Ok(())
}

fn existing_database(root: &Path) -> TestResult {
    let database = root.join("home/.cargo/git/db/old-1");
    fs::create_dir_all(database.join("objects/pack"))?;
    fs::write(database.join("config"), "[core]\n\tbare = true\n")?;
    fs::write(database.join("objects/pack/kept.pack"), "kept")?;
    Ok(())
}

#[test]
fn a_link_inside_an_existing_git_database_is_never_written_through_and_still_revokes() -> TestResult
{
    for linked in ["objects", "objects/pack"] {
        let fixture = Fixture::new(&format!("fetch-git-deep-{}", linked.replace('/', "-")), 0)?;
        let root = fixture.repo.root().to_path_buf();
        cargo_adding_a_pack(&fixture, "")?;
        existing_database(&root)?;
        let outside = root.join("outside");
        fs::create_dir_all(&outside)?;
        let link = root.join("home/.cargo/git/db/old-1").join(linked);
        fs::remove_dir_all(&link)?;
        symlink(&outside, &link)?;
        let api = serve(204)?;
        let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
        assert_eq!(
            fs::read_dir(&outside)?.count(),
            0,
            "{linked}: written through"
        );
        assert!(!output.status.success(), "{linked}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("symbolic link"),
            "{linked}: {output:?}"
        );
        assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    }
    Ok(())
}

#[test]
fn a_link_planted_at_the_staging_path_is_never_written_through_and_still_revokes() -> TestResult {
    let fixture = Fixture::new("fetch-git-staging", 0)?;
    let root = fixture.repo.root().to_path_buf();
    existing_database(&root)?;
    let victim = root.join("victim");
    fs::write(&victim, "victim")?;
    let pack = root.join("home/.cargo/git/db/old-1/objects/pack");
    cargo_adding_a_pack(
        &fixture,
        &format!(
            "ln -s {victim} {pack}/.fresh.pack.velnor-$PPID",
            victim = victim.display(),
            pack = pack.display()
        ),
    )?;
    let api = serve(204)?;
    let output = fixture.run_with(&api, Some(TOKEN), None, &["Cargo.toml"], &[]);
    assert_eq!(fs::read_to_string(&victim)?, "victim", "{output:?}");
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read_to_string(pack.join("fresh.pack"))?, "fresh\n");
    assert_eq!(api.requests.lock().map_err(|e| e.to_string())?.len(), 1);
    Ok(())
}
