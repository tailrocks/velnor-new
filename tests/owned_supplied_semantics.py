#!/usr/bin/env python3
"""Independent supplied-context regressions; never invokes Cargo or a registry.

Inputs must be genuine Cargo metadata and matching inert rustdoc (same library
name/version). Each fixture changes only manifests and metadata feature policy.
"""
import argparse
import copy
from contextlib import nullcontext
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n")


def execute(binary, mode, path):
    result = subprocess.run([str(binary), mode, str(path)], text=True, capture_output=True)
    path.with_suffix(".stdout").write_text(result.stdout)
    path.with_suffix(".stderr").write_text(result.stderr)
    path.with_suffix(".exit").write_text(str(result.returncode) + "\n")
    return result


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def toml_value(value):
    if isinstance(value, dict):
        return "{ " + ", ".join(f'{json.dumps(k)} = {toml_value(v)}' for k, v in value.items()) + " }"
    if isinstance(value, bool):
        return str(value).lower()
    return json.dumps(value)


def fixture(directory, seed, package, features, policy=None, inherited=False, workspace_policy=None):
    directory.mkdir()
    member = directory / "member"
    member.mkdir()
    target = next(t for t in package["targets"] if "lib" in t["kind"])
    shutil.copytree(Path(target["src_path"]).parent, member / "src")
    name, version = package["name"], package["version"]
    manifest = member / "Cargo.toml"
    content = f'[package]\nname = {json.dumps(name)}\nversion = {json.dumps(version)}\nedition = "2024"\n'
    content += f'\n[lib]\nname = {json.dumps(target["name"])}\npath = "src/lib.rs"\n\n[features]\n'
    content += "".join(f'{json.dumps(k)} = {toml_value(v)}\n' for k, v in features.items())
    package_metadata = None if policy is None else {"cargo-semver-checks": {"lints": policy}}
    if policy is not None:
        content += '\n[package.metadata.cargo-semver-checks.lints]\n'
        content += "".join(f'{json.dumps(k)} = {toml_value(v)}\n' for k, v in policy.items())
    if inherited:
        content += "\n[lints]\nworkspace = true\n"
    manifest.write_text(content)
    root = directory / "Cargo.toml"
    root_content = '[workspace]\nmembers = ["member"]\nresolver = "3"\n\n[workspace.lints.rust]\nunsafe_code = "forbid"\n'
    workspace_metadata = None if workspace_policy is None else {"cargo-semver-checks": {"lints": workspace_policy}}
    if workspace_policy is not None:
        root_content += '\n[workspace.metadata.cargo-semver-checks.lints]\n'
        root_content += "".join(f'{json.dumps(k)} = {toml_value(v)}\n' for k, v in workspace_policy.items())
    root.write_text(root_content)
    data = copy.deepcopy(seed)
    selected = copy.deepcopy(package)
    selected["id"] = f"path+file://{member}#{name}@{version}"
    selected["manifest_path"] = str(manifest)
    selected["features"] = features
    selected["metadata"] = package_metadata
    selected["dependencies"] = []
    selected["targets"] = [copy.deepcopy(target)]
    selected["targets"][0]["src_path"] = str(member / "src/lib.rs")
    data.update(packages=[selected], workspace_members=[selected["id"]],
                workspace_default_members=[selected["id"]], workspace_root=str(directory),
                metadata=workspace_metadata, target_directory=str(directory / "target"), resolve=None)
    metadata = directory / "metadata.json"
    write_json(metadata, data)
    context = dict(metadata_path=str(metadata), package_id=selected["id"],
                   package_name=name, package_version=version, manifest_path=str(manifest))
    return context, root


def compare(binary, request_path, request, expected_code, lint=None, findings=None, level=None):
    write_json(request_path, request)
    result = execute(binary, "compare", request_path)
    assert result.returncode == expected_code, (request_path.name, result.returncode, result.stdout, result.stderr)
    if expected_code == 101:
        return
    report = json.loads(result.stdout)
    assert report["success"] == (expected_code == 0), report
    native_output = report["native_report_stdout"]
    assert "\x1b[" not in native_output, native_output
    if expected_code == 100:
        assert native_output.strip(), report
        if lint is not None:
            assert lint in native_output, native_output
    if lint is not None:
        rows = report["crates"][0]["lints"]
        row = next((r for r in rows if r["id"] == lint), None)
        assert row is not None, (lint, rows)
        assert row["findings"] == findings, row
        if level is not None:
            assert row["effective_lint_level"] == level, row
        assert row["effective_required_update"] == "Major", row
    return report


