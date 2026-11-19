#!/usr/bin/env python3

# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

"""Self-test for scripts/restructure-crates.py.

Builds a tiny fake git repository per scenario (old layout, a `restructure` branch that applies the
script's own move table, a feature branch that adds the pattern under test), merges the restructure
into the feature branch the way an open PR would, runs `branch-fixup`, and asserts the outcome.

Run: python3 scripts/test-restructure-crates.py
Set RESTRUCTURE_SCRIPT to test another copy of the script.
"""

import importlib.util
import os
import re
import subprocess
import sys
import tempfile
import textwrap
import unittest
from pathlib import Path

SCRIPT = Path(os.environ.get("RESTRUCTURE_SCRIPT") or Path(__file__).with_name("restructure-crates.py")).resolve()
spec = importlib.util.spec_from_file_location("restructure_crates", SCRIPT)
R = importlib.util.module_from_spec(spec)
spec.loader.exec_module(R)

GIT_ENV = {
    **os.environ,
    "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1",
    "GIT_AUTHOR_NAME": "t",
    "GIT_AUTHOR_EMAIL": "t@example.invalid",
    "GIT_COMMITTER_NAME": "t",
    "GIT_COMMITTER_EMAIL": "t@example.invalid",
}


def git(repo, *args, check=True):
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, env=GIT_ENV)
    if check and result.returncode != 0:
        raise AssertionError(f"git {' '.join(args)}: {result.stderr}{result.stdout}")
    return result


def put(repo, path, text):
    target = Path(repo) / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(text, encoding="utf-8")


def get(repo, path):
    return (Path(repo) / path).read_text(encoding="utf-8")


def unique(path):
    return "".join(f"// {path} line {n}\n" for n in range(8))


def commit(repo, message):
    git(repo, "add", "-A")
    git(repo, "commit", "-q", "-m", message)


def base_files():
    files = {}
    for root in R.OLD_ROOTS:
        files[f"{root}/Cargo.toml"] = f'[workspace]\nmembers = ["crates/*"]\n# {root}\n'
        files[f"{root}/Cargo.lock"] = f"# lock of {root}\nversion = 4\n"
        files[f"{root}/deny.toml"] = f"# deny of {root}\n[licenses]\nallow = [\"MIT\"]\n"
        files[f"{root}/.config/nextest.toml"] = f"# nextest of {root}\n[profile.default]\nretries = 0\n"
    for old, new, _ in R.MOVES:
        name = new.rsplit("/", 1)[1]
        if name == "velnor-test-support":
            files[f"{old}/src/lib.rs"] = unique(f"{old}/src/lib.rs")
            continue
        deps = ""
        if name == "velnor-ci":
            deps = 'velnor-model = { path = "../../../generator/crates/velnor-model" }\n'
        files[f"{old}/Cargo.toml"] = (
            f'[package]\nname = "{name}"\n\n[dependencies]\n{deps}\n[features]\ndefault = []\n'
            + "# padding\n" * 6
            + "\n[dev-dependencies]\n"
        )
        files[f"{old}/src/lib.rs"] = unique(f"{old}/src/lib.rs")
        files[f"{old}/tests/it/main.rs"] = unique(f"{old}/tests/it/main.rs")
    files["generator/samples/a.json"] = "{}\n"
    files["generator/samples/helper.rs"] = "// helper\n"
    files["generator/NOTES.md"] = "# notes\n"
    files["local/README.md"] = "# local\n"
    files["Cargo.toml"] = '[workspace]\nmembers = ["crates/other"]\n'
    files["Cargo.lock"] = "# root lock\nversion = 4\n"
    files["deny.toml"] = "# root deny\n"
    files[".config/nextest.toml"] = "# root nextest\n[profile.default]\nretries = 0\n"
    files["crates/other/src/lib.rs"] = unique("crates/other/src/lib.rs")
    return files


