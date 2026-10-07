//! Strict parsing for the exact systemd properties used by service commands.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UnitSnapshot {
    pub(super) load_state: String,
    pub(super) active_state: String,
    pub(super) sub_state: String,
    pub(super) main_pid: u32,
    pub(super) control_pid: u32,
    pub(super) result: String,
    pub(super) exec_start_pre: ExecInvocation,
    pub(super) exec_start: ExecInvocation,
    pub(super) exec_stop: ExecInvocation,
    pub(super) identity_marker_condition_matches: bool,
    pub(super) timeout_stop: StopTimeout,
    pub(super) user: String,
    pub(super) group: String,
    pub(super) supplementary_groups: String,
    pub(super) working_directory: String,
    pub(super) umask: String,
    pub(super) no_new_privileges: String,
    pub(super) protect_system: String,
    pub(super) read_write_paths: Vec<String>,
    pub(super) requires: Vec<String>,
    pub(super) after: Vec<String>,
    pub(super) unit_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct IdentityUnitSnapshot {
    pub(super) load_state: String,
    pub(super) exec_start: ExecInvocation,
    pub(super) user: String,
    pub(super) group: String,
    pub(super) umask: String,
    pub(super) no_new_privileges: String,
    pub(super) protect_system: String,
    pub(super) unit_type: String,
    pub(super) remain_after_exit: String,
    pub(super) before: Vec<String>,
    pub(super) identity_marker_condition_matches: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExecInvocation {
    pub(super) path: String,
    pub(super) argv: Vec<String>,
    pub(super) ignore_errors: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StopTimeout {
    Finite(u128),
    Infinite,
}

pub(super) fn parse_snapshot(output: &[u8]) -> Option<UnitSnapshot> {
    let text = std::str::from_utf8(output).ok()?;
    let mut load_state = None;
    let mut active_state = None;
    let mut sub_state = None;
    let mut main_pid = None;
    let mut control_pid = None;
    let mut result = None;
    let mut exec_start_pre = None;
    let mut exec_start = None;
    let mut exec_stop = None;
    let mut timeout_stop = None;
    let mut user = None;
    let mut group = None;
    let mut supplementary_groups = None;
    let mut working_directory = None;
    let mut umask = None;
    let mut no_new_privileges = None;
    let mut protect_system = None;
    let mut read_write_paths = None;
    let mut requires = None;
    let mut after = None;
    let mut unit_type = None;
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "LoadState" => set_once(&mut load_state, value)?,
            "ActiveState" => set_once(&mut active_state, value)?,
            "SubState" => set_once(&mut sub_state, value)?,
            "MainPID" => set_once(&mut main_pid, value.parse::<u32>().ok()?)?,
            "ControlPID" => set_once(&mut control_pid, value.parse::<u32>().ok()?)?,
            "Result" => set_once(&mut result, value)?,
            "ExecStartPre" => set_once(&mut exec_start_pre, parse_exec_invocation(value)?)?,
            "ExecStart" => set_once(&mut exec_start, parse_exec_invocation(value)?)?,
            "ExecStop" => set_once(&mut exec_stop, parse_exec_invocation(value)?)?,
            "TimeoutStopUSec" => set_once(&mut timeout_stop, parse_stop_timeout(value)?)?,
            "User" => set_once(&mut user, value)?,
            "Group" => set_once(&mut group, value)?,
            "SupplementaryGroups" => set_once(&mut supplementary_groups, value)?,
            "WorkingDirectory" => set_once(&mut working_directory, value)?,
            "UMask" => set_once(&mut umask, value)?,
            "NoNewPrivileges" => set_once(&mut no_new_privileges, value)?,
            "ProtectSystem" => set_once(&mut protect_system, value)?,
            "ReadWritePaths" => set_once(&mut read_write_paths, words(value))?,
            "Requires" => set_once(&mut requires, words(value))?,
            "After" => set_once(&mut after, words(value))?,
            "Type" => set_once(&mut unit_type, value)?,
            _ => return None,
        }
    }
    Some(UnitSnapshot {
        load_state: load_state?.to_owned(),
        active_state: active_state?.to_owned(),
        sub_state: sub_state?.to_owned(),
        main_pid: main_pid?,
        control_pid: control_pid?,
        result: result?.to_owned(),
        exec_start_pre: exec_start_pre?,
        exec_start: exec_start?,
        exec_stop: exec_stop?,
        identity_marker_condition_matches: false,
        timeout_stop: timeout_stop?,
        user: user?.to_owned(),
        group: group?.to_owned(),
        supplementary_groups: supplementary_groups?.to_owned(),
        working_directory: working_directory?.to_owned(),
        umask: umask?.to_owned(),
        no_new_privileges: no_new_privileges?.to_owned(),
        protect_system: protect_system?.to_owned(),
        read_write_paths: read_write_paths?,
        requires: requires?,
        after: after?,
        unit_type: unit_type?.to_owned(),
    })
}