def plans(binary, root, seed, package):
    features = {"default": ["stable"], "stable": [], "no_std": [], "nightly": [],
                "bench": [], "unstable": [], "_hidden": [], "unstable-extra": [],
                "nightly-extra": []}
    current, _ = fixture(root / "plan-current", seed, package, features)
    baseline, _ = fixture(root / "plan-baseline", seed, package, features)
    index_path = root / "index.json"
    write_json(index_path, dict(name=package["name"], vers=package["version"], deps=[],
                               cksum="0" * 64, features=features, yanked=False))
    for group, expected, defaults in [
        ("heuristic", ["default", "nightly-extra", "stable"], True),
        ("all", sorted(features), True),
        ("default", [], True),
        ("none", [], False),
    ]:
        request = dict(schema_version=1, current=current, baseline=baseline,
                       baseline_index_version_path=str(index_path), feature_group=group,
                       target="aarch64-apple-darwin")
        path = root / ("plan-" + group + ".json")
        write_json(path, request)
        result = execute(binary, "plan", path)
        assert result.returncode == 0, (group, result.stdout, result.stderr)
        plan = json.loads(result.stdout)
        for side in ["current", "baseline"]:
            assert plan[side]["features"] == expected, plan
            assert plan[side]["use_default_features"] == defaults, plan
            assert plan[side]["target"] == request["target"], plan
            assert plan[side]["rustdoc_relative_directory"] == "aarch64-apple-darwin/doc", plan
        print("PASS", "plan_" + group)
    request.update(feature_group="none", extra_current_features=["stable", "stable", "new"],
                   extra_baseline_features=["stable", "stable", "new"])
    path = root / "plan-explicit-baseline-filter-dedup.json"
    write_json(path, request)
    result = execute(binary, "plan", path)
    assert result.returncode == 0, (result.stdout, result.stderr)
    plan = json.loads(result.stdout)
    assert plan["current"]["features"] == ["new", "stable"], plan
    assert plan["baseline"]["features"] == ["stable"], plan
    print("PASS", "plan_explicit_baseline_filter_dedup")
    dependency_plans(binary, root, seed, package)


