//! Executable argument transport at the canonical test-only boundary.

use super::*;
#[test]
fn exact_upload_arguments_survive_executable_argument_transport() {
    let (api, fixture) = GitHubApi::with_fixture("example/app", Vec::new());
    let args = api
        .upload_arguments(
            "repos/example/app/releases/42/assets?name=release.json",
            Path::new("/private/original bytes/release.json"),
        )
        .expect("closed arguments");
    let output = std::process::Command::new("/bin/sh")
        .args(["-c", "printf '%s\\n' \"$@\"", "capture"])
        .args(&args)
        .output()
        .expect("neutral argument capture");
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).expect("argument lines"),
        "https://uploads.github.com/repos/example/app/releases/42/assets?name=release.json\n--method\nPOST\n--header\nContent-Type: application/octet-stream\n--input\n/private/original bytes/release.json\n"
    );
    fixture.assert_drained();
}
#[test]
fn arbitrary_upload_host_repository_query_and_id_are_rejected() {
    let (api, fixture) = GitHubApi::with_fixture("example/app", Vec::new());
    for route in [
        "https://attacker.invalid/anything",
        "repos/other/app/releases/42/assets?name=x",
        "repos/example/app/releases/0/assets?name=x",
        "repos/example/app/releases/042/assets?name=x",
        "repos/example/app/releases/42/assets?name=x&redirect=attacker",
        "repos/example/app/releases/42/assets?name=../x",
    ] {
        assert!(api.upload_arguments(route, Path::new("/unused")).is_err());
    }
    fixture.assert_drained();
}
