// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use velnor_model::{ImageConfig, RepoPath, RepoView};
use velnor_render::IMAGE_TARGET;

use crate::ImageError;

pub(crate) fn check(view: &RepoView, image: &ImageConfig, root: &str) -> Result<(), ImageError> {
    let mut directory = root;
    loop {
        for name in ["config.toml", "config"] {
            let path = if directory == "." {
                format!(".cargo/{name}")
            } else {
                format!("{directory}/.cargo/{name}")
            };
            let text = RepoPath::new(path.as_str())
                .ok()
                .and_then(|path| view.files.get(&path))
                .and_then(|bytes| std::str::from_utf8(bytes).ok());
            if text.is_some_and(sets_cpu_features) {
                return Err(ImageError::CpuBaseline {
                    image: image.name.clone(),
                    path,
                });
            }
        }
        match directory.rsplit_once('/') {
            Some((parent, _)) => directory = parent,
            None if directory == "." => return Ok(()),
            None => directory = ".",
        }
    }
}

fn sets_cpu_features(text: &str) -> bool {
    let Ok(config) = toml::from_str::<toml::Table>(text) else {
        return false;
    };
    let mut tables = Vec::new();
    tables.extend(config.get("build").and_then(toml::Value::as_table));
    if let Some(targets) = config.get("target").and_then(toml::Value::as_table) {
        tables.extend(targets.iter().filter_map(|(key, value)| {
            (key == IMAGE_TARGET || key.starts_with("cfg("))
                .then(|| value.as_table())
                .flatten()
        }));
    }
    tables
        .into_iter()
        .filter_map(|table| table.get("rustflags"))
        .any(|flags| {
            let text = match flags {
                toml::Value::String(text) => text.clone(),
                toml::Value::Array(items) => items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" "),
                _ => String::new(),
            };
            let text = text.replace('_', "-");
            text.contains("target-cpu") || text.contains("target-feature")
        })
}
