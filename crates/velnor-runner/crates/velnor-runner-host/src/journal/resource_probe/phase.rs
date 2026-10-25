use crate::error::HostError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbePhase {
    Prepared,

    CreateRequested,

    ContainerCreated,

    StartRequested,

    Started,

    WaitRequested,

    Waited,

    LogsRequested,

    OutputObserved,

    StopRequested,

    RemoveRequested,

    Removed,

    Aborted,

    Quarantined,
}

impl ProbePhase {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Prepared => "prepared",
            Self::CreateRequested => "create_requested",
            Self::ContainerCreated => "container_created",
            Self::StartRequested => "start_requested",
            Self::Started => "started",
            Self::WaitRequested => "wait_requested",
            Self::Waited => "waited",
            Self::LogsRequested => "logs_requested",
            Self::OutputObserved => "output_observed",
            Self::StopRequested => "stop_requested",
            Self::RemoveRequested => "remove_requested",
            Self::Removed => "removed",
            Self::Aborted => "aborted",
            Self::Quarantined => "quarantined",
        }
    }

    pub(super) fn parse(value: &str) -> Result<Self, HostError> {
        match value {
            "prepared" => Ok(Self::Prepared),
            "create_requested" => Ok(Self::CreateRequested),
            "container_created" => Ok(Self::ContainerCreated),
            "start_requested" => Ok(Self::StartRequested),
            "started" => Ok(Self::Started),
            "wait_requested" => Ok(Self::WaitRequested),
            "waited" => Ok(Self::Waited),
            "logs_requested" => Ok(Self::LogsRequested),
            "output_observed" => Ok(Self::OutputObserved),
            "stop_requested" => Ok(Self::StopRequested),
            "remove_requested" => Ok(Self::RemoveRequested),
            "removed" => Ok(Self::Removed),
            "aborted" => Ok(Self::Aborted),
            "quarantined" => Ok(Self::Quarantined),
            _ => Err(HostError::Journal),
        }
    }

    pub(super) fn allows(self, next: Self) -> bool {
        if next == Self::Quarantined {
            return !matches!(self, Self::Removed | Self::Aborted);
        }
        matches!(
            (self, next),
            (Self::Prepared, Self::CreateRequested | Self::Aborted)
                | (Self::CreateRequested, Self::ContainerCreated)
                | (
                    Self::ContainerCreated,
                    Self::StartRequested | Self::RemoveRequested
                )
                | (
                    Self::StartRequested,
                    Self::Started
                        | Self::WaitRequested
                        | Self::StopRequested
                        | Self::RemoveRequested
                )
                | (
                    Self::Started,
                    Self::WaitRequested | Self::StopRequested | Self::RemoveRequested
                )
                | (
                    Self::WaitRequested,
                    Self::Waited | Self::StopRequested | Self::RemoveRequested
                )
                | (Self::Waited, Self::LogsRequested | Self::RemoveRequested)
                | (
                    Self::LogsRequested,
                    Self::OutputObserved | Self::RemoveRequested
                )
                | (
                    Self::OutputObserved | Self::StopRequested,
                    Self::RemoveRequested
                )
                | (Self::RemoveRequested, Self::Removed)
        )
    }
}
