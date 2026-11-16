// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{CacheKey, CacheRoot, Host, JobId, RepoPath, RepoView};
use velnor_render::render_key;

use crate::facts::TestResult;
use crate::pipeline::{view, workflow};

fn with_config(view: &mut RepoView, path: &str, text: &str) -> TestResult {
    let path = RepoPath::new(path)?;
    view.tracked_paths.insert(path.clone());
    view.files.insert(path, text.as_bytes().to_vec());
    Ok(())
}

fn sources_key(manifest: &str) -> Result<String, Box<dyn std::error::Error>> {
    Ok(render_key(&CacheKey::Sources {
        host: Host,
        root: CacheRoot::new(&RepoPath::new(manifest)?, RepoPath::new("target")?),
    })
    .key)
}

#[test]
fn a_root_path_with_glob_characters_is_read_literally_by_hash_files() -> TestResult {
    for (manifest, literal) in [
        ("a*b/Cargo.toml", "a[*]b/Cargo.lock"),
        ("a?b/Cargo.toml", "a[?]b/Cargo.lock"),
        ("a[b]/Cargo.toml", "a[[]b]/Cargo.lock"),
        ("!x/Cargo.toml", "./!x/Cargo.lock"),
        ("#h/Cargo.toml", "./#h/Cargo.lock"),
        ("plain/Cargo.toml", "plain/Cargo.lock"),
    ] {
        let key = sources_key(manifest)?;
        assert!(
            key.ends_with(&format!("-${{{{ hashFiles('{literal}') }}}}")),
            "{key}"
        );
    }
    Ok(())
}

#[test]
fn a_rendered_key_over_512_characters_fails_generation() -> TestResult {
    let long = |letters: usize| format!("{}/Cargo.toml", "d".repeat(letters));
    workflow(&view(&[&long(300)], None)?)?;
    let error = workflow(&view(&[&long(360)], None)?)
        .err()
        .map(|error| error.to_string())
        .unwrap_or_default();
    assert!(error.contains("512"), "{error}");
    assert!(error.contains("rust"), "{error}");
    Ok(())
}

#[test]
fn no_key_family_carries_the_run_attempt() -> TestResult {
    let root = CacheRoot::new(&RepoPath::new("Cargo.toml")?, RepoPath::new("target")?);
    for key in [
        CacheKey::Tools { host: Host },
        CacheKey::Sources {
            host: Host,
            root: root.clone(),
        },
        CacheKey::Target {
            job: JobId::new("rust")?,
            root: root.clone(),
            host: Host,
        },
        CacheKey::Delta {
            job: JobId::new("rust")?,
            root,
            host: Host,
        },
    ] {
        let rendered = render_key(&key);
        for line in std::iter::once(&rendered.key).chain(&rendered.restore_keys) {
            assert!(!line.contains("run_attempt"), "{line}");
        }
    }
    Ok(())
}

#[test]
fn the_target_cache_path_follows_build_target_dir() -> TestResult {
    let mut one = view(&["Cargo.toml"], None)?;
    with_config(
        &mut one,
        ".cargo/config.toml",
        "[build]\ntarget-dir = \"out\"\n",
    )?;
    let text = workflow(&one)?;
    assert!(text.contains("path: out\n"), "{text}");
    assert!(!text.contains("path: target\n"), "{text}");

    let mut nested = view(&["Cargo.toml", "lab/Cargo.toml", "tools/Cargo.toml"], None)?;
    with_config(
        &mut nested,
        ".cargo/config.toml",
        "[build]\ntarget-dir = \"shared\"\n",
    )?;
    with_config(
        &mut nested,
        "lab/.cargo/config.toml",
        "[build]\ntarget-dir = \"../out/lab\"\n",
    )?;
    with_config(
        &mut nested,
        "tools/.cargo/config",
        "[build]\ntarget-dir = \"build\"\n",
    )?;
    with_config(
        &mut nested,
        "tools/.cargo/config.toml",
        "[build]\ntarget-dir = \"ignored\"\n",
    )?;
    let text = workflow(&nested)?;
    assert!(text.contains("path: shared\n"), "{text}");
    assert!(text.contains("path: out/lab\n"), "{text}");
    assert!(text.contains("path: tools/build\n"), "{text}");
    assert!(!text.contains("path: lab/target\n"), "{text}");

    let plain = workflow(&view(&["lab/Cargo.toml"], None)?)?;
    assert!(plain.contains("path: lab/target\n"), "{plain}");
    Ok(())
}

#[test]
fn a_target_dir_outside_the_repository_fails_generation() -> TestResult {
    for value in [
        "\"/tmp/x\"",
        "\"../../x\"",
        "\".\"",
        "\"../lab\"",
        "{ path = \"x\" }",
    ] {
        let mut one = view(&["lab/Cargo.toml"], None)?;
        with_config(
            &mut one,
            "lab/.cargo/config.toml",
            &format!("[build]\ntarget-dir = {value}\n"),
        )?;
        let error = workflow(&one)
            .err()
            .map(|error| error.to_string())
            .unwrap_or_default();
        assert!(
            error.contains("lab/.cargo/config.toml") && error.contains("target-dir"),
            "{value}: {error}"
        );
    }
    Ok(())
}
