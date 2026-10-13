//! Exact output and scratch cleanup tests for the native MBX version guard.
use std::fs;

use super::{
    TempRoot, assert_version_scratch_removed, run_version_check,
    run_version_check_with_cache_dir_mode, version_check_script, write_tool,
};

#[test]
fn native_action_version_guard_accepts_only_one_exact_complete_line()
-> Result<(), Box<dyn std::error::Error>> {
    let guard = version_check_script()?;
    for (name, output, want_success) in [
        (
            "exact version",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1'\n"),
            true,
        ),
        (
            "wrong version",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.22.0'\n"),
            false,
        ),
        ("empty output", Some("#!/bin/sh\nexit 0\n"), false),
        (
            "extra line",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1' 'diagnostic'\n"),
            false,
        ),
        (
            "unterminated output",
            Some("#!/bin/sh\nprintf '%s' 'mbx 1.21.1'\n"),
            false,
        ),
        (
            "nonzero command",
            Some("#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1'\nexit 2\n"),
            false,
        ),
        ("missing executable", None, false),
    ] {
        let root = TempRoot::new()?;
        let fake_bin = root.0.join("fake-bin");
        fs::create_dir(&fake_bin)?;
        write_tool(
            &fake_bin.join("mise"),
            "#!/bin/sh\nset -eu\n[ \"$1\" = --no-config ] && [ \"$2\" = --no-env ] && [ \"$3\" = --no-hooks ] || exit 2\n[ \"$4\" = exec ] && [ \"$5\" = rust@1.98.1 ] && [ \"$6\" = -- ] || exit 3\nif [ \"$7\" = mbx ] && [ \"$8\" = cache ] && [ \"$9\" = dir ]; then\ncase \"$MBX_CACHE_DIR_MODE\" in\nexact) printf '%s\\n' \"$MBX_CACHE_DIR/actions\" ;;\nwrong) printf '%s\\n' \"$MBX_CACHE_DIR/other\" ;;\nextra) printf '%s\\n' \"$MBX_CACHE_DIR/actions\" extra ;;\nunterminated) printf '%s' \"$MBX_CACHE_DIR/actions\" ;;\nnonzero) printf '%s\\n' \"$MBX_CACHE_DIR/actions\"; exit 2 ;;\nesac\nexit 0\nfi\nshift 6\n[ \"$#\" -eq 2 ] && [ \"$1\" = mbx ] && [ \"$2\" = --version ] || exit 4\nexec \"$@\"\n",
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

#[test]
fn native_action_cache_dir_guard_rejects_wrong_or_ambiguous_output()
-> Result<(), Box<dyn std::error::Error>> {
    let guard = version_check_script()?;
    for mode in ["wrong", "extra", "unterminated", "nonzero"] {
        let root = TempRoot::new()?;
        let fake_bin = root.0.join("fake-bin");
        fs::create_dir(&fake_bin)?;
        write_tool(
            &fake_bin.join("mise"),
            "#!/bin/sh\nset -eu\n[ \"$1\" = --no-config ] && [ \"$2\" = --no-env ] && [ \"$3\" = --no-hooks ] || exit 2\n[ \"$4\" = exec ] && [ \"$5\" = rust@1.98.1 ] && [ \"$6\" = -- ] || exit 3\nif [ \"$7\" = mbx ] && [ \"$8\" = cache ] && [ \"$9\" = dir ]; then\ncase \"$MBX_CACHE_DIR_MODE\" in\nwrong) printf '%s\\n' \"$MBX_CACHE_DIR/other\" ;;\nextra) printf '%s\\n' \"$MBX_CACHE_DIR/actions\" extra ;;\nunterminated) printf '%s' \"$MBX_CACHE_DIR/actions\" ;;\nnonzero) printf '%s\\n' \"$MBX_CACHE_DIR/actions\"; exit 2 ;;\nesac\nexit 0\nfi\nshift 6\n[ \"$#\" -eq 2 ] && [ \"$1\" = mbx ] && [ \"$2\" = --version ] || exit 4\nexec \"$@\"\n",
        )?;
        write_tool(
            &fake_bin.join("mbx"),
            "#!/bin/sh\nprintf '%s\\n' 'mbx 1.21.1'\n",
        )?;
        let result = run_version_check_with_cache_dir_mode(&root.0, &guard, mode)?;
        assert!(
            !result.status.success(),
            "{mode} cache-dir output must fail: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_version_scratch_removed(&root.0)?;
    }
    Ok(())
}
