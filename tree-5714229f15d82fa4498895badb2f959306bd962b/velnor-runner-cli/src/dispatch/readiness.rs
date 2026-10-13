use velnor_runner_host::HostPlatform;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxWorkAdmission {
    NotProven,
}

impl LinuxWorkAdmission {
    const fn as_str(self) -> &'static str {
        match self {
            Self::NotProven => "not_proven",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinuxReadinessReason {
    ImageProfileAdmissionNotProven,
    AppArmorPolicyAdmissionNotProven,
    DurableControllerObservationUnavailable,
}

impl LinuxReadinessReason {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ImageProfileAdmissionNotProven => "image_profile_admission_not_proven",
            Self::AppArmorPolicyAdmissionNotProven => "apparmor_policy_admission_not_proven",
            Self::DurableControllerObservationUnavailable => {
                "durable_controller_observation_unavailable"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LinuxReadinessAssessment {
    work_admission: LinuxWorkAdmission,
    reasons: [LinuxReadinessReason; 3],
}

impl LinuxReadinessAssessment {
    /// Status has no atomic durable readiness observation yet. Keep all
    /// admission claims explicitly unproven until the State-owned snapshot API
    /// is wired into this CLI; local service/dependency observations cannot
    /// promote this result.
    const fn current() -> Self {
        Self {
            work_admission: LinuxWorkAdmission::NotProven,
            reasons: [
                LinuxReadinessReason::ImageProfileAdmissionNotProven,
                LinuxReadinessReason::AppArmorPolicyAdmissionNotProven,
                LinuxReadinessReason::DurableControllerObservationUnavailable,
            ],
        }
    }

    fn json(self) -> serde_json::Value {
        serde_json::json!({
            "controller_runtime": "unknown",
            "work_admission": self.work_admission.as_str(),
            "work_admission_reasons": self
                .reasons
                .map(LinuxReadinessReason::as_str),
        })
    }

    fn lines(self) -> String {
        let reasons = self.reasons.map(LinuxReadinessReason::as_str).join(",");
        format!(
            "\ncontroller_runtime=unknown\nwork_admission={}\nwork_admission_reasons={reasons}",
            self.work_admission.as_str(),
        )
    }
}

pub(super) fn add_linux_readiness(
    document: &mut serde_json::Value,
    platform: Option<HostPlatform>,
) {
    if platform == Some(HostPlatform::Linux) {
        document["readiness"] = LinuxReadinessAssessment::current().json();
    }
}

pub(super) fn readiness_lines(platform: Option<HostPlatform>) -> String {
    if platform == Some(HostPlatform::Linux) {
        LinuxReadinessAssessment::current().lines()
    } else {
        String::new()
    }
}
