//! Configured-roots qualification cases.
use crate::support::{Outcome, TempDir};
use velnor_actions_contract::{TofuStackConfig, Utf8RepoRelDir, build_index};
use velnor_actions_tofu::{STACK_ID, qualify_roots};

/// Tofu config from raw root spellings.
fn tofu(raw: &[&str]) -> TofuStackConfig {
    TofuStackConfig {
        roots: raw
            .iter()
            .map(|entry| Utf8RepoRelDir::from_raw((*entry).to_owned()))
            .collect(),
    }
}

#[test]
fn dot_and_subdir_roots_emit_unit_candidates() -> Outcome {
    let dir = TempDir::create("tofu-roots-ok")?;
    dir.write("main.tf", "")?;
    dir.write("envs/prod/main.tofu", "")?;
    let index = build_index(dir.path(), &[])?;
    let candidates = qualify_roots(
        "config.toml",
        dir.path(),
        &tofu(&[".", "envs/prod"]),
        &index,
    )?;
    assert_eq!(candidates.len(), 2);
    assert_eq!(candidates[0].stack_id, STACK_ID);
    assert_eq!(candidates[0].unit_root, "");
    assert_eq!(candidates[1].unit_root, "envs/prod");
    Ok(())
}

#[test]
fn lexical_violations_fail_closed() -> Outcome {
    let dir = TempDir::create("tofu-roots-lexical")?;
    let index = build_index(dir.path(), &[])?;
    for raw in ["../escape", "/abs", "a//b", ""] {
        let err = qualify_roots("config.toml", dir.path(), &tofu(&[raw]), &index)
            .expect_err("lexical violation fails");
        assert!(
            err.to_string().contains("stacks.tofu.roots"),
            "{raw}: {err}"
        );
    }
    Ok(())
}

#[test]
fn missing_directory_errors_naming_the_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-missing")?;
    let index = build_index(dir.path(), &[])?;
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["ghost"]), &index)
        .expect_err("missing dir fails");
    let text = err.to_string();
    assert!(text.contains("unreadable_root"), "{text}");
    assert!(text.contains("ghost"), "{text}");
    Ok(())
}

#[test]
fn root_without_effective_config_errors_naming_the_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-empty")?;
    dir.write("notes.txt", "not config\n")?;
    dir.write("vars/terraform.tfvars", "")?;
    let index = build_index(dir.path(), &[])?;
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["."]), &index)
        .expect_err("config-less root fails");
    let text = err.to_string();
    assert!(text.contains("no_effective_config"), "{text}");
    assert!(text.contains('.'), "{text}");
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["vars"]), &index)
        .expect_err("var-only root fails");
    assert!(err.to_string().contains("no_effective_config"), "{err}");
    Ok(())
}

#[test]
fn shadowed_pair_still_counts_effective() -> Outcome {
    let dir = TempDir::create("tofu-roots-shadowed")?;
    dir.write("main.tf", "")?;
    dir.write("main.tofu", "")?;
    let index = build_index(dir.path(), &[])?;
    let candidates = qualify_roots("config.toml", dir.path(), &tofu(&["."]), &index)?;
    assert_eq!(candidates.len(), 1);
    Ok(())
}

#[test]
fn nested_files_do_not_satisfy_parent_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-nested")?;
    dir.write("child/main.tf", "")?;
    let index = build_index(dir.path(), &[])?;
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["."]), &index)
        .expect_err("nested-only root fails");
    assert!(err.to_string().contains("no_effective_config"), "{err}");
    let candidates = qualify_roots("config.toml", dir.path(), &tofu(&["child"]), &index)?;
    assert_eq!(candidates.len(), 1);
    Ok(())
}

#[test]
fn json_dialect_satisfies_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-json")?;
    dir.write("main.tf.json", "{}\n")?;
    let index = build_index(dir.path(), &[])?;
    let candidates = qualify_roots("config.toml", dir.path(), &tofu(&["."]), &index)?;
    assert_eq!(candidates.len(), 1);
    Ok(())
}

#[test]
fn override_files_satisfy_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-override")?;
    dir.write("override.tf", "")?;
    let index = build_index(dir.path(), &[])?;
    let candidates = qualify_roots("config.toml", dir.path(), &tofu(&["."]), &index)?;
    assert_eq!(candidates.len(), 1);
    Ok(())
}

#[test]
fn excluded_files_do_not_satisfy_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-excluded")?;
    dir.write("vendor/main.tf", "")?;
    let index = build_index(dir.path(), &["vendor/**".to_owned()])?;
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["vendor"]), &index)
        .expect_err("excluded root fails");
    assert!(err.to_string().contains("no_effective_config"), "{err}");
    Ok(())
}

#[test]
fn alias_without_indexed_files_fails_naming_the_root() -> Outcome {
    let dir = TempDir::create("tofu-roots-link")?;
    dir.write("real/main.tf", "")?;
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.path().join("real"), dir.path().join("alias"))?;
    #[cfg(not(unix))]
    std::fs::create_dir_all(dir.path().join("alias"))?;
    let index = build_index(dir.path(), &[])?;
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["alias"]), &index)
        .expect_err("alias holds no indexed files");
    let text = err.to_string();
    assert!(text.contains("no_effective_config"), "{text}");
    assert!(text.contains("alias"), "{text}");
    Ok(())
}

#[test]
#[cfg(unix)]
fn escaping_symlink_root_fails_closed() -> Outcome {
    use velnor_actions_contract::build_index_from_list;
    let dir = TempDir::create("tofu-roots-escape")?;
    let outside = TempDir::create("tofu-roots-outside")?;
    outside.write("main.tf", "")?;
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape"))?;
    // List-built index (no walk): the escape reaches qualification.
    let listed = [("escape/main.tf").to_owned()];
    let index = build_index_from_list(dir.path(), &listed, &[])?;
    let err = qualify_roots("config.toml", dir.path(), &tofu(&["escape"]), &index)
        .expect_err("escape fails");
    let text = err.to_string();
    assert!(text.contains("symlink_escape"), "{text}");
    assert!(text.contains("escape"), "{text}");
    Ok(())
}

#[test]
fn unreadable_repo_root_fails_closed() -> Outcome {
    let dir = TempDir::create("tofu-roots-noroot")?;
    dir.write("main.tf", "")?;
    let index = build_index(dir.path(), &[])?;
    let ghost = dir.path().join("ghost-root");
    let err = qualify_roots("config.toml", &ghost, &tofu(&["."]), &index)
        .expect_err("missing repo root fails");
    assert!(err.to_string().contains("unreadable_repo_root"), "{err}");
    Ok(())
}
