#!/usr/bin/env python3
"""Fixed stages for generator-owned native desktop workflows."""
import argparse
import importlib.abc
import importlib.machinery
import importlib.util
import json
import os
from pathlib import Path
import re
import signal
import sys
import subprocess
import time
import xml.etree.ElementTree as ET

MANAGED_MODULES = {'desktop_native_core', 'desktop_native_build', 'desktop_native_verify',
                   'desktop_native_sign', 'desktop_native_state'}
MANAGED_ROOT = Path(__file__).resolve().parent


class SourceOnlyLoader(importlib.machinery.SourceFileLoader):
    def get_code(self, fullname):
        source = Path(self.path)
        if source.is_symlink() or not source.is_file():
            raise ValueError('managed native helper must be a regular source file')
        return self.source_to_code(source.read_bytes(), str(source))


class NativeSourceFinder(importlib.abc.MetaPathFinder):
    def find_spec(self, fullname, path=None, target=None):
        if fullname not in MANAGED_MODULES:
            return None
        source = MANAGED_ROOT / (fullname + '.py')
        return importlib.util.spec_from_file_location(fullname, source,
            loader=SourceOnlyLoader(fullname, str(source)))


sys.meta_path.insert(0, NativeSourceFinder())
from desktop_native_core import load_profile_source, path, run, safe_path, validate_source_sha, version_build
from desktop_native_build import assemble_framework, bindings_check, build_app, files
from desktop_native_build import generate_project, xcode_arguments
from desktop_native_verify import verify_app

STAGES = ('generate-project', 'bindings-check', 'xcframework', 'build', 'verify', 'swift-build', 'swift-test', 'xcode-build', 'xcode-test',
          'format', 'lint', 'ui-test', 'deadcode', 'swift-harnesses', 'sign', 'state')


def swift_testing_totals(report):
    root = ET.parse(report).getroot()
    suites = list(root.iter('testsuite'))
    if not suites:
        raise ValueError('missing Swift Testing suites')
    totals = [0, 0, 0]
    for suite in suites:
        for index, key in enumerate(['tests', 'failures', 'errors']):
            raw = suite.attrib.get(key, '')
            if not raw.isascii() or not raw.isdecimal():
                raise ValueError('corrupt Swift Testing suite totals')
            totals[index] += int(raw)
    if totals[0] <= 0 or totals[1:] != [0, 0]:
        raise ValueError('Swift Testing must execute nonzero tests without failures')


def xctest_report_totals(report):
    root = ET.parse(report).getroot()
    suites = list(root.iter('testsuite'))
    if not suites:
        raise ValueError('missing XCTest xUnit suites')
    executed = 0
    for suite in suites:
        if list(suite.findall('testsuite')):
            raise ValueError('nested XCTest suites require an explicit report contract')
        cases = list(suite.findall('testcase'))
        totals = []
        for key in ['tests', 'failures', 'errors']:
            raw = suite.attrib.get(key, '')
            if not raw.isascii() or not raw.isdecimal():
                raise ValueError('corrupt XCTest xUnit totals')
            totals.append(int(raw))
        failures = sum(case.find('failure') is not None for case in cases)
        errors = sum(case.find('error') is not None for case in cases)
        if totals != [len(cases), failures, errors] or failures or errors:
            raise ValueError('XCTest metadata differs from executed passing cases')
        for case in cases:
            if not case.get('name') or not case.get('classname'):
                raise ValueError('corrupt XCTest testcase identity')
            executed += case.find('skipped') is None
    if executed <= 0:
        raise ValueError('XCTest xUnit executed zero tests')
    return executed


def swift_test(profile):
    native = path(profile, 'native_root')
    frameworks = (profile.get('checks') or {}).get('swift_test_frameworks')
    if not frameworks:
        raise ValueError('Swift tests require typed framework expectations')
    report = safe_path(native, '.build/velnor-swift-tests.xml')
    swift_report = report.with_name('velnor-swift-tests-swift-testing.xml')
    log = report.with_suffix('.log')
    for item in [report, swift_report, log]:
        safe_path(native, str(item.relative_to(native)))
        if item.exists():
            item.unlink()
    report.parent.mkdir(parents=True, exist_ok=True)
    try:
        output = run(['swift', 'test', '-c', 'release', '--parallel', '--xunit-output', report], cwd=native, stream=True)
    except subprocess.CalledProcessError as error:
        log.write_text(error.output or '')
        raise
    log.write_text(output)
    if 'xctest' in frameworks:
        xctest_report_totals(report)
    if 'swift-testing' in frameworks:
        swift_testing_totals(swift_report)


