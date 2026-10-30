use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{RUNNER_PROFILE, runner_security_opt};

const MOBY_DEFAULT: &str = include_str!("seccomp/moby-default-v29.9.1.json");
const MOBY_DEFAULT_SHA256: &str =
    "785b2429264afba4d594320337cb17f144f3c7d51585f9805eef72e28f4f9334";
const CLONE_NEWTIME: u64 = 0x0000_0080;
const NAMESPACE_FLAGS: u64 = 0x7e02_0080;
const CAP_SYS_ADMIN_SYSCALLS: &[&str] = &[
    "clone",
    "clone3",
    "fsconfig",
    "fsmount",
    "fsopen",
    "fspick",
    "mount",
    "mount_setattr",
    "move_mount",
    "open_tree",
    "setns",
    "umount",
    "umount2",
    "unshare",
];

fn profile(value: &str) -> Result<Value, String> {
    serde_json::from_str(value).map_err(|error| error.to_string())
}

fn syscall_names(rule: &Value) -> Result<&[Value], String> {
    rule.get("names")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| "syscall names missing".to_owned())
}

fn has_name(rule: &Value, expected: &str) -> bool {
    syscall_names(rule).is_ok_and(|names| names.iter().any(|name| name.as_str() == Some(expected)))
}

fn remove_admin_exclusion(rule: &mut Value) -> Result<(), String> {
    let remove_excludes = {
        let excludes = rule
            .get_mut("excludes")
            .and_then(Value::as_object_mut)
            .ok_or_else(|| "clone cap exclusion missing".to_owned())?;
        let caps = excludes
            .get_mut("caps")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| "clone cap exclusion list missing".to_owned())?;
        let original_len = caps.len();
        caps.retain(|cap| cap.as_str() != Some("CAP_SYS_ADMIN"));
        if caps.len() == original_len {
            return Err("CAP_SYS_ADMIN exclusion absent".to_owned());
        }
        if caps.is_empty() {
            excludes.remove("caps");
        }
        excludes.is_empty()
    };
    if remove_excludes {
        rule.as_object_mut()
            .ok_or_else(|| "syscall rule is not an object".to_owned())?
            .remove("excludes");
    }
    Ok(())
}

fn derive_runner_profile() -> Result<Value, String> {
    let mut profile = profile(MOBY_DEFAULT)?;
    let rules = profile
        .get_mut("syscalls")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Moby syscall list missing".to_owned())?;
    let admin_index = rules
        .iter()
        .position(|rule| {
            rule.get("includes")
                .and_then(|includes| includes.get("caps"))
                .and_then(Value::as_array)
                .is_some_and(|caps| caps.iter().any(|cap| cap.as_str() == Some("CAP_SYS_ADMIN")))
                && CAP_SYS_ADMIN_SYSCALLS
                    .iter()
                    .any(|name| has_name(rule, name))
        })
        .ok_or_else(|| "Moby CAP_SYS_ADMIN syscall rule missing".to_owned())?;
    let names = rules[admin_index]
        .get_mut("names")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Moby CAP_SYS_ADMIN syscall names missing".to_owned())?;
    for denied in CAP_SYS_ADMIN_SYSCALLS {
        if !names.iter().any(|name| name.as_str() == Some(denied)) {
            return Err(format!("Moby CAP_SYS_ADMIN rule lacks {denied}"));
        }
    }
    names.retain(|name| {
        name.as_str()
            .is_none_or(|name| !CAP_SYS_ADMIN_SYSCALLS.contains(&name))
    });

    let rules = profile
        .get_mut("syscalls")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| "Moby syscall list missing".to_owned())?;
    let mut clone_count = 0;
    for rule in rules.iter_mut().filter(|rule| {
        syscall_names(rule)
            .is_ok_and(|names| names.len() == 1 && names[0].as_str() == Some("clone"))
            && rule.get("action").and_then(Value::as_str) == Some("SCMP_ACT_ALLOW")
            && rule.get("args").and_then(Value::as_array).is_some()
    }) {
        remove_admin_exclusion(rule)?;
        let argument = rule
            .get_mut("args")
            .and_then(Value::as_array_mut)
            .and_then(|args| args.first_mut())
            .ok_or_else(|| "Moby clone flag argument missing".to_owned())?;
        let value = argument
            .get("value")
            .and_then(Value::as_u64)
            .ok_or_else(|| "Moby clone flag mask missing".to_owned())?;
        argument
            .as_object_mut()
            .ok_or_else(|| "Moby clone flag argument is not an object".to_owned())?
            .insert("value".to_owned(), Value::from(value | CLONE_NEWTIME));
        clone_count += 1;
    }
    if clone_count != 2 {
        return Err("Moby clone architecture rules changed".to_owned());
    }

    let mut clone3_count = 0;
    for rule in rules.iter_mut().filter(|rule| {
        has_name(rule, "clone3")
            && rule.get("action").and_then(Value::as_str) == Some("SCMP_ACT_ERRNO")
            && rule.get("errnoRet").and_then(Value::as_u64) == Some(38)
    }) {
        remove_admin_exclusion(rule)?;
        clone3_count += 1;
    }
    if clone3_count != 1 {
        return Err("Moby clone3 ENOSYS rule changed".to_owned());
    }
    Ok(profile)
}

