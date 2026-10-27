// SPDX-FileCopyrightText: 2026 Alexey Zhokhov
// SPDX-License-Identifier: Apache-2.0

use super::*;
use std::error::Error;

use velnor_actions_mise::compiler_observer::{
    CompilerCompletion, CompilerEvent, CompilerLedger, DOC_PHASE_COMPILATION, classify_tool_kind,
    observer_env,
};

fn check_rejects_bad_identity() -> Result<(), Box<dyn Error>> {
    let mut invocation = dependency_invocation("event-bad-1");
    for (field, mutate) in [
        ("event_id", "event_id"),
        ("recipe_id", "recipe_id"),
        ("package_id", "package_id"),
    ] {
        let mut bad = invocation.clone();
        match mutate {
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
        return Err(fail("unobserved tool kind admitted"));
    }
    if classify_tool_kind("/usr/bin/cc").is_ok() {
        return Err(fail("absolute native call classified as observed"));
    }
    "rustc".clone_into(&mut invocation.tool_kind);
    "cached".clone_into(&mut invocation.operation_kind);
    if invocation.validate().is_ok() {
        return Err(fail("cache claim admitted as operation"));
    }
    OPERATION_COMPILATION.clone_into(&mut invocation.operation_kind);
    Ok(())
}

fn check_rejects_bad_doc_state() -> Result<(), Box<dyn Error>> {
    let mut rustc = dependency_invocation("event-doc-bad-1");
    rustc.doc_phase = Some(DOC_PHASE_COMPILATION.to_owned());
    if rustc.validate().is_ok() {
        return Err(fail("doc phase admitted on rustc event"));
    }
    let mut rustdoc = rustdoc_invocation(
        "event-doc-bad-2",
        DOC_PHASE_COMPILATION,
        &["rustdoc", "--crate-name", "mise"],
    );
    rustdoc.doc_phase = None;
    if rustdoc.validate().is_ok() {
        return Err(fail("rustdoc event admitted without doc phase"));
    }
    let unknown = rustdoc_invocation(
        "event-doc-bad-3",
        "cached_doc",
        &["rustdoc", "--crate-name", "mise"],
    );
    if unknown.validate().is_ok() {
        return Err(fail("unknown doc phase admitted"));
    }
    let mut empty_argv = dependency_invocation("event-doc-bad-4");
    empty_argv.argv.clear();
    if empty_argv.validate().is_ok() {
        return Err(fail("empty argv admitted"));
    }
    let mut empty_wrapper = dependency_invocation("event-doc-bad-5");
    empty_wrapper.wrapper_chain = vec![String::new()];
    if empty_wrapper.validate().is_ok() {
        return Err(fail("empty wrapper entry admitted"));
    }
    Ok(())
}

fn check_rejects_bad_completion() -> Result<(), Box<dyn Error>> {
    let both = CompilerCompletion {
        exit_code: Some(1),
        signal: Some(9),
        interrupted: false,
    };
    if both.validate().is_ok() {
        return Err(fail("exit code and signal conflated"));
    }
    let overflow = CompilerCompletion {
        exit_code: Some(256),
        signal: None,
        interrupted: false,
    };
    if overflow.validate().is_ok() {
        return Err(fail("out-of-range exit code admitted"));
    }
    let bad_signal = CompilerCompletion {
        exit_code: None,
        signal: Some(0),
        interrupted: false,
    };
    if bad_signal.validate().is_ok() {
        return Err(fail("out-of-range signal admitted"));
    }
    let missing = CompilerCompletion {
        exit_code: None,
        signal: None,
        interrupted: false,
    };
    if missing.validate().is_ok() {
        return Err(fail("completion without terminal status admitted"));
    }
    let conflicted = CompilerCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: true,
    };
    if conflicted.validate().is_ok() {
        return Err(fail("interrupted completion with status admitted"));
    }
    if (CompilerCompletion {
        exit_code: Some(1),
        signal: None,
        interrupted: false,
    })
    .succeeded()
    {
        return Err(fail("failing exit reported as success"));
    }
    Ok(())
}

fn check_rejects_ledger_and_env_misuse() -> Result<(), Box<dyn Error>> {
    let mut ledger = CompilerLedger::new();
    ledger.record(CompilerEvent {
        invocation: dependency_invocation("event-dup-1"),
        completion: None,
    })?;
    if ledger
        .record(CompilerEvent {
            invocation: dependency_invocation("event-dup-1"),
            completion: None,
        })
        .is_ok()
    {
        return Err(fail("duplicate event admitted"));
    }
    if ledger
        .complete(
            "event-missing",
            CompilerCompletion {
                exit_code: Some(0),
                signal: None,
                interrupted: false,
            },
        )
        .is_ok()
    {
        return Err(fail("completion of unknown event admitted"));
    }
    ledger.complete(
        "event-dup-1",
        CompilerCompletion {
            exit_code: Some(0),
            signal: None,
            interrupted: false,
        },
    )?;
    if ledger
        .complete(
            "event-dup-1",
            CompilerCompletion {
                exit_code: Some(0),
                signal: None,
                interrupted: false,
            },
        )
        .is_ok()
    {
        return Err(fail("double completion admitted"));
    }
    if observer_env("", "/opt/observer/rustdoc-observer", None, None).is_ok() {
        return Err(fail("empty observer admitted"));
    }
    if observer_env(
        "/opt/observer/rustc-observer",
        "/opt/observer/rustdoc-observer",
        Some(""),
        None,
    )
    .is_ok()
    {
        return Err(fail("empty previous wrapper admitted"));
    }
    Ok(())
}

#[test]
fn gc1_t17_rejects_invalid_state() -> Result<(), Box<dyn Error>> {
    check_rejects_bad_identity()?;
    check_rejects_bad_doc_state()?;
    check_rejects_bad_completion()?;
    check_rejects_ledger_and_env_misuse()?;
    Ok(())
}
