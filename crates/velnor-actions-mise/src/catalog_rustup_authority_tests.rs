use super::{RustCompilerRole, RustHost, RustupBootstrap, RustupManagerAuthority};

#[test]
fn manager_metadata_is_closed_and_reuses_the_bootstrap_binding() {
    for (role, host) in [
        (RustCompilerRole::RootLinux, RustHost::LinuxAmd64),
        (RustCompilerRole::DesktopMac, RustHost::MacosArm64),
        (RustCompilerRole::DesktopSourceMac, RustHost::MacosArm64),
    ] {
        let authority = RustupManagerAuthority::for_role(role);
        let bootstrap = RustupBootstrap::for_host(host);
        assert_eq!(authority.role(), role);
        assert_eq!(
            authority.domain(),
            velnor_actions_contract::ToolCacheDomain::Full
        );
        assert_eq!(authority.host(), host);
        assert_eq!(authority.expected_path(), "velnor/cargo/bin/rustup");
        assert_eq!(authority.version(), bootstrap.version());
        assert_eq!(authority.sha256(), bootstrap.sha256());
        assert_eq!(authority.archive_url(), bootstrap.url());
        let qualification = authority.qualification_descriptor();
        assert!(
            qualification
                .source_url()
                .contains(qualification.source_commit())
        );
        assert_eq!(qualification.source_commit().len(), 40);
        assert_eq!(qualification.source_tree().len(), 40);
        assert_eq!(
            authority.selected_toolchain(),
            format!("{}-{}", authority.compiler_version(), host.target_triple())
        );
    }
}

#[test]
fn desktop_source_role_preserves_manager_but_removes_mbx_selector() -> Result<(), String> {
    let desktop = RustupManagerAuthority::for_role(RustCompilerRole::DesktopMac);
    let source = RustupManagerAuthority::for_role(RustCompilerRole::DesktopSourceMac);
    let root = RustupManagerAuthority::for_role(RustCompilerRole::RootLinux);
    assert_eq!(desktop.sha256(), source.sha256());
    assert_ne!(desktop.sha256(), root.sha256());
    assert_eq!(desktop.selected_toolchain(), source.selected_toolchain());
    assert_ne!(desktop.selected_toolchain(), root.selected_toolchain());
    let selector = |authority: RustupManagerAuthority| {
        authority
            .options()
            .tool_spec(authority.compiler_version())
            .map_err(|error| error.to_string())
    };
    assert!(selector(desktop)?.contains("mr_boxington=true"));
    assert!(!selector(source)?.contains("mr_boxington="));
    Ok(())
}