def restructure(repo):
    for path in git(repo, "ls-files").stdout.split():
        if path in R.OLD_ROOT_FILES:
            git(repo, "rm", "-q", "-f", path)
            continue
        new = R.map_path(path)
        if new != path:
            (Path(repo) / new).parent.mkdir(parents=True, exist_ok=True)
            git(repo, "mv", path, new)
            if new.endswith("/Cargo.toml"):
                text = get(repo, new)
                put(repo, new, R.rewrite_dep_paths(text, os.path.dirname(path), os.path.dirname(new)))
    put(repo, "Cargo.toml", '[workspace]\nmembers = ["crates/other", "crates/velnor-ci"]\n')
    put(repo, "Cargo.lock", "# root lock\nversion = 4\n# merged\n")
    put(repo, "deny.toml", "# root deny\n# merged\n")
    put(repo, ".config/nextest.toml", "# root nextest\n[profile.default]\nretries = 0\n# merged\n")
    commit(repo, "restructure")


class Scenario:
    def __init__(self, test, edit):
        self.dir = tempfile.TemporaryDirectory()
        test.addCleanup(self.dir.cleanup)
        self.repo = Path(self.dir.name) / "repo"
        self.repo.mkdir()
        git(self.repo, "init", "-q", "-b", "main")
        for path, text in base_files().items():
            put(self.repo, path, text)
        commit(self.repo, "base")
        git(self.repo, "checkout", "-q", "-b", "restructure")
        restructure(self.repo)
        git(self.repo, "checkout", "-q", "-b", "feature", "main")
        edit(self.repo)
        commit(self.repo, "feature")
        merged = git(
            self.repo, "-c", "merge.renameLimit=4000", "-c", "merge.directoryRenames=false", "merge", "-q", "--no-edit", "restructure", check=False
        )
        if merged.returncode != 0:
            commit(self.repo, "merge with the unresolved state committed")

    def fixup(self, *extra):
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "--root", str(self.repo), *extra],
            capture_output=True,
            text=True,
            env=GIT_ENV,
        )
        return result.returncode, result.stdout + result.stderr

    def branch_fixup(self):
        return self.fixup("branch-fixup", "--base", "restructure")

    def tracked(self):
        return set(git(self.repo, "ls-files").stdout.split())


