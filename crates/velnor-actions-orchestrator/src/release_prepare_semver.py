"""Native supplied-document producer; data observations confer no authority.

The embedding private SDK constructor owns executable qualification, approved
environment and exact native source selection. Injected executors support local
tests; their responses never become a qualified capability in this module.
"""
import hashlib
import json
import os
from pathlib import Path
import stat
import tempfile
from types import MappingProxyType


def _semver_walk_error(error):
    raise ReconcileError("semver_baseline_walk") from error


def _semver_registry_tree(root):
    root = Path(root).resolve(strict=True)
    require(root.is_dir(), "semver_baseline_root")
    inventory, layout = {}, set()
    for directory, directories, files in os.walk(root, followlinks=False,
                                                onerror=_semver_walk_error):
        directory = Path(directory)
        for path in [*(directory / item for item in directories),
                     *(directory / item for item in files)]:
            mode = path.lstat().st_mode
            require(stat.S_ISDIR(mode) or stat.S_ISREG(mode), "semver_baseline_type")
            if stat.S_ISDIR(mode):
                layout.add(path.relative_to(root).as_posix())
            else:
                require(path.stat().st_size <= REGISTRY_FILE_LIMIT,
                        "semver_baseline_file_size")
                inventory[path.relative_to(root).as_posix()] = {
                    "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
                    "mode": "100755" if mode & 0o111 else "100644"}
        require(len(inventory) + len(layout) <= REGISTRY_MEMBER_LIMIT,
                "semver_baseline_file_count")
    return inventory, frozenset(layout)


def _semver_registry_inventory(root):
    return _semver_registry_tree(root)[0]


def _semver_bind_registry(root, authenticated, acquired_layout=None):
    """Compare actual baseline bytes/modes with separately acquired registry bytes."""
    observed, actual_layout = _semver_registry_tree(root)
    expected, registry_layout = _semver_registry_tree(authenticated["package_root"])
    require(acquired_layout is None or registry_layout == acquired_layout,
            "semver_registry_layout_changed")
    require(actual_layout == registry_layout, "semver_baseline_registry_layout")
    require(same_json(observed, expected), "semver_baseline_registry_source")
    encoded = json.dumps(expected, sort_keys=True, separators=(",", ":")).encode()
    require(hashlib.sha256(encoded).hexdigest() == authenticated["inventory_sha256"],
            "semver_registry_inventory_changed")
    index = Path(authenticated["index_version_path"])
    require(index.is_file() and not index.is_symlink() and
            hashlib.sha256(index.read_bytes()).hexdigest() ==
            authenticated["index_version_sha256"], "semver_registry_index_changed")
    return registry_layout


def _semver_write_json(path, value):
    raw = json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()
    with path.open("xb") as stream:
        stream.write(raw)
    return raw


def _semver_package_context(observed, name, version, destination):
    raw = observed["metadata_bytes"]
    require(type(raw) is bytes and len(raw) <= 32 * 1024 * 1024,
            "semver_metadata_size")
    with destination.open("xb") as stream:
        stream.write(raw)
    return {"metadata_path": str(destination), "package_id": observed["package_id"],
            "package_name": name, "package_version": version,
            "manifest_path": observed["manifest_path"]}


def _semver_checker(checker, mode, request, directory, cwd, run):
    require(isinstance(checker, str) and Path(checker).is_absolute(),
            "semver_checker_absolute")
    request_path = directory / (mode + "-request.json")
    request_raw = _semver_write_json(request_path, request)
    try:
        result = run([checker, mode, str(request_path)], cwd, {})
    finally:
        require(request_path.is_file() and not request_path.is_symlink() and
                request_path.read_bytes() == request_raw, "semver_checker_request_changed")
    require(type(result) is tuple and len(result) == 2 and
            type(result[0]) is int and type(result[1]) is bytes and
            len(result[1]) <= 32 * 1024 * 1024, "semver_checker_result")
    code, raw = result
    require(code in ({0} if mode == "plan" else {0, 100}), "semver_checker_failed")
    try:
        value = decode_json(raw)
    except (UnicodeError, json.JSONDecodeError) as error:
        raise ReconcileError("semver_checker_json") from error
    require(type(value) is dict and type(value.get("schema_version")) is int and
            value["schema_version"] == 1, "semver_checker_schema")
    return code, raw, value


