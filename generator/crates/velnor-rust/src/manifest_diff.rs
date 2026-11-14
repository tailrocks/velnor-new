// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use toml::{Table, Value};

pub(crate) fn only_dependency_versions_differ(base: &str, head: &str) -> bool {
    let (Ok(mut base), Ok(mut head)) =
        (toml::from_str::<Table>(base), toml::from_str::<Table>(head))
    else {
        return false;
    };
    mask_versions(&mut base);
    mask_versions(&mut head);
    base == head
}

fn mask_versions(manifest: &mut Table) {
    let Some(dependencies) = manifest
        .get_mut("workspace")
        .and_then(Value::as_table_mut)
        .and_then(|workspace| workspace.get_mut("dependencies"))
        .and_then(Value::as_table_mut)
    else {
        return;
    };
    for (_, dependency) in dependencies.iter_mut() {
        match dependency {
            Value::String(version) => version.clear(),
            Value::Table(table) => {
                if let Some(Value::String(version)) = table.get_mut("version") {
                    version.clear();
                }
            }
            _ => {}
        }
    }
}