#[test]
fn pinned_moby_source_and_runner_derivative_are_exact() -> Result<(), String> {
    let digest = Sha256::digest(MOBY_DEFAULT.as_bytes());
    let digest_hex = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest_hex != MOBY_DEFAULT_SHA256 {
        return Err("pinned Moby default profile checksum changed".to_owned());
    }
    if derive_runner_profile()? != profile(RUNNER_PROFILE)? {
        return Err("runner profile differs from the pinned Moby-derived policy".to_owned());
    }
    Ok(())
}

#[test]
fn runner_profile_denies_namespace_and_mount_view_syscalls() -> Result<(), String> {
    let runner = profile(RUNNER_PROFILE)?;
    let rules = runner
        .get("syscalls")
        .and_then(Value::as_array)
        .ok_or_else(|| "runner syscall list missing".to_owned())?;
    let denied = [
        "unshare",
        "setns",
        "fsconfig",
        "fsmount",
        "fsopen",
        "fspick",
        "mount",
        "mount_setattr",
        "move_mount",
        "open_tree",
        "pivot_root",
        "umount",
        "umount2",
    ];
    for syscall in denied {
        if rules
            .iter()
            .any(|rule| has_name(rule, syscall) && rule["action"] == "SCMP_ACT_ALLOW")
        {
            return Err(format!("runner profile allows {syscall}"));
        }
    }
    if runner["defaultAction"] != "SCMP_ACT_ERRNO" {
        return Err("runner profile is not default-deny".to_owned());
    }
    Ok(())
}

#[test]
fn clone_keeps_thread_flags_but_rejects_all_namespace_flags() -> Result<(), String> {
    let runner = profile(RUNNER_PROFILE)?;
    let rules = runner
        .get("syscalls")
        .and_then(Value::as_array)
        .ok_or_else(|| "runner syscall list missing".to_owned())?;
    let clone_rules: Vec<&Value> = rules
        .iter()
        .filter(|rule| {
            syscall_names(rule)
                .is_ok_and(|names| names.len() == 1 && names[0].as_str() == Some("clone"))
                && rule["action"] == "SCMP_ACT_ALLOW"
                && rule.get("args").and_then(Value::as_array).is_some()
        })
        .collect();
    if clone_rules.len() != 2 {
        return Err("runner clone compatibility rules missing".to_owned());
    }
    for rule in clone_rules {
        let arg = rule["args"]
            .as_array()
            .and_then(|args| args.first())
            .ok_or_else(|| "clone flag argument missing".to_owned())?;
        if arg["op"] != "SCMP_CMP_MASKED_EQ"
            || arg["value"].as_u64() != Some(NAMESPACE_FLAGS)
            || arg.get("valueTwo").and_then(Value::as_u64).unwrap_or(0) != 0
        {
            return Err("clone namespace mask changed".to_owned());
        }
    }
    let thread_flags = 0x100 | 0x200 | 0x400 | 0x800 | 0x1_0000 | 0x4_0000;
    if thread_flags & NAMESPACE_FLAGS != 0 {
        return Err("thread/process clone flags are unexpectedly blocked".to_owned());
    }
    for namespace_flag in [
        0x0000_0080_u64,
        0x0002_0000_u64,
        0x0200_0000,
        0x0400_0000,
        0x0800_0000,
        0x1000_0000,
        0x2000_0000,
        0x4000_0000,
    ] {
        if namespace_flag & NAMESPACE_FLAGS == 0 {
            return Err("namespace flag missing from clone deny mask".to_owned());
        }
    }
    let clone3 = rules
        .iter()
        .find(|rule| has_name(rule, "clone3") && rule["action"] == "SCMP_ACT_ERRNO")
        .ok_or_else(|| "clone3 fallback rule missing".to_owned())?;
    if clone3["errnoRet"].as_u64() != Some(38) || clone3.get("excludes").is_some() {
        return Err("clone3 must return unconditional ENOSYS".to_owned());
    }
    Ok(())
}

#[test]
fn runner_seccomp_option_is_inline_and_pinned() -> Result<(), String> {
    let option = runner_security_opt();
    let json = option
        .strip_prefix("seccomp=")
        .ok_or_else(|| "runner seccomp option missing".to_owned())?;
    if json.contains('\n') || json.contains('\r') || profile(json)? != profile(RUNNER_PROFILE)? {
        return Err("runner seccomp option is not exact inline policy".to_owned());
    }
    Ok(())
}
