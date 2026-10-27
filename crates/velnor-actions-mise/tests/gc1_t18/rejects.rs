// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;
use std::ffi::OsString;

use velnor_actions_mise::compiler_observer::{
    RUSTC_WRAPPER_ENV, compose_observer_env, observer_env,
};
use velnor_actions_mise::native_compiler_observer::{
    NativeCompletion, NativeEvent, NativeLedger, TOOL_KIND_CC, native_observer_env,
    resolve_native_tool_kind, select_native_adapter,
};

fn check_rejects_bad_native_identity() -> Result<(), Box<dyn Error>> {
    let mut invocation = native_invocation("event-bad-1", TOOL_KIND_CC, CC_ADAPTER);
    for field in ["event_id", "recipe_id", "package_id"] {
        let mut bad = invocation.clone();
        match field {
            "event_id" => bad.event_id.clear(),
            "recipe_id" => bad.recipe_id.clear(),
            _ => bad.package_id.clear(),
        }
        if bad.validate().is_ok() {
            return Err(fail(&format!("empty {field} admitted")));
        }
    }
    "workspace".clone_into(&mut invocation.dependency_role);
    if invocation.validate().is_ok() {
        return Err(fail("unknown dependency role admitted"));
    }
    ROLE_DEPENDENCY.clone_into(&mut invocation.dependency_role);
    "../escape".clone_into(&mut invocation.lane_id);
    if invocation.validate().is_ok() {
        return Err(fail("escaping lane admitted"));
    }
    "build".clone_into(&mut invocation.lane_id);
    "gcc".clone_into(&mut invocation.tool_kind);
    if invocation.validate().is_ok() {
        return Err(fail("raw compiler name admitted as tool kind"));
    }
    TOOL_KIND_CC.clone_into(&mut invocation.tool_kind);
    invocation.executable.clear();
    if invocation.validate().is_ok() {
        return Err(fail("empty executable admitted"));
    }
    CC_ADAPTER.clone_into(&mut invocation.executable);
    invocation.argv.clear();
    if invocation.validate().is_ok() {
        return Err(fail("empty argv admitted"));
    }
    invocation.argv = native_argv();
    invocation.argv.push("a\0b".to_owned());
    if invocation.validate().is_ok() {
        return Err(fail("NUL flag admitted"));
    }
    invocation.argv = native_argv();
    invocation.adapter_chain = vec![String::new()];
    if invocation.validate().is_ok() {
        return Err(fail("empty adapter entry admitted"));
    }
    invocation.adapter_chain = vec![CC_ADAPTER.to_owned()];
    invocation.parent_event_id = Some(String::new());
    if invocation.validate().is_ok() {
        return Err(fail("empty parent event admitted"));
    }
    invocation.parent_event_id = None;
    "cached".clone_into(&mut invocation.operation_kind);
    if invocation.validate().is_ok() {
        return Err(fail("cache claim admitted as operation"));
    }
    OPERATION_COMPILATION.clone_into(&mut invocation.operation_kind);
    invocation.cwd.clear();
    if invocation.validate().is_ok() {
        return Err(fail("empty cwd admitted"));
    }
    Ok(())
}

fn check_rejects_bad_adapter_state() -> Result<(), Box<dyn Error>> {
    if native_observer_env("", CXX_ADAPTER, None, None).is_ok() {
        return Err(fail("empty CC adapter admitted"));
    }
    if native_observer_env("rel/cc", CXX_ADAPTER, None, None).is_ok() {
        return Err(fail("relative CC adapter admitted"));
    }
    if native_observer_env("a\0b", CXX_ADAPTER, None, None).is_ok() {
        return Err(fail("NUL CC adapter admitted"));
    }
    if native_observer_env(CC_ADAPTER, CC_ADAPTER, None, None).is_ok() {
        return Err(fail("shared CC and CXX adapter admitted"));
    }
    if native_observer_env(CC_ADAPTER, CXX_ADAPTER, Some(""), None).is_ok() {
        return Err(fail("empty previous CC admitted"));
    }
    if native_observer_env(CC_ADAPTER, CXX_ADAPTER, Some("rel/cc"), None).is_ok() {
        return Err(fail("relative previous CC admitted"));
    }
    if select_native_adapter("rustc", CC_ADAPTER, CXX_ADAPTER).is_ok() {
        return Err(fail("rust tool kind selected a native adapter"));
    }
    if select_native_adapter(TOOL_KIND_CC, "", CXX_ADAPTER).is_ok() {
        return Err(fail("empty CC adapter selected"));
    }
    if resolve_native_tool_kind(CC_ADAPTER, CC_ADAPTER, CC_ADAPTER).is_ok() {
        return Err(fail("ambiguous adapters resolved a tool kind"));
    }
    if resolve_native_tool_kind("", CC_ADAPTER, CXX_ADAPTER).is_ok() {
        return Err(fail("empty executable resolved a tool kind"));
    }
    Ok(())
}

