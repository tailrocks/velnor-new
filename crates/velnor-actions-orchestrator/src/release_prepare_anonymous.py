"""Anonymous pinned release-plz update; publish bounded immutable byte proposals."""
import base64
import hashlib
import stat
import tempfile
import tomllib

PREPARATION_CHANGELOG_CONFIG = '''# Pinned release-plz 0.3.169 defaults; explicit ambient-config override.
[changelog]
body = ""
trim = true
render_always = false
format = false
postprocessors = []
[git]
conventional_commits = true
require_conventional = false
filter_unconventional = false
split_commits = false
commit_preprocessors = []
commit_parsers = []
protect_breaking_commits = false
link_parsers = []
filter_commits = false
fail_on_unmatched_commit = false
use_branch_tags = false
topo_order = false
topo_order_commits = false
sort_commits = "newest"
recurse_submodules = false
'''


def _preparation_run(argv, root, environment):
    result = subprocess.run(argv, cwd=root, env=environment, capture_output=True,
                            check=False, timeout=900, shell=False)
    require(result.returncode == 0, "preparation_command_failed")
    require(len(result.stdout) <= 8 * 1024 * 1024, "preparation_command_output")
    return result.stdout


def _preparation_snapshot(root):
    result, modes = {}, {}
    for directory, directories, files in os.walk(root, followlinks=False):
        if Path(directory) == root:
            directories[:] = [name for name in directories if name != ".git"]
        links = [name for name in directories if (Path(directory) / name).is_symlink()]
        directories[:] = [name for name in directories if name not in links]
        for name in [*files, *links]:
            path = Path(directory) / name
            relative = path.relative_to(root).as_posix()
            mode = path.lstat().st_mode
            require(stat.S_ISREG(mode) or stat.S_ISLNK(mode), "preparation_source_file")
            content = os.fsencode(os.readlink(path)) if stat.S_ISLNK(mode) else path.read_bytes()
            require(len(content) <= 16 * 1024 * 1024, "preparation_source_file_size")
            result[relative] = content
            modes[relative] = stat.S_IFMT(mode) | stat.S_IMODE(mode)
    require(len(result) <= 100000, "preparation_source_file_count")
    return result, modes


def _preparation_scope(root, approved):
    document = _run_metadata(root / _safe_relative_manifest(os.environ["RELEASE_MANIFEST"]))
    packages = _workspace_packages(document, root)
    require(set(approved["packages"]) <= set(packages), "preparation_package_scope")
    workspace = _canonical_path(document["workspace_root"], "workspace_root")
    workspace_manifest = (workspace / "Cargo.toml").relative_to(root).as_posix()
    lock = (workspace / "Cargo.lock").relative_to(root).as_posix()
    paths = {workspace_manifest, lock}
    changelogs = {}
    manifests = {}
    for name, package in packages.items():
        manifest = Path(package["manifest_path"]).relative_to(root).as_posix()
        paths.add(manifest)
        manifests[name] = manifest
        if name in approved["packages"]:
            changelog = str(Path(manifest).parent / "CHANGELOG.md")
            paths.add(changelog)
            changelogs[name] = changelog
    return paths, changelogs, workspace_manifest, manifests, lock


def _preparation_config(approved):
    # Immutable generator-owned source record binds this environment literal.
    config = os.environ["RELEASE_PREPARE_CONFIG"]
    document = tomllib.loads(config)
    require(isinstance(document, dict) and set(document) == {"workspace", "package"},
            "preparation_config_shape")
    workspace = document["workspace"]
    require(workspace.get("release") is False and
            workspace.get("release_always") is False and
            workspace.get("semver_check") is True and
            workspace.get("publish_no_verify") is False and
            workspace.get("publish_allow_dirty") is False, "preparation_config_defaults")
    packages = document["package"]
    require(isinstance(packages, list) and len(packages) == len(approved["packages"]) and
            {item.get("name") for item in packages} == set(approved["packages"]),
            "preparation_config_scope")
    require(all(item.get("release") is True and item.get("publish") is True and
                item.get("git_only") is False for item in packages), "preparation_config_package")
    return config


def _preparation_identity(approved, root, environment):
    require(approved["tools"]["release-plz"] == "0.3.169", "preparation_tool_policy")
    version = _preparation_run(["release-plz", "--version"], root, environment).decode().strip()
    require(version == "release-plz 0.3.169", "preparation_tool_version")
    semver_pin = os.environ["RELEASE_SEMVER_CHECKS_VERSION"]
    require(VERSION.fullmatch(semver_pin), "preparation_semver_tool_policy")
    semver_version = _preparation_run(["cargo-semver-checks", "--version"],
                                      root, environment).decode().strip()
    require(semver_version == "cargo-semver-checks " + semver_pin,
            "preparation_semver_tool_version")
    head = _preparation_run(["git", "rev-parse", "HEAD"], root, environment).decode().strip()
    require(head == approved["source_sha"], "preparation_source_head")
    status = _preparation_run(["git", "status", "--porcelain=v1", "--untracked-files=all"],
                              root, environment)
    require(not status, "preparation_dirty_source")
    tree = _preparation_run(["git", "rev-parse", "HEAD^{tree}"], root, environment).decode().strip()
    require(re.fullmatch(r"[0-9a-f]{40}", tree), "preparation_source_tree")
    return tree