pub(super) fn parse_identity_snapshot(output: &[u8]) -> Option<IdentityUnitSnapshot> {
    let text = std::str::from_utf8(output).ok()?;
    let mut load_state = None;
    let mut exec_start = None;
    let mut user = None;
    let mut group = None;
    let mut umask = None;
    let mut no_new_privileges = None;
    let mut protect_system = None;
    let mut unit_type = None;
    let mut remain_after_exit = None;
    let mut before = None;
    for line in text.lines() {
        let (key, value) = line.split_once('=')?;
        match key {
            "LoadState" => set_once(&mut load_state, value)?,
            "ExecStart" => set_once(&mut exec_start, parse_exec_invocation(value)?)?,
            "User" => set_once(&mut user, value)?,
            "Group" => set_once(&mut group, value)?,
            "UMask" => set_once(&mut umask, value)?,
            "NoNewPrivileges" => set_once(&mut no_new_privileges, value)?,
            "ProtectSystem" => set_once(&mut protect_system, value)?,
            "Type" => set_once(&mut unit_type, value)?,
            "RemainAfterExit" => set_once(&mut remain_after_exit, value)?,
            "Before" => set_once(&mut before, words(value))?,
            _ => return None,
        }
    }
    Some(IdentityUnitSnapshot {
        load_state: load_state?.to_owned(),
        exec_start: exec_start?,
        user: user?.to_owned(),
        group: group?.to_owned(),
        umask: umask?.to_owned(),
        no_new_privileges: no_new_privileges?.to_owned(),
        protect_system: protect_system?.to_owned(),
        unit_type: unit_type?.to_owned(),
        remain_after_exit: remain_after_exit?.to_owned(),
        before: before?,
        identity_marker_condition_matches: false,
    })
}

/// Parse `busctl get-property` output for the systemd `Unit.Conditions`
/// signature `a(sbbsi)`. The package unit is deliberately constrained to one
/// exact, non-triggering, non-negated identity-marker condition.
pub(super) fn parse_identity_marker_condition(output: &[u8], marker: &str) -> Option<bool> {
    let text = std::str::from_utf8(output).ok()?;
    let mut tokens = text.split_whitespace();
    if tokens.next()? != "a(sbbsi)" {
        return None;
    }
    let count = tokens.next()?.parse::<usize>().ok()?;
    if count != 1 {
        return (tokens.next().is_none()).then_some(false);
    }

    let condition_type = tokens.next()?;
    let trigger = tokens.next()?;
    let negate = tokens.next()?;
    let parameter = tokens.next()?;
    let result = tokens.next()?.parse::<i32>().ok()?;
    if tokens.next().is_some() || !matches!(result, -1..=1) {
        return None;
    }
    let expected_parameter = format!("\"{marker}\"");
    Some(
        condition_type == "\"ConditionPathExists\""
            && trigger == "false"
            && negate == "false"
            && parameter == expected_parameter,
    )
}

fn words(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_owned).collect()
}

fn set_once<T>(slot: &mut Option<T>, value: T) -> Option<()> {
    if slot.is_some() {
        return None;
    }
    *slot = Some(value);
    Some(())
}

fn parse_exec_invocation(value: &str) -> Option<ExecInvocation> {
    let inner = value.strip_prefix('{')?.strip_suffix('}')?;
    let mut path = None;
    let mut argv = None;
    let mut ignore_errors = None;
    for segment in inner.split(';') {
        let (key, value) = segment.trim().split_once('=')?;
        match key {
            "path" => set_once(&mut path, value.to_owned())?,
            "argv[]" => set_once(
                &mut argv,
                value
                    .split_whitespace()
                    .map(str::to_owned)
                    .collect::<Vec<_>>(),
            )?,
            "ignore_errors" => set_once(&mut ignore_errors, value.to_owned())?,
            "code" | "status" | "start_time" | "stop_time" | "pid" => {}
            _ => return None,
        }
    }
    Some(ExecInvocation {
        path: path?,
        argv: argv?,
        ignore_errors: ignore_errors?,
    })
}

pub(super) fn parse_timespan_usec(value: &str) -> Option<u128> {
    if value.trim().is_empty() {
        return None;
    }
    let mut total = 0_u128;
    for component in value.split_whitespace() {
        let split = component
            .find(|character: char| !character.is_ascii_digit())
            .unwrap_or(component.len());
        let amount = component.get(..split)?.parse::<u128>().ok()?;
        let unit = component.get(split..)?;
        let multiplier = match unit {
            "w" | "week" | "weeks" => 604_800_000_000,
            "d" | "day" | "days" => 86_400_000_000,
            "h" | "hr" | "hour" | "hours" => 3_600_000_000,
            "min" | "minute" | "minutes" => 60_000_000,
            "s" | "sec" | "second" | "seconds" => 1_000_000,
            "ms" | "msec" | "millisecond" | "milliseconds" => 1_000,
            "us" | "usec" | "microsecond" | "microseconds" => 1,
            _ => return None,
        };
        total = total.checked_add(amount.checked_mul(multiplier)?)?;
    }
    (total > 0).then_some(total)
}

pub(super) fn parse_stop_timeout(value: &str) -> Option<StopTimeout> {
    if value == "infinity" {
        return Some(StopTimeout::Infinite);
    }
    parse_timespan_usec(value).map(StopTimeout::Finite)
}
