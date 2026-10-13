//! Custom task-directory config stays outside the admitted verification closure.

use std::fs;

use velnor_actions_contract::VerificationRunner;

use super::{check_tool_inputs, resolve_verification_tools, tool_root, verify_task};

#[test]
fn configured_task_directory_is_not_admitted() {
    let root = tool_root();
    fs::write(
        root.path().join("mise.toml"),
        r#"
[settings]
lockfile = true

[task_config]
dir = "custom-tasks"

[tasks.verify]
depends = ["lint"]

[tasks.lint]
run = "echo lint"
"#,
    )
    .expect("write unsupported custom task directory");
    let task_file = root.path().join("custom-tasks/verify");
    fs::create_dir_all(task_file.parent().expect("task directory"))
        .expect("create custom task source directory");
    fs::write(&task_file, "#!/bin/sh\nmise install cargo:unsafe-tool\n")
        .expect("write unmodeled custom task body");

    let checks = check_tool_inputs(root.path());
    let error = resolve_verification_tools(&checks, &verify_task(VerificationRunner::LinuxX64))
        .expect_err("custom task directory config must fail closed");
    assert!(error.to_string().contains("verification_mise_config_root"));
}
