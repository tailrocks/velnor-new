"""Generator-owned Developer ID signing; fixed tools, pinned identity, no hooks."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import tempfile

from desktop_native_core import path, run, safe_path, version_build
from desktop_native_verify import verify_app


def required(name):
    value = os.environ.get(name, "")
    if not value or value != value.strip() or "\n" in value or "\r" in value:
        raise RuntimeError(f"{name} must be nonempty and single-line")
    return value


def expectations():
    team = required("EXPECTED_TEAM_ID")
    if not re.fullmatch(r"[A-Z0-9]{10}", team):
        raise RuntimeError("EXPECTED_TEAM_ID must be ten uppercase letters/digits")
    cert = required("EXPECTED_CERT_SHA256").replace(":", "").lower()
    if not re.fullmatch(r"[0-9a-f]{64}", cert):
        raise RuntimeError("EXPECTED_CERT_SHA256 must contain 64 hexadecimal digits")
    identity = required("DEVELOPER_ID_APPLICATION")
    if not re.fullmatch(r"Developer ID Application: [^\r\n]+ \(" + team + r"\)", identity):
        raise RuntimeError("Developer ID Application identity must match expected team")
    return team, cert, identity


def temporary_root():
    root = Path(required("RUNNER_TEMP")).resolve(strict=True)
    if not root.is_dir():
        raise RuntimeError("RUNNER_TEMP must be an existing directory")
    return root


def notary_credentials(root):
    key = Path(required("APP_STORE_CONNECT_API_KEY_PATH"))
    if not key.is_absolute():
        raise RuntimeError("notary key path must be absolute")
    resolved = key.resolve(strict=True)
    try:
        relative = resolved.relative_to(root)
    except ValueError as error:
        raise RuntimeError("notary key must be inside RUNNER_TEMP") from error
    candidate = root
    for component in relative.parts:
        candidate = candidate / component
        if candidate.is_symlink():
            raise RuntimeError("notary key must not traverse symlinks")
    if key.is_symlink() or key.absolute() != resolved or not resolved.is_file():
        raise RuntimeError("notary key must be a regular file without symlinks")
    key_id = required("APP_STORE_CONNECT_KEY_ID")
    issuer = required("APP_STORE_CONNECT_ISSUER_ID")
    if not re.fullmatch(r"[A-Z0-9]{10}", key_id):
        raise RuntimeError("invalid Apple key identifier")
    if not re.fullmatch(r"[0-9a-fA-F]{8}(?:-[0-9a-fA-F]{4}){3}-[0-9a-fA-F]{12}", issuer):
        raise RuntimeError("invalid Apple issuer identifier")
    return resolved, key_id, issuer


def archive_path(profile, version):
    version_build(version, "1")
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+", version):
        raise RuntimeError("native release version must be stable X.Y.Z")
    app = path(profile, "apple.app_path")
    prefix = profile["apple"]["archive_name_prefix"]
    if not re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9._-]*", prefix):
        raise RuntimeError("invalid native archive prefix")
    output = app.parent / f"{prefix}-{version}-aarch64-apple-darwin.zip"
    return safe_path(profile["_root"], output.relative_to(profile["_root"]).as_posix())


def check_team(app, expected):
    output = run(["codesign", "-dv", "--verbose=4", str(app)])
    teams = [line.removeprefix("TeamIdentifier=").strip()
             for line in output.splitlines() if line.startswith("TeamIdentifier=")]
    if teams != [expected]:
        raise RuntimeError("signing TeamIdentifier mismatch or missing")


def check_certificate(app, expected, temporary):
    prefix = temporary / "leaf-cert"
    run(["codesign", "-d", f"--extract-certificates={prefix}", str(app)])
    leaf = Path(str(prefix) + "0")
    if leaf.is_symlink() or not leaf.is_file():
        raise RuntimeError("missing regular leaf signing certificate")
    if hashlib.sha256(leaf.read_bytes()).hexdigest() != expected:
        raise RuntimeError("signing certificate SHA-256 mismatch")


def check_entitlements(app):
    output = run(["codesign", "-d", "--entitlements", ":-", str(app)])
    start = output.find("<?xml")
    if start == -1:
        start = output.find("<plist")
    end = output.find("</plist>", start)
    if start == -1:
        diagnostics = [line for line in output.splitlines() if line.strip()]
        if not diagnostics or diagnostics == [f"Executable={app}"]:
            return
        raise RuntimeError("could not parse signing entitlements")
    if end == -1:
        raise RuntimeError("could not parse signing entitlements")
    entitlements = plistlib.loads(output[start:end + len("</plist>")].encode())
    if not isinstance(entitlements, dict):
        raise RuntimeError("signing entitlements must be a dictionary")
    if any(key.endswith("get-task-allow") for key in entitlements):
        raise RuntimeError("forbidden get-task-allow entitlement present")


def check_signature(app, team, cert, temporary):
    run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", str(app)])
    check_team(app, team)
    check_certificate(app, cert, temporary)
    check_entitlements(app)


def zip_app(app, output):
    if output.is_symlink():
        raise RuntimeError("ZIP destination must not be a symlink")
    if output.exists():
        output.unlink()
    run(["ditto", "-c", "-k", "--keepParent", app.name, str(output)], cwd=app.parent)
    if output.is_symlink() or not output.is_file():
        raise RuntimeError("ditto did not create a regular archive")


def sign(profile, version, build):
    version, build = version_build(version, build)
    team, cert, identity = expectations()
    runner = temporary_root()
    key, key_id, issuer = notary_credentials(runner)
    app = path(profile, "apple.app_path")
    output = archive_path(profile, version)
    verify_app(profile, version, build, app=app)
    with tempfile.TemporaryDirectory(prefix="velnor-native-sign-", dir=runner) as name:
        temporary = Path(name)
        run(["codesign", "--force", "--options", "runtime", "--timestamp",
             "--sign", identity, str(app)])
        check_signature(app, team, cert, temporary)
        submission = temporary / "submission.zip"
        zip_app(app, submission)
        response = run(["xcrun", "notarytool", "submit", str(submission),
                        "--key", str(key), "--key-id", key_id, "--issuer", issuer,
                        "--wait", "--output-format", "json"])
        status = json.loads(response)
        if not isinstance(status, dict) or status.get("status") != "Accepted":
            raise RuntimeError("notarization must return Accepted")
        run(["xcrun", "stapler", "staple", str(app)])
        run(["xcrun", "stapler", "validate", str(app)])
        check_signature(app, team, cert, temporary)
        run(["spctl", "--assess", "--type", "execute", "--verbose=4", str(app)])
        verify_app(profile, version, build, release=True, app=app)
        zip_app(app, output)
        verify_app(profile, version, build, release=True, zip_path=output, app=app)
        submission.unlink()
    print(json.dumps({"operation": "native-sign", "status": "Accepted",
                      "asset": output.name, "team": team, "certificate_sha256": cert}))
    return output