def _semver_compare_result(code, value, name):
    require(set(value) == {"schema_version", "success", "crates", "native_report_stdout"} and
            type(value["success"]) is bool and value["success"] == (code == 0),
            "semver_checker_status")
    require(type(value["native_report_stdout"]) is str and
            (code == 0 or value["native_report_stdout"].strip()),
            "semver_checker_native_report")
    crates = value["crates"]
    require(type(crates) is list and len(crates) == 1 and type(crates[0]) is dict,
            "semver_checker_crates")
    crate = crates[0]
    require(set(crate) == {"name", "success", "detected_bump", "required_bump",
            "selected_checks", "skipped_checks", "lints"} and crate["name"] == name and
            type(crate["success"]) is bool and crate["success"] == value["success"],
            "semver_checker_crate_identity")
    require(type(crate["detected_bump"]) is str and
            crate["detected_bump"] in {"Major", "Minor", "Patch", "NotChanged"} and
            (crate["required_bump"] is None or type(crate["required_bump"]) is str and
             crate["required_bump"] in {"Major", "Minor", "Patch"}),
            "semver_checker_bumps")
    require(crate["detected_bump"] == "Minor" and crate["required_bump"] ==
            (None if crate["success"] else "Major"), "semver_checker_minor_result")
    require(all(type(crate[key]) is int and 0 <= crate[key] <= 2**64 - 1
                for key in ("selected_checks", "skipped_checks")), "semver_checker_counts")
    require(type(crate["lints"]) is list, "semver_checker_lints")
    require(crate["selected_checks"] == len(crate["lints"]), "semver_checker_lint_count")
    identifiers = set()
    for lint in crate["lints"]:
        require(type(lint) is dict and set(lint) == {"id", "effective_required_update",
                "effective_lint_level", "findings"} and type(lint["id"]) is str and
                lint["id"] and lint["id"] not in identifiers and
                type(lint["effective_required_update"]) is str and
                lint["effective_required_update"] in {"Major", "Minor"} and
                type(lint["effective_lint_level"]) is str and
                lint["effective_lint_level"] in {"Allow", "Warn", "Deny"} and
                type(lint["findings"]) is int and 0 <= lint["findings"] <= 2**64 - 1,
                "semver_checker_lint")
        identifiers.add(lint["id"])


def _semver_plan_result(value, request):
    require(set(value) == {"schema_version", "current", "baseline",
            "baseline_index_version_path", "target", "build_environment"} and
            value["baseline_index_version_path"] == request["baseline_index_version_path"] and
            same_json(value["target"], request["target"]), "semver_plan_identity")
    build = value["build_environment"]
    require(type(build) is dict and set(build) == {"target_triple", "cargo_rustflags",
            "cargo_rustdocflags", "toolchain_version"} and
            all(type(item) is str and "\x00" not in item for item in build.values()),
            "semver_plan_build_environment")
    for side in ("current", "baseline"):
        selected = value[side]
        require(type(selected) is dict and set(selected) == {"package", "features",
                "use_default_features", "target", "rustdoc_relative_directory"} and
                same_json(selected["package"], request[side]) and
                same_json(selected["target"], request["target"]), "semver_plan_package")
        require(type(selected["features"]) is list and
                all(type(feature) is str for feature in selected["features"]) and
                selected["features"] == sorted(set(selected["features"])) and
                type(selected["use_default_features"]) is bool,
                "semver_plan_features")
        relative = selected["rustdoc_relative_directory"]
        require(type(relative) is str and relative and not Path(relative).is_absolute() and
                all(part not in (".", "..") for part in Path(relative).parts),
                "semver_plan_rustdoc_directory")


def _semver_generate(side, initial, root, name, version, context, plan,
                     governing_lock_context, run, cwd):
    selected = plan[side]
    recipe = dict(context, features=selected["features"],
                  use_default_features=selected["use_default_features"],
                  target=selected["target"], native_build_environment=plan["build_environment"],
                  rustdoc_relative_directory=selected["rustdoc_relative_directory"],
                  generation_cwd=str(cwd))
    generated = generate_locked_docs(root, initial["manifest_path"], name, version, recipe,
                                     governing_lock_context, run)
    require(generated["package_id"] == initial["package_id"] and
            same_json(generated["package_metadata"], initial["package_metadata"]) and
            generated["input_snapshots"] == initial["input_snapshots"] and
            generated["source_snapshot"] == initial["source_snapshot"],
            "semver_generated_source_changed")
    return generated


def _semver_inputs_unchanged(roots, initial, inputs):
    for side in ("current", "baseline"):
        require(_docs_inventory(roots[side]) == initial[side]["source_snapshot"],
                "semver_source_changed")
    for path, raw in inputs.items():
        path = Path(path)
        require(path.is_file() and not path.is_symlink() and path.read_bytes() == raw,
                "semver_supplied_input_changed")