class BranchFixup(unittest.TestCase):
    def test_stranded_files_are_moved_to_their_new_paths(self):
        stranded = {
            "crates/velnor-runner/crates/velnor-runner-host/src/scale_set/support/tests/refusal.rs": "crates/velnor-runner-host/src/scale_set/support/tests/refusal.rs",
            "local/crates/velnor-local/tests/it/supervisor/start.rs": "crates/velnor-local/tests/it/supervisor/start.rs",
            "crates/test_support/src/extra.rs": "crates/velnor-test-support/src/extra.rs",
            "ci-helper/crates/velnor-ci/src/io/new_dir/mod.rs": "crates/velnor-ci/src/io/new_dir/mod.rs",
        }
        stay = ("generator/samples/new.json", "generator/EXTRA.md", "local/docker/Dockerfile", "ci-helper/docs/new.md")

        def edit(repo):
            for old in (*stranded, *stay):
                put(repo, old, unique(old))

        scenario = Scenario(self, edit)
        before = scenario.tracked()
        for old in stranded:
            self.assertIn(old, before, "scenario must reproduce the stranded file")
        code, out = scenario.branch_fixup()
        self.assertEqual(code, 0, out)
        after = scenario.tracked()
        for old, new in stranded.items():
            self.assertNotIn(old, after, out)
            self.assertIn(new, after, out)
            self.assertEqual(get(scenario.repo, new), unique(old))
        for path in stay:
            self.assertIn(path, after, "stay-put paths must not move")

    def test_unmovable_files_under_old_roots_fail_and_are_listed(self):
        def edit(repo):
            put(repo, "local/stray.sh", "echo hi\n")
            put(repo, "ci-helper/crates/velnor-ci/src/clash.rs", "// branch\n")
            put(repo, "crates/velnor-ci/src/clash.rs", "// already there\n")

        scenario = Scenario(self, edit)
        code, out = scenario.branch_fixup()
        self.assertNotEqual(code, 0, out)
        self.assertIn("local/stray.sh", out)
        self.assertIn("ci-helper/crates/velnor-ci/src/clash.rs", out)
        self.assertEqual(get(scenario.repo, "crates/velnor-ci/src/clash.rs"), "// already there\n")

    def test_path_dependencies_resolve_from_the_new_location(self):
        def edit(repo):
            manifest = "ci-helper/crates/velnor-ci/Cargo.toml"
            put(repo, manifest, get(repo, manifest) + 'velnor-render = { path = "../../../generator/crates/velnor-render" }\n')
            put(
                repo,
                "generator/crates/velnor-gen/xtask/Cargo.toml",
                '[package]\nname = "xtask"\n\n[dependencies]\n'
                'velnor-local = { path = "../../../../local/crates/velnor-local" }\n'
                'velnor-model = { path = "../../velnor-model" }\n',
            )
            put(repo, "generator/crates/velnor-gen/xtask/src/main.rs", "fn main() {}\n")

        scenario = Scenario(self, edit)
        code, out = scenario.branch_fixup()
        self.assertEqual(code, 0, out)
        ci = get(scenario.repo, "crates/velnor-ci/Cargo.toml")
        self.assertIn('velnor-render = { path = "../velnor-render" }', ci)
        self.assertIn('velnor-model = { path = "../velnor-model" }', ci, "the line the restructure rewrote must stay as it is")
        xtask = get(scenario.repo, "crates/velnor-gen/xtask/Cargo.toml")
        self.assertIn('velnor-local = { path = "../../velnor-local" }', xtask)
        self.assertIn('velnor-model = { path = "../../velnor-model" }', xtask)

    def test_relative_paths_in_moved_and_added_rust_files(self):
        def edit(repo):
            put(
                repo,
                "generator/crates/velnor-gen/tests/it/fixtures_test.rs",
                'const A: &str = include_str!("../../../../samples/a.json");\n'
                '#[path = "../../../../samples/helper.rs"]\nmod helper;\n'
                'fn p() -> String { concat!(env!("CARGO_MANIFEST_DIR"), "/../../samples/a.json").to_string() }\n'
                'const SAME: &str = include_str!("sibling.json");\n',
            )
            put(repo, "generator/crates/velnor-gen/tests/it/sibling.json", "{}\n")
            lib = "generator/crates/velnor-gen/src/lib.rs"
            put(repo, lib, get(repo, lib) + 'const B: &[u8] = include_bytes!("../../../samples/a.json");\n')

        scenario = Scenario(self, edit)
        code, out = scenario.branch_fixup()
        self.assertEqual(code, 0, out)
        added = get(scenario.repo, "crates/velnor-gen/tests/it/fixtures_test.rs")
        self.assertIn('include_str!("../../../../generator/samples/a.json")', added)
        self.assertIn('#[path = "../../../../generator/samples/helper.rs"]', added)
        self.assertIn('"/../../generator/samples/a.json"', added)
        self.assertIn('include_str!("sibling.json")', added)
        lib = get(scenario.repo, "crates/velnor-gen/src/lib.rs")
        self.assertIn('include_bytes!("../../../generator/samples/a.json")', lib)
        code, out = scenario.branch_fixup()
        self.assertEqual(code, 0, out)
        self.assertEqual(get(scenario.repo, "crates/velnor-gen/tests/it/fixtures_test.rs"), added, "a second run must change nothing")

    def test_edits_to_deleted_old_root_files_are_reported_with_the_hunk(self):
        def edit(repo):
            nextest = "ci-helper/.config/nextest.toml"
            put(repo, nextest, get(repo, nextest) + '[[profile.default.overrides]]\nfilter = "test(crates_mirror)"\nslow-timeout = { period = "30s" }\n')
            put(repo, "ci-helper/Cargo.lock", get(repo, "ci-helper/Cargo.lock") + "# bumped by the branch\n")
            put(repo, "generator/deny.toml", get(repo, "generator/deny.toml") + "# branch exception\n")

        scenario = Scenario(self, edit)
        root_lock = get(scenario.repo, "Cargo.lock")
        code, out = scenario.branch_fixup()
        self.assertNotEqual(code, 0, out)
        self.assertIn("ci-helper/.config/nextest.toml", out)
        self.assertIn("+slow-timeout = { period = \"30s\" }", out, "the hunk must be shown")
        self.assertIn(".config/nextest.toml", out.split("ci-helper/.config/nextest.toml", 1)[1])
        self.assertIn("generator/deny.toml", out)
        self.assertIn("+# branch exception", out)
        self.assertIn("ci-helper/Cargo.lock", out)
        self.assertIn("cargo update -w", out)
        self.assertNotIn("+# bumped by the branch", out, "lockfile hunks are not shown for hand merging")
        self.assertEqual(get(scenario.repo, "Cargo.lock"), root_lock, "lockfiles are never auto-merged")

    def test_untouched_old_root_files_are_dropped_without_a_report(self):
        def edit(repo):
            put(repo, "crates/other/src/lib.rs", "// feature edit\n")

        scenario = Scenario(self, edit)
        code, out = scenario.branch_fixup()
        self.assertEqual(code, 0, out)
        for path in R.OLD_ROOT_FILES:
            self.assertNotIn(path, scenario.tracked())

    def test_status_counts_stranded_files(self):
        def edit(repo):
            put(repo, "local/crates/velnor-local/src/new.rs", "// new\n")

        scenario = Scenario(self, edit)
        code, out = scenario.fixup("status")
        self.assertIn("stranded files under old roots: 1", out)


