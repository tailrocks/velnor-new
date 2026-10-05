//! Purpose-specific, fail-closed native checks for generator release binaries.

use std::ffi::{OsStr, OsString};
use std::path::Path;

use velnor_actions_contract::{GeneratorReleaseTarget, Step};

use crate::catalog::ToolCatalog;
use crate::error::MiseError;
use crate::steps::ToolHomes;

use super::{invalid_step_input, rust_exec_step};

const ELF_HEADER_GUARD_AWK: &str = r#"
{ print }
/^[[:space:]]*Class:/ {
  value = $0
  sub(/^[[:space:]]*Class:[[:space:]]*/, "", value)
  gsub(/[[:space:]]/, "", value)
  class = value
}
/^[[:space:]]*Machine:/ {
  value = $0
  sub(/^[[:space:]]*Machine:[[:space:]]*/, "", value)
  machine = value
}
END {
  if (class != "ELF64" || machine != "Advanced Micro Devices X86-64") exit 1
}
"#;

const GNU_ABI_GUARD_AWK: &str = r#"
{ print }
function canonical_component(value) {
  return value ~ /^(0|[1-9][0-9]?)$/;
}
function supported_glibc_name(value, version, count, part, major, minor, patch) {
  sub(/^GLIBC_/, "", value)
  count = split(value, version, ".")
  if (count < 2 || count > 3) return 0
  for (part = 1; part <= count; part++) {
    if (!canonical_component(version[part])) return 0
  }
  major = version[1] + 0
  minor = version[2] + 0
  patch = (count == 3) ? version[3] + 0 : 0
  if (major < 2) return 1
  if (major > 2 || minor > 35) return 0
  if (minor == 35 && patch > 0) return 0
  return 1
}
/^Version needs section / { in_needs = 1; saw_needs = 1; next }
/^Version/ { in_needs = 0 }
in_needs {
  field_count = split($0, fields, /[[:space:]]+/)
  for (field = 1; field < field_count; field++) {
    if (fields[field] != "Name:") continue
    name = fields[field + 1]
    if (name !~ /^GLIBC_/) continue
    count++
    if (!supported_glibc_name(name)) bad = 1
  }
}
END {
  if (bad || !saw_needs || count == 0) exit 1
}
"#;

/// Require the selected native runner OS and machine to match the target.
///
/// # Errors
///
/// Returns [`MiseError`] if the runner identity cannot be represented.
pub fn native_host_check_step(
    target: GeneratorReleaseTarget,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    let expected = match target {
        GeneratorReleaseTarget::LinuxX86_64 => "Linux x86_64",
        GeneratorReleaseTarget::MacosArm64 => "Darwin arm64",
    };
    let body = format!(
        "uname -sm | awk -v expected={} '{{ print \"Native host: \" $0; if (NR != 1 || $0 != expected) bad = 1 }} END {{ if (bad) exit 1 }}'",
        shell_quote(expected)
    );
    guarded_step("Verify native build host", &body, homes, catalog)
}

/// Require the exact pinned Rust release and target host triple.
///
/// # Errors
///
/// Returns [`MiseError`] for an unsupported version or invalid step input.
pub fn rust_toolchain_check_step(
    target: GeneratorReleaseTarget,
    rust_toolchain_version: &str,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    validate_exact_version("rust_toolchain_version", rust_toolchain_version)?;
    let body = format!(
        "rustc -vV | awk -v release={} -v host={} '{{ print; if ($0 == release) has_release = 1; if ($0 == host) has_host = 1 }} END {{ if (!has_release || !has_host) exit 1 }}'",
        shell_quote(&format!("release: {rust_toolchain_version}")),
        shell_quote(&format!("host: {}", target.triple()))
    );
    guarded_step("Verify selected Rust toolchain", &body, homes, catalog)
}

