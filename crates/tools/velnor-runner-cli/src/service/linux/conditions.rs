#[cfg(test)]
use super::ManagerOutput;
use super::systemd::parse_identity_marker_condition;
use super::{IDENTITY_MARKER, Manager, ServiceFault};

const SYSTEMD_BUS_NAME: &str = "org.freedesktop.systemd1";
const UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";
const CONDITIONS_PROPERTY: &str = "Conditions";

pub(super) fn read_identity_marker_condition(
    manager: &mut impl Manager,
    unit_object_path: &str,
) -> Result<bool, ServiceFault> {
    let output = manager
        .busctl(&[
            "--system",
            "--timeout=5",
            "get-property",
            SYSTEMD_BUS_NAME,
            unit_object_path,
            UNIT_INTERFACE,
            CONDITIONS_PROPERTY,
        ])
        .map_err(|_| ServiceFault::Manager)?;
    if !output.success {
        return Err(ServiceFault::Manager);
    }
    parse_identity_marker_condition(&output.stdout, IDENTITY_MARKER)
        .ok_or(ServiceFault::UnknownState)
}

#[cfg(test)]
pub(super) fn condition_output(matches_marker: bool) -> ManagerOutput {
    let value = if matches_marker {
        format!("a(sbbsi) 1 \"ConditionPathExists\" false false \"{IDENTITY_MARKER}\" 0\n")
    } else {
        "a(sbbsi) 1 \"ConditionPathExists\" false false \"/tmp/other-marker\" 0\n".to_owned()
    };
    ManagerOutput {
        success: true,
        stdout: value.into_bytes(),
    }
}
