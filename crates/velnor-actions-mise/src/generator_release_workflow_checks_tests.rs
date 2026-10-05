use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use velnor_actions_contract::{GeneratorReleaseTarget, Step, StepKind};

use super::{
    apple_linker_check_step, apple_sdk_check_step, binary_format_architecture_check_step,
    checked_binary_path, gnu_runtime_abi_check_step, help_smoke_check_step, native_host_check_step,
    rust_toolchain_check_step, shell_quote, version_smoke_check_step,
};
use crate::catalog::ToolCatalog;
use crate::steps::ToolHomes;

static NEXT_TEMP: AtomicUsize = AtomicUsize::new(0);

#[test]
fn check_steps_bind_native_runner_and_toolchain_identity() {
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    for (target, expected) in [
        (GeneratorReleaseTarget::LinuxX86_64, "Linux x86_64"),
        (GeneratorReleaseTarget::MacosArm64, "Darwin arm64"),
    ] {
        let host = native_host_check_step(target, &homes, &catalog).expect("host step");
        let host_script = script(&host);
        assert!(host_script.contains("uname -sm"));
        assert!(host_script.contains(expected));
        let rust = rust_toolchain_check_step(target, "1.98.1", &homes, &catalog)
            .expect("rust identity step");
        let rust_script = script(&rust);
        assert!(rust_script.contains("rustc -vV"));
        assert!(rust_script.contains("release: 1.98.1"));
        assert!(rust_script.contains(target.triple()));
    }
    assert!(
        rust_toolchain_check_step(
            GeneratorReleaseTarget::LinuxX86_64,
            "stable",
            &homes,
            &catalog,
        )
        .is_err()
    );
}

#[test]
fn binary_checks_reject_wrong_target_paths_and_runner_families() {
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let linux = GeneratorReleaseTarget::LinuxX86_64;
    let linux_binary = binary_path(linux);
    assert!(checked_binary_path(&linux_binary).is_ok());
    for unsafe_path in [
        Path::new(""),
        Path::new("/tmp/velnor-actions"),
        Path::new("target/../outside"),
        Path::new("./target/velnor-actions"),
        Path::new("target/./release/velnor-actions"),
        Path::new("target//release/velnor-actions"),
        Path::new("target\\release\\velnor-actions"),
    ] {
        assert!(checked_binary_path(unsafe_path).is_err());
    }
    let non_utf8 = PathBuf::from(std::ffi::OsString::from_vec(b"target/\xff".to_vec()));
    assert!(checked_binary_path(&non_utf8).is_err());
    assert!(
        gnu_runtime_abi_check_step(
            GeneratorReleaseTarget::MacosArm64,
            &binary_path(GeneratorReleaseTarget::MacosArm64),
            &homes,
            &catalog,
        )
        .is_err()
    );
    assert!(apple_sdk_check_step(linux, &homes, &catalog).is_err());
    assert!(apple_linker_check_step(linux, &homes, &catalog).is_err());
    assert!(version_smoke_check_step(&linux_binary, "0.1", &homes, &catalog).is_err());
}

#[test]
fn linux_elf_header_parser_normalizes_spacing_and_requires_exact_fields() {
    let root = TempDir::new();
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let step = binary_format_architecture_check_step(
        GeneratorReleaseTarget::LinuxX86_64,
        &binary_path(GeneratorReleaseTarget::LinuxX86_64),
        &homes,
        &catalog,
    )
    .expect("ELF check");
    write_output_command(
        root.path(),
        "readelf",
        "ELF Header:\n  Class:                             ELF64\n  Machine:                           Advanced Micro Devices X86-64\n",
    );
    assert!(run_step(&step, root.path(), root.path()));
    write_output_command(
        root.path(),
        "readelf",
        "  Class: ELF32\n  Machine: Advanced Micro Devices X86-64\n",
    );
    assert!(!run_step(&step, root.path(), root.path()));
    write_output_command(
        root.path(),
        "readelf",
        "  Class: ELF64\n  Machine: AArch64\n",
    );
    assert!(!run_step(&step, root.path(), root.path()));
}