fn check_rejects_bad_native_completion() -> Result<(), Box<dyn Error>> {
    let both = NativeCompletion {
        exit_code: Some(1),
        signal: Some(9),
        interrupted: false,
    };
    if both.validate().is_ok() {
        return Err(fail("exit code and signal conflated"));
    }
    let overflow = NativeCompletion {
        exit_code: Some(256),
        signal: None,
        interrupted: false,
    };
    if overflow.validate().is_ok() {
        return Err(fail("out-of-range exit code admitted"));
    }
    let bad_signal = NativeCompletion {
        exit_code: None,
        signal: Some(0),
        interrupted: false,
    };
    if bad_signal.validate().is_ok() {
        return Err(fail("out-of-range signal admitted"));
    }
    let missing = NativeCompletion {
        exit_code: None,
        signal: None,
        interrupted: false,
    };
    if missing.validate().is_ok() {
        return Err(fail("completion without terminal status admitted"));
    }
    let conflicted = NativeCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: true,
    };
    if conflicted.validate().is_ok() {
        return Err(fail("interrupted completion with status admitted"));
    }
    if (NativeCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: false,
    })
    .succeeded()
    {
        return Err(fail("failing native exit reported as success"));
    }
    Ok(())
}

fn check_rejects_native_ledger_and_env_misuse() -> Result<(), Box<dyn Error>> {
    let mut ledger = NativeLedger::new();
    ledger.record(NativeEvent {
        invocation: native_invocation("event-dup-1", TOOL_KIND_CC, CC_ADAPTER),
        completion: None,
    })?;
    if ledger
        .record(NativeEvent {
            invocation: native_invocation("event-dup-1", TOOL_KIND_CC, CC_ADAPTER),
            completion: None,
        })
        .is_ok()
    {
        return Err(fail("duplicate native event admitted"));
    }
    if ledger
        .complete(
            "event-missing",
            NativeCompletion {
                exit_code: Some(0),
                signal: None,
                interrupted: false,
            },
        )
        .is_ok()
    {
        return Err(fail("completion of unknown native event admitted"));
    }
    ledger.complete(
        "event-dup-1",
        NativeCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    if ledger
        .complete(
            "event-dup-1",
            NativeCompletion {
                exit_code: Some(0),
                signal: None,
                interrupted: false,
            },
        )
        .is_ok()
    {
        return Err(fail("double native completion admitted"));
    }
    if ledger.record_unobserved_absolute("cc").is_ok() {
        return Err(fail("bare executable counted as absolute call"));
    }
    if ledger.record_unobserved_absolute("").is_ok() {
        return Err(fail("empty executable counted as absolute call"));
    }
    let rust = observer_env(
        "/opt/observer/rustc-observer",
        "/opt/observer/rustdoc-observer",
        None,
        None,
    )?;
    let conflicting = vec![(
        OsString::from(RUSTC_WRAPPER_ENV),
        OsString::from(CC_ADAPTER),
    )];
    if compose_observer_env(rust.clone(), conflicting).is_ok() {
        return Err(fail("conflicting observer key merged"));
    }
    let empty_key = vec![(OsString::new(), OsString::from(CC_ADAPTER))];
    if compose_observer_env(rust, empty_key).is_ok() {
        return Err(fail("empty observer key merged"));
    }
    Ok(())
}

#[test]
fn gc1_t18_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_native_identity()?;
    check_rejects_bad_adapter_state()?;
    check_rejects_bad_native_completion()?;
    check_rejects_native_ledger_and_env_misuse()?;
    Ok(())
}