def _semver_produce(roots, directory, name, versions, manifests, output, context,
                     checker, governing_lock_contexts, run, check_run, authenticated,
                     acquired_layout, initial, inputs):
    packages = {}
    for side in ("current", "baseline"):
        observed = read_locked_package(roots[side], manifests[side], name, versions[side],
            context["cargo_executable"], str(output), context["max_jobs"],
            governing_lock_contexts[side], run)
        require(observed["source_snapshot"] == initial[side]["source_snapshot"],
                "semver_metadata_source_changed")
        initial[side] = observed
        packages[side] = _semver_package_context(initial[side], name, versions[side],
                                                directory / (side + "-plan-metadata.json"))
        inputs.update(initial[side]["input_snapshots"])
        inputs[packages[side]["metadata_path"]] = initial[side]["metadata_bytes"]
    inputs[authenticated["index_version_path"]] = Path(
        authenticated["index_version_path"]).read_bytes()
    request = {"schema_version": 1, **packages, "baseline_index_version_path":
               authenticated["index_version_path"], "feature_group": "heuristic",
               "extra_current_features": [], "extra_baseline_features": [],
               "target": context["target"]}
    _, _, plan = _semver_checker(checker, "plan", request, directory, roots["current"], check_run)
    _semver_plan_result(plan, request)
    _semver_inputs_unchanged(roots, initial, inputs)
    docs = {}
    for side in ("current", "baseline"):
        docs[side] = _semver_generate(side, initial[side], roots[side], name, versions[side],
                                      context, plan, governing_lock_contexts[side], run,
                                      roots["current"])
        _semver_inputs_unchanged(roots, initial, inputs)
    require(docs["current"]["format_version"] == docs["baseline"]["format_version"],
            "semver_rustdoc_format")
    compared = {}
    for side in ("current", "baseline"):
        package = _semver_package_context(docs[side], name, versions[side],
                                          directory / (side + "-compare-metadata.json"))
        inputs[package["metadata_path"]] = docs[side]["metadata_bytes"]
        inputs[docs[side]["rustdoc_path"]] = docs[side]["rustdoc_bytes"]
        compared[side] = {"package": package, "rustdoc_path": docs[side]["rustdoc_path"]}
    request = {"schema_version": 1, **compared, "current_workspace_manifest_path":
               initial["current"]["workspace_manifest_path"], "release_type": "minor"}
    _semver_inputs_unchanged(roots, initial, inputs)
    code, _, report = _semver_checker(checker, "compare", request, directory,
                                       roots["current"], check_run)
    _semver_compare_result(code, report, name)
    _semver_inputs_unchanged(roots, initial, inputs)
    _semver_bind_registry(roots["baseline"], authenticated, acquired_layout)
    return {"status": "compatible" if code == 0 else "incompatible",
            "report": report, "report_stdout": report["native_report_stdout"].strip(),
            "plan": plan, "current": docs["current"], "baseline": docs["baseline"],
            "registry": authenticated}


def _semver_governing_contexts(observations):
    """Freeze native observations before callbacks; observations grant no authority."""
    require(type(observations) is dict and set(observations) == {"current", "baseline"},
            "semver_governing_lock_contexts")
    return MappingProxyType({side: _docs_freeze_lock_observation(observations[side])[0]
                             for side in ("current", "baseline")})


def prepare_locked_semver(current_root, current_manifest, name, current_version,
                          baseline_version, baseline_root, output_root, context,
                          checker, governing_lock_contexts, run, check_run, fetch=None):
    """Observe native plan, docs and report; private constructor admits the result."""
    governing_lock_contexts = _semver_governing_contexts(governing_lock_contexts)
    roots = {"current": Path(current_root).resolve(strict=True),
             "baseline": Path(baseline_root).resolve(strict=True)}
    output = Path(output_root).resolve(strict=True)
    require(output.is_dir() and all(output != root and root not in output.parents
            for root in roots.values()), "semver_output_scope")
    directory = Path(tempfile.mkdtemp(prefix="velnor-semver-", dir=output))
    authenticated = acquire_registry_baseline(name, baseline_version,
                                              directory / "registry", fetch)
    acquired_layout = _semver_bind_registry(roots["baseline"], authenticated)
    versions = {"current": current_version, "baseline": baseline_version}
    manifests = {"current": current_manifest, "baseline": roots["baseline"] / "Cargo.toml"}
    initial = {side: {"source_snapshot": _docs_inventory(root)}
               for side, root in roots.items()}
    inputs = {}
    try:
        return _semver_produce(roots, directory, name, versions, manifests, output, context,
                               checker, governing_lock_contexts, run, check_run, authenticated,
                               acquired_layout, initial, inputs)
    finally:
        _semver_inputs_unchanged(roots, initial, inputs)
        _semver_bind_registry(roots["baseline"], authenticated, acquired_layout)