#[test]
fn macos_binary_check_rejects_universal_and_wrong_architectures() {
    let root = TempDir::new();
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let target = GeneratorReleaseTarget::MacosArm64;
    let step =
        binary_format_architecture_check_step(target, &binary_path(target), &homes, &catalog)
            .expect("Mach-O check");
    write_output_command(root.path(), "file", "Mach-O 64-bit executable arm64\n");
    write_output_command(root.path(), "lipo", "arm64\n");
    assert!(run_step(&step, root.path(), root.path()));
    write_output_command(root.path(), "lipo", "arm64 x86_64\n");
    assert!(!run_step(&step, root.path(), root.path()));
    write_output_command(root.path(), "lipo", "x86_64\n");
    assert!(!run_step(&step, root.path(), root.path()));
}

#[test]
fn gnu_abi_parser_accepts_ubuntu_2204_and_fails_closed_on_unknown_or_new_tokens() {
    let root = TempDir::new();
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let target = GeneratorReleaseTarget::LinuxX86_64;
    let step = gnu_runtime_abi_check_step(target, &binary_path(target), &homes, &catalog)
        .expect("GNU ABI check");
    write_output_command(root.path(), "ldd", "libc.so.6 => /lib/libc.so.6\n");
    for version in ["GLIBC_2.2.5", "GLIBC_2.34", "GLIBC_2.35", "GLIBC_1.9"] {
        write_readelf_needs(root.path(), version);
        assert!(run_step(&step, root.path(), root.path()), "{version}");
    }
    for invalid in [
        "GLIBC_2.35.1",
        "GLIBC_2.35.0.1",
        "GLIBC_2.036",
        "GLIBC_PRIVATE",
        "GLIBC_999999.1",
    ] {
        write_readelf_needs(root.path(), invalid);
        assert!(!run_step(&step, root.path(), root.path()), "{invalid}");
    }
    write_output_command(root.path(), "readelf", "Version definition section only\n");
    assert!(!run_step(&step, root.path(), root.path()));
}

#[test]
fn gnu_abi_check_rejects_missing_dependencies_and_failed_readelf() {
    let root = TempDir::new();
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let target = GeneratorReleaseTarget::LinuxX86_64;
    let step = gnu_runtime_abi_check_step(target, &binary_path(target), &homes, &catalog)
        .expect("GNU ABI check");
    write_readelf_needs(root.path(), "GLIBC_2.35");
    write_output_command(root.path(), "ldd", "libmissing.so => not found\n");
    assert!(!run_step(&step, root.path(), root.path()));
    write_executable(root.path(), "readelf", "#!/bin/sh\nexit 9\n");
    assert!(!run_step(&step, root.path(), root.path()));
}

#[test]
fn apple_sdk_and_linker_checks_observe_real_executable_paths() {
    let root = TempDir::new();
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let target = GeneratorReleaseTarget::MacosArm64;
    let sdk = root.path().join("sdk");
    let clang = root.path().join("clang");
    let linker = root.path().join("ld");
    fs::create_dir(&sdk).expect("sdk directory");
    write_executable_at(&clang, "#!/bin/sh\nexit 0\n");
    write_executable_at(&linker, "#!/bin/sh\nexit 0\n");
    let xcrun = format!(
        "#!/bin/sh\ncase \"$*\" in\n  '--show-sdk-path') printf '%s\\n' {} ;;\n  '--find clang') printf '%s\\n' {} ;;\n  '--find ld') printf '%s\\n' {} ;;\n  *) exit 2 ;;\nesac\n",
        shell_quote(&sdk.display().to_string()),
        shell_quote(&clang.display().to_string()),
        shell_quote(&linker.display().to_string()),
    );
    write_executable(root.path(), "xcrun", &xcrun);
    let sdk_step = apple_sdk_check_step(target, &homes, &catalog).expect("SDK check");
    let linker_step = apple_linker_check_step(target, &homes, &catalog).expect("linker check");
    assert!(run_step(&sdk_step, root.path(), root.path()));
    assert!(run_step(&linker_step, root.path(), root.path()));
    write_executable(
        root.path(),
        "xcrun",
        "#!/bin/sh\nprintf '/missing/tool\\n'\n",
    );
    assert!(!run_step(&linker_step, root.path(), root.path()));
}

