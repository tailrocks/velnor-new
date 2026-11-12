// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::task_script::capture_command;

fn capture(producer: &str, name: &str) -> String {
    capture_command(producer, &format!("$task_root/{name}.txt"), name)
}

pub(super) fn mbx(version: &str) -> Vec<String> {
    vec![
        "case \"$OSTYPE\" in darwin*) tool_identity() { /usr/bin/stat -f '%d:%i:%z:%m:%c' \"$1\"; } ;; linux*) tool_identity() { /usr/bin/stat -c '%d:%i:%s:%Y:%Z' \"$1\"; } ;; *) exit 1 ;; esac".to_owned(),
        capture("/bin/realpath \"$task_root\"", "task_physical"),
        format!("mbx_root=\"$task_physical/data/installs/mr-boxington/{version}\""),
        "test -d \"$task_root\" && test -O \"$task_root\" && test -d \"$mbx_root\" && test -O \"$mbx_root\" && test ! -L \"$mbx_root\"".to_owned(),
        capture("/bin/realpath \"$mbx_root\"", "mbx_root_real"),
        "test \"$mbx_root_real\" = \"$mbx_root\"".to_owned(),
        capture("mise --no-env --locked --no-hooks which mbx", "mbx_alias"),
        capture("/bin/realpath \"$mbx_alias\"", "mbx_path"),
        "case \"$mbx_path\" in \"$mbx_root/\"*) ;; *) exit 1 ;; esac".to_owned(),
        "test -f \"$mbx_path\" && test -O \"$mbx_path\" && test -x \"$mbx_path\" && test ! -L \"$mbx_path\"".to_owned(),
        capture(&format!("mise --no-env --locked --no-hooks exec mr-boxington@{version} -- \"$mbx_path\" --version"), "mbx_version"),
        format!("test \"$mbx_version\" = 'mbx {version}'"),
        capture("tool_identity \"$mbx_path\"", "mbx_identity"),
    ]
}

pub(super) fn rust(version: &str, host: &str, shared_cargo_sources: bool) -> Vec<String> {
    vec![
        format!("export VELNOR_EXPECTED_RUSTUP_TOOLCHAIN='{version}-{host}'"),
        capture("/bin/realpath \"$CARGO_HOME\"", "cargo_home_physical"),
        capture("/bin/realpath \"$RUSTUP_HOME\"", "rustup_home_physical"),
        if shared_cargo_sources { "test \"$cargo_home_physical\" = \"$RUNNER_TEMP/velnor/cargo\" && test \"$rustup_home_physical\" = \"$RUNNER_TEMP/velnor/rustup\"" } else { "test \"$cargo_home_physical\" = \"$task_physical/cargo\" && test \"$rustup_home_physical\" = \"$task_physical/rustup\"" }.to_owned(),
        "rustup_path=\"$cargo_home_physical/bin/rustup\"".to_owned(),
        "test -f \"$rustup_path\" && test -O \"$rustup_path\" && test -x \"$rustup_path\" && test ! -L \"$rustup_path\"".to_owned(),
        capture("/bin/realpath \"$rustup_path\"", "rustup_real"),
        "test \"$rustup_real\" = \"$rustup_path\"".to_owned(),
        capture("mise --no-env --locked --no-hooks which rustc", "rustc_alias"),
        capture("mise --no-env --locked --no-hooks which cargo", "cargo_alias"),
        capture("/bin/realpath \"$rustc_alias\"", "rustc_alias_real"),
        capture("/bin/realpath \"$cargo_alias\"", "cargo_alias_real"),
        "test \"$rustc_alias_real\" = \"$rustup_path\" && test \"$cargo_alias_real\" = \"$rustup_path\"".to_owned(),
        capture(&format!("mise --no-env --locked --no-hooks exec rust@{version} -- rustup show active-toolchain"), "active_toolchain"),
        "case \"$active_toolchain\" in \"$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN \"*) ;; *) exit 1 ;; esac".to_owned(),
        capture(&format!("mise --no-env --locked --no-hooks exec rust@{version} -- rustup which --toolchain \"$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN\" rustc"), "rustc_real"),
        capture(&format!("mise --no-env --locked --no-hooks exec rust@{version} -- rustup which --toolchain \"$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN\" cargo"), "cargo_real"),
        "toolchain_bin=\"$rustup_home_physical/toolchains/$VELNOR_EXPECTED_RUSTUP_TOOLCHAIN/bin\"".to_owned(),
        capture("/bin/realpath \"$toolchain_bin\"", "toolchain_bin_real"),
        "test \"$toolchain_bin_real\" = \"$toolchain_bin\"".to_owned(),
        "test \"$rustc_real\" = \"$toolchain_bin/rustc\" && test \"$cargo_real\" = \"$toolchain_bin/cargo\"".to_owned(),
        "for executable in \"$rustc_real\" \"$cargo_real\"; do test -f \"$executable\" && test -O \"$executable\" && test -x \"$executable\" && test ! -L \"$executable\" || exit 1; done".to_owned(),
        capture("/bin/realpath \"$rustc_real\"", "rustc_canonical"),
        capture("/bin/realpath \"$cargo_real\"", "cargo_canonical"),
        "test \"$rustc_canonical\" = \"$rustc_real\" && test \"$cargo_canonical\" = \"$cargo_real\"".to_owned(),
        capture(&format!("mise --no-env --locked --no-hooks exec rust@{version} -- \"$mbx_path\" exec --project-root \"$workspace_root\" \"$rustc_real\" --version"), "rustc_version"),
        format!("case \"$rustc_version\" in 'rustc {version} ('*) ;; *) exit 1 ;; esac"),
        capture(&format!("mise --no-env --locked --no-hooks exec rust@{version} -- cargo --version"), "cargo_version"),
        format!("case \"$cargo_version\" in 'cargo {version} ('*) ;; *) exit 1 ;; esac"),
        capture("tool_identity \"$rustup_path\"", "rustup_identity"),
        capture("tool_identity \"$rustc_real\"", "rustc_identity"),
        capture("tool_identity \"$cargo_real\"", "cargo_identity"),
    ]
}