/// Require ELF64 x86-64 or one exact native Mach-O architecture.
///
/// # Errors
///
/// Returns [`MiseError`] for a mismatched target/path or invalid workflow input.
pub fn binary_format_architecture_check_step(
    target: GeneratorReleaseTarget,
    binary_relative_path: &Path,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    let binary = checked_binary_path(binary_relative_path)?;
    let body = match target {
        GeneratorReleaseTarget::LinuxX86_64 => format!(
            "readelf -hW {} | awk {}",
            shell_quote(&binary),
            shell_quote(ELF_HEADER_GUARD_AWK)
        ),
        GeneratorReleaseTarget::MacosArm64 => apple_format_script(&binary, "arm64"),
    };
    guarded_step(
        "Verify binary format and exact architecture",
        &body,
        homes,
        catalog,
    )
}

/// Require a GNU/Linux binary whose complete GLIBC version set fits Ubuntu 22.04.
///
/// # Errors
///
/// Returns [`MiseError`] for a non-Linux target or mismatched binary path.
pub fn gnu_runtime_abi_check_step(
    target: GeneratorReleaseTarget,
    binary_relative_path: &Path,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    require_target(target, GeneratorReleaseTarget::LinuxX86_64)?;
    let binary = checked_binary_path(binary_relative_path)?;
    let body = format!(
        "readelf --version-info --wide {} | awk {}; ldd -v {} | awk '/not found/ {{ bad = 1 }} {{ print }} END {{ if (bad) exit 1 }}'",
        shell_quote(&binary),
        shell_quote(GNU_ABI_GUARD_AWK),
        shell_quote(&binary)
    );
    guarded_step(
        "Verify Ubuntu 22.04 GNU ABI baseline",
        &body,
        homes,
        catalog,
    )
}

/// Observe the selected SDK and require its directory to exist.
///
/// # Errors
///
/// Returns [`MiseError`] for a non-Apple target.
pub fn apple_sdk_check_step(
    target: GeneratorReleaseTarget,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    require_apple_target(target)?;
    let body = "xcrun --show-sdk-path | awk '{ count++; sdk = $0 } END { if (count != 1 || sdk == \"\") exit 1; print sdk }' | while IFS= read -r sdk; do test -d \"$sdk\" || exit 1; printf 'Apple SDK: %s\\n' \"$sdk\"; done";
    guarded_step("Observe selected Apple SDK", body, homes, catalog)
}

/// Observe and require executable selected Apple Clang and linker paths.
///
/// # Errors
///
/// Returns [`MiseError`] for a non-Apple target.
pub fn apple_linker_check_step(
    target: GeneratorReleaseTarget,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    require_apple_target(target)?;
    let body = "{ xcrun --find clang || exit 1; xcrun --find ld || exit 1; } | awk 'NR == 1 { clang = $0 } NR == 2 { linker = $0 } END { if (NR != 2 || clang == \"\" || linker == \"\") exit 1; print clang; print linker }' | { IFS= read -r clang || exit 1; IFS= read -r linker || exit 1; test -x \"$clang\" || exit 1; test -x \"$linker\" || exit 1; printf 'Apple Clang: %s\\nApple linker: %s\\n' \"$clang\" \"$linker\"; }";
    guarded_step("Observe selected Apple linker", body, homes, catalog)
}

/// Execute the candidate and require its exact release version output.
///
/// # Errors
///
/// Returns [`MiseError`] for an invalid version or unsafe binary path.
pub fn version_smoke_check_step(
    binary_relative_path: &Path,
    version: &str,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    validate_exact_version("release_version", version)?;
    let binary = checked_binary_path(binary_relative_path)?;
    let body = format!(
        "{} --version | awk -v expected={} '{{ raw = raw $0 \"\\n\" }} END {{ sub(/\\n+$/, \"\", raw); if (raw != expected) exit 1; printf \"Binary version: %s\\n\", raw }}'",
        shell_quote(&binary),
        shell_quote(&format!("velnor-actions {version}"))
    );
    guarded_step("Smoke test release binary version", &body, homes, catalog)
}