#[test]
fn candidate_version_and_help_smokes_execute_the_expected_public_cli() {
    let root = TempDir::new();
    let target = GeneratorReleaseTarget::LinuxX86_64;
    let binary = binary_path(target);
    let absolute = root.path().join(binary);
    fs::create_dir_all(absolute.parent().expect("binary parent")).expect("target directories");
    write_candidate(&absolute, "velnor-actions 0.1.1", valid_help());
    let homes = ToolHomes::runner_temp();
    let catalog = ToolCatalog::pinned();
    let version = version_smoke_check_step(&binary_path(target), "0.1.1", &homes, &catalog)
        .expect("version smoke");
    let help = help_smoke_check_step(&binary_path(target), &homes, &catalog).expect("help smoke");
    assert!(run_step(&version, root.path(), Path::new("/usr/bin:/bin")));
    assert!(run_step(&help, root.path(), Path::new("/usr/bin:/bin")));
    write_candidate(&absolute, "velnor-actions 0.1.0", valid_help());
    assert!(!run_step(&version, root.path(), Path::new("/usr/bin:/bin")));
    write_candidate(
        &absolute,
        "velnor-actions 0.1.1",
        "Usage: velnor-actions <COMMAND>\n",
    );
    assert!(!run_step(&help, root.path(), Path::new("/usr/bin:/bin")));
}

fn binary_path(target: GeneratorReleaseTarget) -> PathBuf {
    PathBuf::from("target")
        .join(target.triple())
        .join("release")
        .join("velnor-actions")
}

fn script(step: &Step) -> &str {
    let StepKind::Shell { run, .. } = &step.kind else {
        panic!("shell step expected");
    };
    run.last().expect("guard script")
}

fn run_step(step: &Step, current_dir: &Path, path: &Path) -> bool {
    Command::new("/bin/bash")
        .args(["-e", "-u", "-o", "pipefail", "-c", script(step)])
        .env("PATH", format!("{}:/usr/bin:/bin", path.display()))
        .current_dir(current_dir)
        .status()
        .expect("bash available")
        .success()
}

fn write_readelf_needs(root: &Path, version: &str) {
    let output = format!(
        "Version needs section '.gnu.version_r' contains 1 entry:\n File: libc.so.6 Cnt: 1\n  0x0010: Name: {version} Flags: none Version: 2\n"
    );
    write_output_command(root, "readelf", &output);
}

fn write_output_command(root: &Path, name: &str, output: &str) {
    let script = format!("#!/bin/sh\nprintf '%s\\n' {}\n", shell_quote(output));
    write_executable(root, name, &script);
}

fn write_candidate(path: &Path, version: &str, help: &str) {
    let script = format!(
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf '%s\\n' {} ;;\n  --help) printf '%s\\n' {} ;;\n  *) exit 2 ;;\nesac\n",
        shell_quote(version),
        shell_quote(help),
    );
    write_executable_at(path, &script);
}

fn valid_help() -> &'static str {
    "Usage: velnor-actions <COMMAND>\n\nCommands:\n  init\n  plan\n  generate\n  config\n"
}

fn write_executable(root: &Path, name: &str, script: &str) {
    write_executable_at(&root.join(name), script);
}

fn write_executable_at(path: &Path, script: &str) {
    fs::write(path, script).expect("write test command");
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).expect("executable test command");
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let serial = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "velnor-generator-release-{}-{serial}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("unique temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        match fs::remove_dir_all(&self.0) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) if std::thread::panicking() => {
                eprintln!("failed to clean up {}: {error}", self.0.display());
            }
            Err(error) => panic!("failed to clean up {}: {error}", self.0.display()),
        }
    }
}