pub(super) fn recheck(rust: bool) -> Vec<String> {
    let mut checks = vec![
        capture("/bin/realpath \"$mbx_alias\"", "mbx_after"),
        capture("tool_identity \"$mbx_path\"", "mbx_identity_after"),
        "test \"$mbx_after\" = \"$mbx_path\" && test \"$mbx_identity_after\" = \"$mbx_identity\""
            .to_owned(),
    ];
    if rust {
        for (producer, name) in [
            ("/bin/realpath \"$rustc_alias\"", "rustc_alias_after"),
            ("/bin/realpath \"$cargo_alias\"", "cargo_alias_after"),
            ("tool_identity \"$rustup_path\"", "rustup_identity_after"),
            ("tool_identity \"$rustc_real\"", "rustc_identity_after"),
            ("tool_identity \"$cargo_real\"", "cargo_identity_after"),
        ] {
            checks.push(capture(producer, name));
        }
        checks.push("test \"$rustc_alias_after\" = \"$rustup_path\" && test \"$cargo_alias_after\" = \"$rustup_path\" && test \"$rustup_identity_after\" = \"$rustup_identity\" && test \"$rustc_identity_after\" = \"$rustc_identity\" && test \"$cargo_identity_after\" = \"$cargo_identity\"".to_owned());
    }
    checks
}

pub(super) fn ordinary_mbx(mbx_version: &str) -> Vec<String> {
    vec![
        capture_command(
            "mise --no-env --no-hooks which mbx",
            "$task_root/mbx_path.txt",
            "mbx_path",
        ),
        format!(
            "case \"$mbx_path\" in \"$MISE_DATA_DIR/installs/mr-boxington/{mbx_version}/\"*) ;; *) exit 1 ;; esac"
        ),
        "test -x \"$mbx_path\"".to_owned(),
        "test ! -L \"$mbx_path\"".to_owned(),
        "mbx_dir=\"${mbx_path%/*}\"".to_owned(),
        "cd -P \"$mbx_dir\"".to_owned(),
        "mbx_parent=\"$PWD\"".to_owned(),
        "cd -P \"$workspace_root\"".to_owned(),
        "mbx_base=\"${mbx_path##*/}\"".to_owned(),
        "test \"$mbx_path\" = \"$mbx_parent/$mbx_base\"".to_owned(),
        "cd -P \"$task_working_directory\"".to_owned(),
    ]
}