/// Execute the candidate help path and require the public command surface.
///
/// # Errors
///
/// Returns [`MiseError`] for an unsafe binary path.
pub fn help_smoke_check_step(
    binary_relative_path: &Path,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    let binary = checked_binary_path(binary_relative_path)?;
    let body = format!(
        "{} --help | awk '{{ help = help $0 \"\\n\"; if ($0 ~ /^[[:space:]]+init([[:space:]]|$)/) has_init = 1; if ($0 ~ /^[[:space:]]+plan([[:space:]]|$)/) has_plan = 1; if ($0 ~ /^[[:space:]]+generate([[:space:]]|$)/) has_generate = 1; if ($0 ~ /^[[:space:]]+config([[:space:]]|$)/) has_config = 1 }} END {{ sub(/\\n+$/, \"\", help); printf \"%s\\n\", help; if (index(help, \"Usage: velnor-actions <COMMAND>\") == 0 || !has_init || !has_plan || !has_generate || !has_config) exit 1 }}'",
        shell_quote(&binary)
    );
    guarded_step("Smoke test release binary help", &body, homes, catalog)
}

fn apple_format_script(binary: &str, architecture: &str) -> String {
    format!(
        "file -b {} | awk -v expected={} '{{ description = description $0 \"\\n\" }} END {{ sub(/\\n+$/, \"\", description); printf \"Mach-O: %s\\n\", description; macho = index(description, \"Mach-O\"); arch = index(description, expected); if (macho == 0 || arch <= macho) exit 1 }}'; lipo -archs {} | awk -v expected={} '{{ architectures = architectures $0 \"\\n\" }} END {{ sub(/\\n+$/, \"\", architectures); if (architectures != expected) exit 1 }}'",
        shell_quote(binary),
        shell_quote(architecture),
        shell_quote(binary),
        shell_quote(architecture)
    )
}

fn guarded_step(
    name: &str,
    body: &str,
    homes: &ToolHomes,
    catalog: &ToolCatalog,
) -> Result<Step, MiseError> {
    // Workflow validation rejects controls and command substitution in every argv token.
    // Flatten the fixed Bash and AWK bodies into one line and stream captured output.
    let body = body
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("; ");
    let script = format!("set -euo pipefail; {body}");
    if script.chars().any(char::is_control) {
        return Err(invalid_step_input("script", "control_character"));
    }
    if script.contains("$(") || script.contains('`') {
        return Err(invalid_step_input("script", "command_substitution"));
    }
    let args = ["-e", "-u", "-o", "pipefail", "-c"]
        .into_iter()
        .map(OsString::from)
        .chain(std::iter::once(OsString::from(script)))
        .collect::<Vec<_>>();
    rust_exec_step(name, OsStr::new("bash"), &args, homes, catalog)
}

fn checked_binary_path(binary_relative_path: &Path) -> Result<String, MiseError> {
    let Some(value) = binary_relative_path.to_str() else {
        return Err(invalid_step_input("binary_relative_path", "non_utf8"));
    };
    let components_are_canonical = value
        .split('/')
        .all(|component| !component.is_empty() && component != "." && component != "..");
    if value.is_empty()
        || value.starts_with('/')
        || value.contains('\\')
        || value.bytes().any(|byte| byte.is_ascii_control())
        || !components_are_canonical
    {
        return Err(invalid_step_input("binary_relative_path", value));
    }
    Ok(format!("./{value}"))
}

fn validate_exact_version(field: &str, version: &str) -> Result<(), MiseError> {
    velnor_actions_contract::require_release_version(version, "generator-release")
        .map_err(|_| invalid_step_input(field, version))
}

fn require_target(
    actual: GeneratorReleaseTarget,
    expected: GeneratorReleaseTarget,
) -> Result<(), MiseError> {
    if actual == expected {
        Ok(())
    } else {
        Err(invalid_step_input("target", actual.triple()))
    }
}

fn require_apple_target(target: GeneratorReleaseTarget) -> Result<(), MiseError> {
    match target {
        GeneratorReleaseTarget::MacosArm64 => Ok(()),
        GeneratorReleaseTarget::LinuxX86_64 => Err(invalid_step_input("target", target.triple())),
    }
}

fn shell_quote(value: &str) -> String {
    let mut quoted = String::from("'");
    for character in value.chars() {
        if character == '\'' {
            quoted.push_str("'\"'\"'");
        } else {
            quoted.push(character);
        }
    }
    quoted.push('\'');
    quoted
}
