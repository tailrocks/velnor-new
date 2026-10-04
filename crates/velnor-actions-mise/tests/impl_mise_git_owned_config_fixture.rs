use std::fs;
use std::path::{Path, PathBuf};

#[cfg(test)]
#[path = "impl_mise_git_index_fixture.rs"]
mod base;

pub(crate) use base::git_fixture;
pub(crate) use base::{Fixture, git_dir, git_owned, install_hook, repo, touch_identical};

pub(crate) fn configure_filter(
    root: &Path,
    layer: &str,
    operation: &str,
) -> Result<PathBuf, String> {
    if !matches!(layer, "include" | "nested-include") {
        return Err("unsupported local include fixture layer".to_owned());
    }
    fs::write(root.join(".gitattributes"), "tracked.txt filter=hostile\n")
        .map_err(|error| error.to_string())?;
    let marker = root.join(format!("{layer}-{operation}.marker"));
    let script = root.join(format!("{layer}-{operation}.sh"));
    let script_body = format!(
        "#!/bin/sh\nprintf '%s' '{}' > '{}'\ncat\n",
        operation,
        marker.display()
    );
    write_executable(&script, &script_body)?;

    let leaf = root.join(format!("{layer}-{operation}.inc"));
    let config = format!(
        "[filter \"hostile\"]\n\t{operation} = {}\n",
        script.display()
    );
    fs::write(&leaf, config).map_err(|error| error.to_string())?;
    let include = if layer == "include" {
        leaf
    } else {
        let parent = root.join(format!("{layer}-{operation}-parent.inc"));
        fs::write(&parent, format!("[include]\n\tpath = {}\n", leaf.display()))
            .map_err(|error| error.to_string())?;
        parent
    };
    git_owned(
        root,
        vec![
            "config".to_owned(),
            "--add".to_owned(),
            "--".to_owned(),
            "include.path".to_owned(),
            include.to_string_lossy().into_owned(),
        ],
    )?;
    Ok(marker)
}

pub(crate) fn add_native_values(root: &Path) -> Result<(), String> {
    for value in [
        "--file",
        "--blob",
        "--config-env",
        "line one\nline two",
        "",
        "tail",
    ] {
        git_owned(
            root,
            ["config", "--add", "--", "velnor.native", value]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        )?;
    }
    Ok(())
}

pub(crate) fn configure_trace_target(root: &Path) -> Result<PathBuf, String> {
    let marker = root.join("trace2-config.marker");
    git_owned(
        root,
        vec![
            "config".to_owned(),
            "--".to_owned(),
            "trace2.eventTarget".to_owned(),
            marker.to_string_lossy().into_owned(),
        ],
    )?;
    Ok(marker)
}

pub(crate) fn configure_metadata(root: &Path, kind: &str) -> Result<(), String> {
    let git = git_dir(root)?;
    match kind {
        "unknown" => git_owned(
            root,
            ["config", "--", "extensions.unknown", "true"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        )?,
        "promisor" => write_file(&git.join("objects/info/promisor"))?,
        "alternates" => write_file(&git.join("objects/info/alternates"))?,
        "shallow" => fs::write(
            git.join("shallow"),
            b"0000000000000000000000000000000000000000\n",
        )
        .map_err(|error| error.to_string())?,
        "replace" => {
            fs::create_dir_all(git.join("refs/replace")).map_err(|error| error.to_string())?
        }
        "gitlinks" => fs::write(root.join(".gitmodules"), b"[submodule \"nested\"]\n")
            .map_err(|error| error.to_string())?,
        other => return Err(format!("unknown metadata fixture: {other}")),
    }
    Ok(())
}

fn write_file(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(path, b"fixture\n").map_err(|error| error.to_string())
}

fn write_executable(path: &Path, body: &str) -> Result<(), String> {
    fs::write(path, body).map_err(|error| error.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path)
            .map_err(|error| error.to_string())?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).map_err(|error| error.to_string())?;
    }
    Ok(())
}