class ApplyIsAPureMove(unittest.TestCase):
    def test_apply_has_no_source_level_lint_rewrite(self):
        self.assertFalse(hasattr(R, "fix_let_underscore"))
        source = "fn f() {\n    let _ = std::fs::remove_file(\"x\");\n}\n"
        path = "local/crates/velnor-local/src/lib.rs"
        files, dirs = R.tree_state([path])
        self.assertEqual(R.transform(path, source, files, dirs, "apply"), source)
        self.assertNotIn("scripts/restructure-crates.py", R.SOURCE_EDITS)
        grouped = "crates/velnor-local/tests/it/supervisor.rs"
        self.assertNotIn(grouped, R.SOURCE_EDITS, "no import regrouping: a restructure only moves")

    def test_root_manifest_allows_the_lint_at_workspace_level(self):
        lints = textwrap.dedent(
            """
            [workspace.lints.rust]
            unsafe_code = "forbid"

            [workspace.lints.clippy]
            let_underscore_future = "deny"
            let_underscore_must_use = "deny"

            [workspace.lints.rustdoc]
            broken_intra_doc_links = "deny"
            """
        )
        root = (
            '[workspace]\nmembers = ["crates/a"]\nresolver = "3"\n\n[workspace.dependencies]\nserde = "1"\n\n'
            '[workspace.package]\nversion = "0.1.80"\n' + lints
        )
        old = {name: f"[workspace]\nmembers = []\n\n[workspace.dependencies]\n{lints}" for name in R.OLD_ROOTS}
        text = R.build_root_manifest(root, old)
        parsed = R.tomllib.loads(text)
        clippy = parsed["workspace"]["lints"]["clippy"]
        self.assertEqual(clippy["let_underscore_must_use"], "allow")
        self.assertEqual(clippy["let_underscore_future"], "deny")
        self.assertIn("rewritten in a follow-up PR", text)


