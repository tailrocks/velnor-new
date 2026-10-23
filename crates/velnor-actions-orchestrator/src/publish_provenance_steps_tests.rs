use super::{
    COSIGN_LINUX_AMD64_BYTES, COSIGN_LINUX_AMD64_SHA256, COSIGN_LINUX_ARM64_BYTES,
    COSIGN_LINUX_ARM64_SHA256, CosignPlatform, cosign_for_runner,
};

#[test]
fn each_hosted_linux_label_selects_the_matching_official_cosign_asset() {
    assert_eq!(COSIGN_LINUX_ARM64_BYTES, 136_437_761);
    assert_eq!(
        COSIGN_LINUX_ARM64_SHA256,
        "ade83b7aac62e131cacfe96d958948b2150cc9efec46a570a2649b8e27dedad2"
    );
    for label in ["ubuntu-22.04", "ubuntu-24.04", "ubuntu-26.04"] {
        let (pin, platform) = cosign_for_runner(label).expect("x64 runner pin");
        assert_eq!(platform, CosignPlatform::LinuxAmd64);
        assert_eq!(pin.sha256, COSIGN_LINUX_AMD64_SHA256);
        assert_eq!(pin.size, COSIGN_LINUX_AMD64_BYTES);
    }
    for label in ["ubuntu-22.04-arm", "ubuntu-24.04-arm", "ubuntu-26.04-arm"] {
        let (pin, platform) = cosign_for_runner(label).expect("arm runner pin");
        assert_eq!(platform, CosignPlatform::LinuxArm64);
        assert_eq!(pin.sha256, COSIGN_LINUX_ARM64_SHA256);
        assert_eq!(pin.size, COSIGN_LINUX_ARM64_BYTES);
    }
    assert!(cosign_for_runner("ubuntu-latest").is_err());
}