def dependency_plans(binary, root, seed, package):
    for suppressed in [False, True]:
        features = {"stable": [], "default": []}
        if suppressed:
            features["public"] = ["dep:optional"]
        suffix = "suppressed" if suppressed else "implicit"
        current, _ = fixture(root / ("deps-current-" + suffix), seed, package, features)
        baseline, _ = fixture(root / ("deps-baseline-" + suffix), seed, package, features)
        dependencies = [dict(name=name, req="^1", features=[], optional=True,
                             default_features=True, target=target, kind="normal")
                        for name, target in [("optional", None), ("target_optional", "cfg(windows)")]]
        for context in [current, baseline]:
            manifest = Path(context["manifest_path"])
            with manifest.open("a") as stream:
                stream.write('\n[dependencies]\noptional = { version = "1", optional = true }\n'
                             '\n[target.\'cfg(windows)\'.dependencies]\n'
                             'target_optional = { version = "1", optional = true }\n')
            metadata_path = Path(context["metadata_path"])
            metadata = json.loads(metadata_path.read_text())
            metadata["packages"][0]["dependencies"] = [
                dict(name=dep["name"], source="registry+https://github.com/rust-lang/crates.io-index",
                     req=dep["req"], kind=None, rename=None, optional=True,
                     uses_default_features=True, features=[], target=dep["target"], registry=None)
                for dep in dependencies]
            write_json(metadata_path, metadata)
        index_path = root / ("index-deps-" + suffix + ".json")
        write_json(index_path, dict(name=package["name"], vers=package["version"],
                                   deps=dependencies, cksum="0" * 64, features=features, yanked=False))
        expected = sorted(set(features) | {"target_optional"} | (set() if suppressed else {"optional"}))
        for group in ["all", "heuristic"]:
            request = dict(schema_version=1, current=current, baseline=baseline,
                           baseline_index_version_path=str(index_path), feature_group=group,
                           target="aarch64-apple-darwin")
            path = root / ("plan-deps-" + suffix + "-" + group + ".json")
            write_json(path, request)
            result = execute(binary, "plan", path)
            assert result.returncode == 0, (result.stdout, result.stderr)
            plan = json.loads(result.stdout)
            for side in ["current", "baseline"]:
                assert plan[side]["features"] == expected, plan
                assert plan[side]["use_default_features"] is True, plan
            print("PASS", path.stem)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    parser.add_argument("--rustdoc", type=Path, required=True)
    parser.add_argument("--require-manifest-feature-binding", action="store_true",
                        help="probe provenance enforcement; trusted metadata APIs may delegate this to their producer")
    parser.add_argument("--skip-plan", action="store_true", help="run supplied-doc comparisons only")
    parser.add_argument("--output-root", type=Path, help="persist every fixture and raw native receipt")
    args = parser.parse_args()
    seed = json.loads(args.metadata.read_text())
    package = next(p for p in seed["packages"] if any("lib" in t["kind"] for t in p["targets"]))
    assert not package["dependencies"], "seed must have no dependencies"
    docs = args.rustdoc.resolve()
    binary = args.binary.resolve()
    cases = [
        ("removed", {"stable": []}, {}, "feature_missing", 1, 100),
        ("implication_removed", {"a": ["c"], "b": [], "c": []}, {"a": ["b"], "b": [], "c": []}, "feature_no_longer_enables_feature", 1, 100),
        ("implication_rerouted", {"a": ["c"], "b": ["c"], "c": []}, {"a": ["b"], "b": ["c"], "c": []}, "feature_no_longer_enables_feature", 0, 0),
        ("default_transitive_removed", {"default": ["a"], "a": ["b"], "b": []}, {"default": ["a"], "a": [], "b": []}, "feature_not_enabled_by_default", 1, 100),
        ("private_removed", {"_private": [], "nightly-extra": []}, {}, "feature_missing", 0, 0),
    ]
    if args.output_root:
        args.output_root.mkdir(parents=True, exist_ok=False)
    context = nullcontext(str(args.output_root.resolve())) if args.output_root else tempfile.TemporaryDirectory(prefix="owned-semver-regressions-")
    with context as temp:
        root = Path(temp)
        sources = [args.metadata.resolve(), docs, binary,
                   Path(next(t for t in package["targets"] if "lib" in t["kind"])["src_path"])]
        before = {str(path): digest(path) for path in sources}
        write_json(root / "scope.json", dict(
            scope="Synthetic semantic metadata fixtures derived from genuine inert API rustdoc; real metadata provenance remains producer responsibility.",
            source_sha256_before=before))
        if not args.skip_plan:
            plans(binary, root, seed, package)
        for name, old, new, lint, count, code in cases:
            baseline, _ = fixture(root / (name + "-baseline"), seed, package, old)
            current, workspace = fixture(root / (name + "-current"), seed, package, new)
            request = dict(schema_version=1, current=dict(package=current, rustdoc_path=str(docs)),
                           baseline=dict(package=baseline, rustdoc_path=str(docs)),
                           current_workspace_manifest_path=str(workspace), release_type="minor")
            compare(binary, root / (name + ".json"), request, code, lint, count, "Deny")
            print("PASS", name)
        baseline, _ = fixture(root / "policy-baseline", seed, package, {"stable": []})
        for name, inherited, package_policy, workspace_policy, code, level in [
            ("lints_inheritance", True, None, {"feature_missing": "warn"}, 0, "Warn"),
            ("metadata_inheritance", False, {"workspace": True}, {"feature_missing": "warn"}, 0, "Warn"),
            ("package_precedence", True, {"feature_missing": "deny"}, {"feature_missing": "warn"}, 100, "Deny"),
            ("required_update_inheritance", True, None, {"feature_missing": {"level": "deny", "required-update": "minor"}}, 0, None),
        ]:
            current, workspace = fixture(root / name, seed, package, {}, package_policy, inherited, workspace_policy)
            request = dict(schema_version=1, current=dict(package=current, rustdoc_path=str(docs)),
                           baseline=dict(package=baseline, rustdoc_path=str(docs)),
                           current_workspace_manifest_path=str(workspace), release_type="minor")
            report = compare(binary, root / (name + ".json"), request, code,
                             "feature_missing" if level else None, 1, level)
            if level is None:
                assert all(row["id"] != "feature_missing" for row in report["crates"][0]["lints"]), report
            print("PASS", name)
        current, workspace = fixture(root / "forged", seed, package, {})
        request = dict(schema_version=1, current=dict(package=current, rustdoc_path=str(docs)),
                       baseline=dict(package=baseline, rustdoc_path=str(docs)),
                       current_workspace_manifest_path=str(workspace), release_type="minor")
        for field, forged in [("package_id", "absent"), ("package_name", "other"), ("package_version", "99.0.0"), ("manifest_path", str(root / "absent.toml"))]:
            invalid = copy.deepcopy(request)
            invalid["current"]["package"][field] = forged
            compare(binary, root / (field + ".json"), invalid, 101)
            print("PASS", "forged_" + field)
        if args.require_manifest_feature_binding:
            metadata_path = Path(current["metadata_path"])
            metadata = json.loads(metadata_path.read_text())
            metadata["packages"][0]["features"] = {"stable": []}
            write_json(metadata_path, metadata)
            compare(binary, root / "forged_feature_graph.json", request, 101)
            print("PASS", "forged_feature_graph")
        after = {str(path): digest(path) for path in sources}
        write_json(root / "source-hashes-after.json", after)
        assert before == after, (before, after)


if __name__ == "__main__":
    main()