def _git_blob(content):
    return hashlib.sha1(b"blob " + str(len(content)).encode() + b"\x00" + content).hexdigest()


def _preparation_files(before, after, allowed, selected=None):
    require(set(before) <= set(after), "preparation_deleted_file")
    files = {}
    for path in sorted(set(before) | set(after)):
        old, new = before.get(path), after.get(path)
        if old == new:
            continue
        require(path in allowed and new is not None, "preparation_path_scope")
        preparation_bytes(old, new, path, selected)
        files[path] = {"before": _git_blob(old) if old is not None else None,
                       "after": base64.b64encode(new).decode("ascii"),
                       "sha256": hashlib.sha256(new).hexdigest()}
    require(sum(len(item["after"]) for item in files.values()) <= 12 * 1024 * 1024,
            "preparation_proposal_size")
    return files


def create_preparation_proposal():
    approved = policy()
    validate_source()
    root = Path(SOURCE_DIR).resolve(strict=True)
    environment = _clean_environment()
    environment["GIT_TERMINAL_PROMPT"] = "0"
    environment["GIT_CONFIG_COUNT"] = "0"
    tree = _preparation_identity(approved, root, environment)
    allowed, changelogs, workspace_manifest, workspace_manifests, lock = _preparation_scope(root, approved)
    before, before_modes = _preparation_snapshot(root)
    require(all(path not in before_modes or stat.S_ISREG(before_modes[path]) for path in allowed),
            "preparation_allowlist_mode")
    with tempfile.TemporaryDirectory(prefix="velnor-release-update-") as directory:
        config = Path(directory) / "release-plz.toml"
        config.write_text(_preparation_config(approved), encoding="utf-8")
        changelog_config = Path(directory) / "velnor-changelog.toml"
        changelog_config.write_text(PREPARATION_CHANGELOG_CONFIG, encoding="utf-8")
        environment["CARGO_HOME"] = str(Path(directory) / "cargo-home")
        environment["CARGO_TARGET_DIR"] = str(Path(directory) / "target")
        summary = _preparation_run(["release-plz", "update", "--config", str(config),
                                   "--changelog-config", str(changelog_config),
                                   "--manifest-path", str(root / os.environ["RELEASE_MANIFEST"]),
                                   "--repo-url", "https://github.com/" + approved["repository"],
                                   "--forge", "github"], root, environment)
    after, after_modes = _preparation_snapshot(root)
    require(all(after_modes.get(path) == mode for path, mode in before_modes.items()),
            "preparation_source_mode_changed")
    require(all(after.get(path) == before[path] for path, mode in before_modes.items()
                if stat.S_ISLNK(mode)), "preparation_source_symlink_changed")
    require(all(stat.S_ISREG(mode) for path, mode in after_modes.items() if path not in before_modes),
            "preparation_new_symlink")
    files = _preparation_files(before, after, allowed, set(approved["packages"]))
    document = _run_metadata(root / os.environ["RELEASE_MANIFEST"])
    actual = _workspace_packages(document, root)
    versions = {name: package["version"] for name, package in actual.items()}
    manifests = {path for path in allowed if path.endswith("Cargo.toml")}
    preparation_dependency_versions({path: before[path] for path in manifests},
                                    {path: after[path] for path in manifests}, versions)
    preparation_lock_versions(after[lock], versions)
    packages = {}
    for name in approved["packages"]:
        changelog = changelogs[name]
        notes = preparation_notes(after[changelog], actual[name]["version"]) if changelog in files else ""
        require(len(notes) <= 65536, "preparation_notes_size")
        packages[name] = {"version": actual[name]["version"], "notes": notes}
    outcomes = preparation_summary(summary, packages, approved["packages"])
    for name, package in packages.items():
        package.update(outcomes[name])
    actor = os.environ["GITHUB_ACTOR_ID"]
    require(re.fullmatch(r"[1-9][0-9]*", actor), "preparation_actor")
    base = os.environ["RELEASE_DEFAULT_BRANCH"]
    require(re.fullmatch(r"[A-Za-z0-9_.-]+(?:/[A-Za-z0-9_.-]+)*", base), "preparation_base")
    save_receipt({"schema": 1, "policy": approved, "source_sha": approved["source_sha"],
                  "source_tree": tree, "workflow_sha": os.environ["GITHUB_SHA"],
                  "run_id": os.environ["GITHUB_RUN_ID"],
                  "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"], "actor": actor,
                  "base": base, "files": files, "packages": packages,
                  "workspace_manifest": workspace_manifest, "manifests": workspace_manifests,
                  "status": "prepared"}, "release-proposal/evidence.json")


def preparation_anonymous_main():
    try:
        create_preparation_proposal()
    except (ReconcileError, ValidationError, OSError, ValueError, KeyError, TypeError,
            subprocess.SubprocessError) as error:
        raise SystemExit(f"release_prepare:{type(error).__name__}:{str(error)[:160]}") from error


if __name__ == "__main__":
    preparation_anonymous_main()