class ReleaseToolsProfile(unittest.TestCase):
    OLD = (
        '        run: cargo build --release --locked --target "$TARGET" --manifest-path ci-helper/Cargo.toml\n'
        '        run: cargo build --release --locked --target "$TARGET" --manifest-path generator/Cargo.toml --package velnor-gen\n'
        '        run: cp "ci-helper/target/$TARGET/release/velnor-ci" "dist/velnor-ci-$SUFFIX"\n'
        '        run: cp "generator/target/$TARGET/release/velnor-gen" "dist/velnor-gen-$SUFFIX"\n'
    )

    @staticmethod
    def rule_pattern():
        match = re.search(r"pattern: '([^\n]*)'\n", R.RELEASE_PROFILE_RULE)
        return re.compile(match.group(1))

    def rewrite(self, text):
        for old, new in R.RELEASE_TOOLS_EDITS:
            text = text.replace(old, new)
        return text

    def test_both_release_builds_select_the_profile_and_copy_from_its_directory(self):
        new = self.rewrite(self.OLD)
        self.assertEqual(new.count("--profile release-tools"), 2)
        self.assertNotIn("--release", new)
        self.assertIn('cp "target/$TARGET/release-tools/velnor-ci"', new)
        self.assertIn('cp "target/$TARGET/release-tools/velnor-gen"', new)
        self.assertIn("--package velnor-ci", new)
        self.assertEqual(self.rewrite(new), new)

    def test_the_alint_rule_rejects_a_release_build_of_either_tool_and_accepts_the_profile(self):
        pattern = self.rule_pattern()
        for line in self.OLD.splitlines():
            self.assertTrue(pattern.search(line), line)
        for line in (
            "cargo build --release --locked --package velnor-ci",
            "cargo build --locked --release -p velnor-gen",
            'cp "target/$TARGET/release/velnor-gen" dist/x',
        ):
            self.assertTrue(pattern.search(line), line)
        for line in self.rewrite(self.OLD).splitlines():
            self.assertFalse(pattern.search(line), line)
        self.assertFalse(pattern.search("cargo build --release --locked --package velnor-runner-cli"))

    def test_the_rule_is_added_to_the_alint_config(self):
        edits = R.POST_EDITS[".alint.yml"]
        self.assertTrue(any("ci-gen-release-profile" in new for _, new in edits))


class LockMergeKeepsTheHighestVersion(unittest.TestCase):
    @staticmethod
    def lock(*packages):
        body = "".join(
            f'[[package]]\nname = "{name}"\nversion = "{version}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n\n'
            for name, version in packages
        )
        return "version = 4\n\n" + body

    def test_union_picks_the_highest_version_of_each_compatible_range(self):
        merged = R.union_lock(
            [self.lock(("syn", "3.0.9"), ("syn", "2.0.119")), self.lock(("syn", "3.0.10")), self.lock(("syn", "3.0.9"))]
        )
        self.assertEqual(sorted(merged), [("syn", "2.0.119"), ("syn", "3.0.10")])

    def test_a_result_below_any_old_lock_is_reported(self):
        old = [self.lock(("syn", "3.0.9")), self.lock(("syn", "3.0.10"), ("log", "0.4.34"))]
        result = {("syn", "3.0.9"), ("log", "0.4.35")}
        self.assertEqual(R.lowered_versions(old, result), [("syn", "3.0.10", "3.0.9")])
        self.assertEqual(R.lowered_versions(old, {("syn", "3.0.10"), ("log", "0.4.35")}), [])

    def test_the_syn_exact_pin_follows_the_highest_lock(self):
        self.assertEqual(R.PINS["syn"], ("3.0.9", "3.0.10"))
        line = 'syn = { version = "=3.0.9", features = ["full", "visit"] }\n'
        self.assertEqual(R.apply_pins(line), 'syn = { version = "=3.0.10", features = ["full", "visit"] }\n')

    def test_velnor_local_keeps_the_base64_features_it_had(self):
        old_path = "local/crates/velnor-local/Cargo.toml"
        manifest = '[package]\nname = "velnor-local"\n\n[dependencies]\nbase64 = { version = "=0.22.1" }\nbytes = "1"\n'
        out = R.rewrite_manifest(old_path, manifest)
        dependency = R.tomllib.loads(out)["dependencies"]["base64"]
        self.assertEqual(dependency, {"version": "=0.23.1", "default-features": False, "features": ["std"]})
        with self.assertRaises(SystemExit):
            R.rewrite_manifest(old_path, manifest.replace("=0.22.1", "=0.22.2"))

    def test_report_lists_each_binary_and_flags_downgrades(self):
        labels = [label for label, _, _ in R.SHIPPED_BINARIES]
        self.assertEqual(len(labels), 7)
        before = {(label, target): {"syn": {"3.0.10"}} for label in labels for target in R.REPORT_TARGETS}
        after = {key: {"syn": {"3.0.9"}} for key in before}
        text = R.version_report(before, after)
        self.assertIn("| syn | 3.0.10 | 3.0.9 |", text)
        self.assertIn("velnor-ci: syn 3.0.10 -> 3.0.9", text)
        self.assertIn("Downgrades: none.", R.version_report(after, before))


if __name__ == "__main__":
    unittest.main()
