// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, CacheRoot, Host, JobId, LockIdentity, RepoPath};
use velnor_render::render_key;
use velnor_rust::LOCK_PATH;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn workflow_with_helper(helper: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut repo = view(&["Cargo.toml", "lab/Cargo.toml"], None)?;
    let lock = repo.files.get(&LOCK_PATH).ok_or("no helper lock")?;
    let pinned = String::from_utf8(lock.clone())?.replace("0.1.92", helper);
    repo.files.insert(LOCK_PATH, pinned.into_bytes());
    workflow(&repo)
}

#[test]
fn a_helper_that_predates_the_member_lock_key_keeps_hashing_the_lock_file() -> TestResult {
    for helper in ["0.1.83", "0.1.84", "0.1.85"] {
        let text = workflow_with_helper(helper)?;
        assert!(
            text.contains("hashFiles(''Cargo.lock'')"),
            "{helper}: {text}"
        );
        assert!(text.contains("hashFiles(''lab/Cargo.lock'')"), "{helper}");
        assert!(!text.contains(".outputs.lock_"), "{helper}");
    }
    Ok(())
}

#[test]
fn a_helper_with_the_member_lock_key_reads_it_from_the_prepare_step_for_every_root() -> TestResult {
    let text = workflow_with_helper("0.1.86")?;
    assert!(text.contains("velnor-src-${{ runner.os }}-${{ runner.arch }}-all.roots-${{ steps.sources_locks.outputs.hash }}"), "{text}");
    for slug in ["root", "lab"] {
        let lock = format!("steps.prepare.outputs.lock_{slug}");
        let target = format!(
            "${{{{ steps.prepare.outputs.compat }}}}-${{{{ {lock} }}}}-${{{{ github.sha }}}}"
        );
        assert!(text.contains(&target), "{target}");
    }
    assert!(text.contains("hashFiles(''mise.lock'')"));
    Ok(())
}

#[test]
fn the_member_lock_key_replaces_only_the_lock_segment_of_the_target_key() -> TestResult {
    let manifest = RepoPath::new("lab/Cargo.toml")?;
    let file = CacheRoot::new(&manifest, RepoPath::new("target")?);
    let members = file.clone().with_lock_identity(LockIdentity::Members);
    let target = |root: &CacheRoot| {
        Ok::<_, Box<dyn std::error::Error>>(render_key(&CacheKey::Target {
            job: JobId::new("rust-lint")?,
            root: root.clone(),
            host: Host,
        }))
    };
    let (old, new) = (target(&file)?, target(&members)?);
    assert_eq!(old.restore_keys.last(), new.restore_keys.last());
    assert_ne!(old.restore_keys.first(), new.restore_keys.first());
    Ok(())
}
