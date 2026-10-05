//! Exact output and scratch cleanup tests for the native MBX version guard.
use std::fs;

use super::{
    TempRoot, assert_version_scratch_removed, run_version_check, version_check_script, write_tool,
};

#[test]
fn native_action_version_guard_accepts_only_one_exact_complete_line()
-> Result<(), Box<dyn std::error::Error>> {
    let guard = version_check_script()?;
    for (name, output, want_success) in [
        (
            "exact version",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.22.0'\n"),
            true,
        ),
        (
            "wrong version",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1'\n"),
            false,
        ),
        ("empty output", Some("#!/bin/sh\nexit 0\n"), false),
        (
            "extra line",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.22.0' 'diagnostic'\n"),
            false,
        ),
        (
            "unterminated output",
            Some("#!/bin/sh\nprintf '%s' 'mbx 1.22.0'\n"),
            false,
        ),
        (
            "nonzero command",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.22.0'\nexit 2\n"),
            false,
        ),
        ("missing executable", None, false),
    ] {
        let root = TempRoot::new()?;
        let fake_bin = root.0.join("fake-bin");
        fs::create_dir(&fake_bin)?;
        write_tool(
            &fake_bin.join("mise"),
            "#!/bin/sh\nset -eu\n[ \"$1\" = --no-config ] && [ \"$2\" = --no-env ] && [ \"$3\" = --no-hooks ] || exit 2\n[ \"$4\" = exec ] && [ \"$5\" = rust@1.98.1 ] && [ \"$6\" = -- ] || exit 3\nshift 6\n[ \"$#\" -eq 2 ] && [ \"$1\" = mbx ] && [ \"$2\" = --version ] || exit 4\nexec \"$@\"\n",
        )?;
        if let Some(output) = output {
            write_tool(&fake_bin.join("mbx"), output)?;
        }
        let result = run_version_check(&root.0, &guard)?;
        assert_eq!(
            result.status.success(),
            want_success,
            "{name}: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_version_scratch_removed(&root.0)?;
    }
    Ok(())
}