def format_check(profile):
    native = path(profile, 'native_root')
    generated = path(profile, 'ffi.bindings_path')
    checks = profile.get('checks')
    if not checks or not checks['source_dirs']:
        raise ValueError('format check requires typed source directories')
    sources = []
    for directory in checks['source_dirs']:
        source = safe_path(native, directory)
        files(source)
        sources += [item for item in sorted(source.rglob('*.swift'))
                    if not item.is_relative_to(generated)]
    if not sources:
        raise ValueError('no handwritten Swift sources')
    argv = ['xcrun', 'swift-format', 'lint', '--strict', '--parallel']
    if checks.get('format_config') is not None:
        argv += ['--configuration', safe_path(native, checks['format_config'])]
    run(argv + sources, cwd=native)


def deadcode(profile):
    native = path(profile, 'native_root')
    generated = path(profile, 'ffi.bindings_path')
    if not generated.is_relative_to(native):
        raise ValueError('bindings must be within native root for deadcode exclusion')
    run(['periphery', 'scan', '--project', path(profile, 'apple.project_path'),
         '--schemes', profile['apple']['scheme'], '--retain-public',
         '--retain-objc-accessible', '--retain-assign-only-properties',
         '--report-exclude', str(generated / '**'), '--strict', '--quiet'], cwd=native)


def result_summary(result, expected):
    if not (result / 'Info.plist').is_file():
        raise ValueError('missing or corrupt Xcode result bundle')
    output = run(['xcrun', 'xcresulttool', 'get', 'test-results', 'summary', '--path', result])
    summary = json.loads(output)
    keys = ['totalTestCount', 'failedTests', 'passedTests']
    if any(type(summary.get(key)) is not int for key in keys):
        raise ValueError('corrupt Xcode test totals')
    if (summary['totalTestCount'] != expected or summary['failedTests'] != 0 or
            summary['passedTests'] != expected or expected <= 0):
        raise ValueError('Xcode test counts differ from expected passing tests')
    tree = json.loads(run(['xcrun', 'xcresulttool', 'get', 'test-results', 'tests', '--path', result]))
    if has_runtime_warning(tree):
        raise ValueError('Xcode tests emitted runtime warnings')


def has_runtime_warning(node):
    if isinstance(node, dict):
        return node.get('nodeType') == 'Runtime Warning' or any(has_runtime_warning(v) for v in node.values())
    if isinstance(node, list):
        return any(has_runtime_warning(value) for value in node)
    return False


def selected_tests(profile, directory):
    field = 'ui_test_sources' if directory == 'UITests' else 'test_sources'
    sources = profile['apple'].get(field)
    if not sources:
        raise ValueError('native test stage requires explicit typed ' + field)
    inventory = {}
    for source in sources:
        tree = safe_path(path(profile, 'native_root'), source)
        inventory.update({source + '/' + key: value for key, value in files(tree).items()})
    selections = []
    field = 'ui_test_target' if directory == 'UITests' else 'test_target'
    target = profile['apple'].get(field)
    if not isinstance(target, str) or re.fullmatch(r'[A-Za-z0-9_][A-Za-z0-9_.-]*', target) is None:
        raise ValueError('native test stage requires an explicit typed ' + field)
    for name, data in inventory.items():
        if not name.endswith('.swift'):
            continue
        text = data.decode()
        classes = list(re.finditer(r'\bclass\s+(\w+)\s*:\s*XCTestCase\b', text))
        for index, match in enumerate(classes):
            end = classes[index + 1].start() if index + 1 < len(classes) else len(text)
            tests = re.findall(r'^\s*(?:@MainActor\s+)?func\s+(test\w+)\s*\(', text[match.end():end], re.MULTILINE)
            selections += [target + '/' + match[1] + '/' + test for test in tests]
    if not selections or len(set(selections)) != len(selections):
        raise ValueError('missing or duplicate native XCTest selections')
    return sorted(selections)


def stop_repo_apps(profile):
    app = str(path(profile, 'apple.app_path')) + '/Contents/MacOS/'
    derived = str(path(profile, 'apple.derived_data_path')) + '/'
    name = profile['apple']['app_name']
    output = run(['ps', '-axo', 'pid=,command='])
    runner = profile['apple'].get('ui_test_target')
    executables = re.escape(name) + ('|' + re.escape(runner + '-Runner') if runner else '')
    pattern = re.compile(r'^(?:' + re.escape(app) + re.escape(name) +
                         r'|' + re.escape(derived) + r'.*\.app/Contents/MacOS/(?:' +
                         executables + r'))(?: |$)')
    targets = []
    for line in output.splitlines():
        fields = line.strip().split(None, 1)
        if len(fields) == 2 and fields[0].isdigit() and pattern.search(fields[1]):
            targets.append(int(fields[0]))
    for pid in targets:
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            continue
    for _ in range(20):
        live = []
        for pid in targets:
            try:
                os.kill(pid, 0)
                live.append(pid)
            except ProcessLookupError:
                continue
        targets = live
        if not targets:
            return
        time.sleep(0.25)
    for pid in targets:
        try:
            os.kill(pid, signal.SIGKILL)
        except ProcessLookupError:
            continue


