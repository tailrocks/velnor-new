// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use crate::gc1_scene::Scene;

use super::*;

#[test]
#[cfg(unix)]
fn gc1_t53_recover_goes_cold_when_the_import_directory_is_in_the_workspace()
-> Result<(), Box<dyn Error>> {
    let scene = Scene::runner_in_workspace("import-in-workspace", crate::gc1_t53::scripts()?)?;
    let key = scene.discover()?;
    scene.pack_export("old")?;
    let output = scene.recover(&key.exact(), &[], None)?;
    scene.assert_cold(&output, "import_in_workspace")?;
    if scene.workspace.join("target").exists() {
        return Err(fail("import wrote the workspace target"));
    }
    scene.finish()
}
