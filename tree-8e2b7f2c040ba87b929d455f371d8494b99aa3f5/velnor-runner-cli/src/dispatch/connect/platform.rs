use velnor_runner_host::HostPlatform;

pub(in crate::dispatch) fn host_platform_for(target_os: &str) -> Option<HostPlatform> {
    match target_os {
        "linux" => Some(HostPlatform::Linux),
        "macos" => Some(HostPlatform::Macos),
        _ => None,
    }
}

pub(super) fn host_platform() -> Option<HostPlatform> {
    host_platform_for(std::env::consts::OS)
}