def xcode_test(profile, ui=False):
    import tempfile
    native = path(profile, 'native_root')
    selections = selected_tests(profile, 'UITests' if ui else 'Tests')
    root = safe_path(native, '.build/velnor-test-results')
    root.mkdir(parents=True, exist_ok=True)
    lock = safe_path(native, '.build/velnor-ui-test.lock') if ui else None
    if lock is not None:
        lock.mkdir()
    try:
        generate_project(profile)
        temporary = tempfile.mkdtemp(prefix='ui-' if ui else 'unit-', dir=root)
        for index, selection in enumerate(selections):
            if ui:
                stop_repo_apps(profile)
            result = Path(temporary) / (str(index) + '.xcresult')
            output = run(xcode_arguments(profile, 'Debug') + ['test',
                '-parallel-testing-enabled', 'NO', '-only-testing:' + selection,
                '-resultBundlePath', result], cwd=native, stream=True)
            (Path(temporary) / (str(index) + '.log')).write_text(output)
            result_summary(result, 1)
    finally:
        if ui:
            try:
                stop_repo_apps(profile)
            finally:
                lock.rmdir()


def swift_harnesses(profile):
    products = (profile.get('checks') or {}).get('swift_harness_products')
    if not products:
        raise ValueError('Swift harness tests require explicit typed products')
    for product in products:
        run(['swift', 'run', '-c', 'release', product], cwd=path(profile, 'native_root'), stream=True)


def execute(args, profile):
    stage = args.stage
    if stage == 'generate-project':
        generate_project(profile)
    elif stage == 'bindings-check':
        bindings_check(profile)
    elif stage == 'xcframework':
        assemble_framework(profile)
    elif stage == 'swift-harnesses':
        swift_harnesses(profile)
    elif stage == 'swift-build':
        run(['swift', 'build', '-c', 'release'], cwd=path(profile, 'native_root'), stream=True)
    elif stage == 'swift-test':
        swift_test(profile)
    elif stage == 'xcode-build':
        run(xcode_arguments(profile) + ['ARCHS=arm64', 'CODE_SIGNING_ALLOWED=NO',
            'MACOSX_DEPLOYMENT_TARGET=' + profile['deployment_target'], 'build'],
            cwd=path(profile, 'native_root'), stream=True)
    elif stage in ('xcode-test', 'ui-test'):
        xcode_test(profile, ui=stage == 'ui-test')
    elif stage == 'format':
        format_check(profile)
    elif stage == 'lint':
        checks = profile.get('checks')
        if checks is None:
            raise ValueError('lint requires typed native check policy')
        argv = ['swiftlint', 'lint', '--strict']
        if checks.get('lint_config') is not None:
            argv += ['--config', safe_path(path(profile, 'native_root'), checks['lint_config'])]
        run(argv, cwd=path(profile, 'native_root'))
    elif stage == 'deadcode':
        deadcode(profile)
    elif stage == 'state':
        from desktop_native_state import release_state
        version, _ = version_build(args.version, '1')
        release_state(profile, version, args.repository, args.homebrew_tap)
    else:
        version, build = version_build(args.version, args.build)
        if stage == 'build':
            build_app(profile, version, build)
        elif stage == 'verify':
            archive = safe_path(profile['_root'], args.zip) if args.zip else None
            verify_app(profile, version, build, release=args.release, zip_path=archive)
        elif stage == 'sign':
            from desktop_native_sign import sign
            sign(profile, version, build)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('stage', choices=STAGES)
    parser.add_argument('--profile', required=True)
    parser.add_argument('--source-root', default='.')
    parser.add_argument('--source-sha', required=True)
    parser.add_argument('--rust-artifacts')
    parser.add_argument('--rust-profile-digest')
    parser.add_argument('--version')
    parser.add_argument('--build')
    parser.add_argument('--release', action='store_true')
    parser.add_argument('--zip')
    parser.add_argument('--repository')
    parser.add_argument('--homebrew-tap')
    args = parser.parse_args()
    source = safe_path(Path.cwd(), args.profile).read_text()
    profile = load_profile_source(source, args.source_root)
    validate_source_sha(profile, args.source_sha)
    profile['_source_sha'] = args.source_sha
    profile['_rust_artifacts'] = safe_path(profile['_root'], args.rust_artifacts) if args.rust_artifacts else None
    profile['_rust_profile_digest'] = args.rust_profile_digest
    os.chdir(profile['_root'])
    execute(args, profile)


if __name__ == '__main__':
    main()
