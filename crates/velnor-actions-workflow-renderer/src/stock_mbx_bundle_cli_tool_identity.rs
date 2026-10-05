use super::*;

pub(super) fn official_binary() -> Result<PathBuf, String> {
    let path = std::env::var_os(BINARY_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("unset {BINARY_ENV}"))?;
    let info = fs::symlink_metadata(&path).map_err(|err| err.to_string())?;
    if !path.is_absolute() || !info.is_file() || info.file_type().is_symlink() {
        return Err(format!(
            "{BINARY_ENV} must be an absolute regular file: {}",
            path.display()
        ));
    }
    let binary = path.canonicalize().map_err(|err| err.to_string())?;
    let digest = sha256(&binary)?;
    if digest != BINARY_SHA256 {
        return Err(format!(
            "MBX digest mismatch: expected {BINARY_SHA256}, got {digest}"
        ));
    }
    let version = Command::new(&binary)
        .arg("--version")
        .env_clear()
        .output()
        .map_err(|err| err.to_string())?;
    let version_text = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if !version.status.success() || version_text != "mbx 1.21.1" {
        return Err(format!(
            "expected MBX 1.21.1 source={MBX_SOURCE}; got {version_text}"
        ));
    }
    eprintln!(
        "official MBX: path={} version={version_text} source={MBX_SOURCE} archive={MBX_ARCHIVE} archive_sha256={MBX_ARCHIVE_SHA256} executable_sha256={digest} manifest_sha256={MBX_MANIFEST_SHA256}",
        binary.display(),
    );
    Ok(binary)
}

pub(super) fn official_mise_binary() -> Result<PathBuf, String> {
    let path = std::env::var_os(MISE_BINARY_ENV)
        .map(PathBuf::from)
        .ok_or_else(|| format!("unset {MISE_BINARY_ENV}"))?;
    let metadata = fs::symlink_metadata(&path).map_err(|err| err.to_string())?;
    if !path.is_absolute() || !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!(
            "{MISE_BINARY_ENV} must be an absolute regular file"
        ));
    }
    let binary = path.canonicalize().map_err(|err| err.to_string())?;
    let digest = sha256(&binary)?;
    if digest != MISE_BINARY_SHA256 {
        return Err(format!(
            "Mise digest mismatch: expected {MISE_BINARY_SHA256}, got {digest}"
        ));
    }
    let version = Command::new(&binary)
        .arg("--version")
        .env_clear()
        .output()
        .map_err(|err| err.to_string())?;
    let version_text = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if !version.status.success() || !version_text.starts_with("2026.10.2 ") {
        return Err(format!(
            "expected Mise 2026.10.2 source={MISE_SOURCE}; got {version_text}"
        ));
    }
    eprintln!(
        "official Mise: path={} version={version_text} source={MISE_SOURCE} archive_sha256={MISE_ARCHIVE_SHA256} executable_sha256={digest}",
        binary.display()
    );
    Ok(binary)
}

pub(super) fn official_cargo_binary(stage: &Stage) -> Result<PathBuf, String> {
    let mise = official_mise_binary()?;
    let output_file = stage.root.join("cargo-path.output");
    let mut resolve_rust = stage.command(
        mise.to_str().ok_or("non-UTF-8 Mise path")?,
        &output_file,
        &stage.temp,
    );
    let rust_root = resolve_rust
        .args(["where", "rust@1.98.1"])
        .output()
        .map_err(|err| err.to_string())?;
    if !rust_root.status.success() {
        return Err(format!(
            "Mise could not resolve Rust 1.98.1: {}",
            super::runner::text(&rust_root)
        ));
    }
    let rustup_root = PathBuf::from(String::from_utf8_lossy(&rust_root.stdout).trim());
    let rustup = rustup_root.join("rustup");
    if !rustup.is_absolute() || !rustup.is_file() {
        return Err(format!(
            "Mise Rust 1.98.1 install omitted its Rustup proxy: {}",
            rustup.display()
        ));
    }
    let resolved = stage
        .command(
            rustup.to_str().ok_or("non-UTF-8 Rustup path")?,
            &output_file,
            &stage.temp,
        )
        .args(["which", "cargo", "--toolchain", "1.98.1"])
        .output()
        .map_err(|err| err.to_string())?;
    if !resolved.status.success() {
        return Err(format!(
            "Rustup could not resolve pinned Cargo 1.98.1: {}",
            super::runner::text(&resolved)
        ));
    }
    let path = PathBuf::from(String::from_utf8_lossy(&resolved.stdout).trim());
    let cargo = path.canonicalize().map_err(|err| err.to_string())?;
    if !cargo.is_absolute() || !cargo.is_file() {
        return Err(format!(
            "Mise resolved invalid Cargo path: {}",
            cargo.display()
        ));
    }
    let version = stage
        .command(
            cargo.to_str().ok_or("non-UTF-8 Cargo path")?,
            &output_file,
            &stage.temp,
        )
        .arg("--version")
        .output()
        .map_err(|err| err.to_string())?;
    let version_text = String::from_utf8_lossy(&version.stdout).trim().to_owned();
    if !version.status.success() || !version_text.starts_with("cargo 1.98.1 ") {
        return Err(format!("expected pinned Cargo 1.98.1, got {version_text}"));
    }
    let digest = sha256(&cargo)?;
    eprintln!(
        "official Cargo metadata child: path={} version={version_text} sha256={digest}",
        cargo.display()
    );
    Ok(cargo)
}

pub(super) fn mise_data_root() -> Result<PathBuf, String> {
    let root = std::env::var_os("MISE_DATA_DIR")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".local/share/mise"))
        })
        .ok_or("cannot resolve installed Mise data root")?;
    if !root.is_dir() {
        return Err(format!("Mise data root is absent: {}", root.display()));
    }
    root.canonicalize().map_err(|err| err.to_string())
}

pub(super) fn sha256(path: &Path) -> Result<String, String> {
    let result = Command::new("/usr/bin/sha256sum")
        .arg(path)
        .env_clear()
        .output()
        .map_err(|err| err.to_string())?;
    if !result.status.success() {
        return Err(format!("sha256sum failed for {}", path.display()));
    }
    String::from_utf8_lossy(&result.stdout)
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or("sha256sum returned no digest".to_owned())
}

pub(super) fn write_project(project: &Path) -> Result<(), String> {
    let app = project.join("app");
    let lib = project.join("probe");
    for path in [app.join("src"), lib.join("src")] {
        fs::create_dir_all(path).map_err(|err| err.to_string())?;
    }
    for (path, text) in [
        (
            project.join("Cargo.toml"),
            "[workspace]\nmembers = [\"app\", \"probe\"]\nresolver = \"3\"\n",
        ),
        (project.join("Cargo.lock"), LOCK),
        (
            app.join("Cargo.toml"),
            "[package]\nname = \"mbx-cache-probe-app\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nmbx-cache-probe-lib = { path = \"../probe\" }\n",
        ),
        (
            app.join("src/main.rs"),
            "fn main() { println!(\"{}\", mbx_cache_probe_lib::cached_value()); }\n",
        ),
        (
            lib.join("Cargo.toml"),
            "[package]\nname = \"mbx-cache-probe-lib\"\nversion = \"0.1.0\"\nedition = \"2024\"\n",
        ),
        (
            lib.join("src/lib.rs"),
            "pub fn cached_value() -> u64 { 42 }\n",
        ),
    ] {
        fs::write(path, text).map_err(|err| err.to_string())?;
    }
    Ok(())
}
