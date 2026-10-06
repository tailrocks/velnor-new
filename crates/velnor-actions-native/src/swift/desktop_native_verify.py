"""Fail-closed static arm64 app and archive verification."""
from pathlib import Path, PurePosixPath
import plistlib
import re
import stat
import tempfile
import zipfile

from desktop_native_core import path, run, safe_path
from desktop_native_build import files


def check_plist(profile, app, version, build):
    apple = profile['apple']
    with (app / 'Contents/Info.plist').open('rb') as handle:
        info = plistlib.load(handle)
    expected = {'CFBundleIdentifier': apple['bundle_identifier'],
                'CFBundleExecutable': apple['app_name'], 'CFBundleName': apple['bundle_name'],
                'CFBundleShortVersionString': version, 'CFBundleVersion': build,
                'LSMinimumSystemVersion': profile['deployment_target'],
                'LSUIElement': apple['bundle_lsui_element']}
    for key, value in expected.items():
        if info.get(key) != value or type(info.get(key)) is not type(value):
            raise ValueError(f'app plist mismatch: {key}')
    for resource in apple['required_resources']:
        if not safe_path(app, resource).exists():
            raise ValueError(f'app resource missing: {resource}')


def check_binary(profile, binary):
    if not binary.is_file():
        raise ValueError('app executable missing')
    if run(['lipo', '-archs', binary]).split() != ['arm64']:
        raise ValueError('app must be arm64 only')
    output = run(['xcrun', 'vtool', '-arch', 'arm64', '-show-build', binary])
    versions = re.findall(r'^\s*minos\s+([0-9]+(?:\.[0-9]+){0,2})\s*$', output, re.MULTILINE)
    expected = version_tuple(profile['deployment_target'])
    if len(versions) != 1 or version_tuple(versions[0]) != expected:
        raise ValueError('app Mach-O deployment target missing or mismatched')
    linkage = run(['otool', '-L', binary])
    static_name = profile['ffi']['static_library'].removesuffix('.a')
    framework = profile['ffi']['framework_name']
    for line in linkage.splitlines()[1:]:
        linked = line.strip().split(' ', 1)[0]
        if (static_name in linked or framework in linked or 'target/' in linked or
                linked.startswith('/') and not linked.startswith(('/System/Library/', '/usr/lib/'))):
            raise ValueError('app contains absolute or dynamic FFI linkage')


def version_tuple(value):
    pieces = [int(part) for part in value.split('.')]
    return tuple(pieces + [0] * (3 - len(pieces)))


def check_embedded(app):
    files(app)
    for item in app.rglob('*'):
        if item.suffix.lower() in ('.dylib', '.a', '.framework', '.xcframework'):
            raise ValueError(f'app embeds a library or framework: {item}')


def verify_app(profile, version, build, release=False, zip_path=None, app=None):
    app = Path(app) if app is not None else path(profile, 'apple.app_path')
    check_embedded(app)
    check_plist(profile, app, version, build)
    check_binary(profile, app / 'Contents/MacOS' / profile['apple']['app_name'])
    run(['codesign', '--verify', '--deep', '--strict', app])
    if release:
        run(['spctl', '--assess', '--type', 'execute', app])
        run(['xcrun', 'stapler', 'validate', app])
    if zip_path is not None:
        verify_zip(profile, zip_path, version, build, release, app)


def archive_inventory(archive, app_name):
    app = app_name + '.app'
    entries = {}
    with zipfile.ZipFile(archive) as handle:
        for item in handle.infolist():
            name = item.filename.rstrip('/')
            parts = PurePosixPath(name).parts
            if (not parts or name.startswith('/') or '\\' in name or
                    any(p in ('', '.', '..') for p in name.split('/')) or name in entries):
                raise ValueError('unsafe or duplicate ZIP entry')
            mode = item.external_attr >> 16
            if stat.S_ISLNK(mode) or (stat.S_IFMT(mode) not in (0, stat.S_IFREG, stat.S_IFDIR)):
                raise ValueError('ZIP contains symlink or special file')
            entries[name] = item
        app_entries = {name for name in entries if name == app or name.startswith(app + '/')}
        if not app_entries:
            raise ValueError('ZIP must contain exactly one expected app')
        for name, item in entries.items():
            if name in app_entries:
                if any(part.endswith('.app') for part in PurePosixPath(name).parts[1:]):
                    raise ValueError('ZIP contains an additional nested app')
            elif name.startswith('__MACOSX/') or name == '__MACOSX':
                check_appledouble(handle, name, item, app_entries)
            else:
                raise ValueError('ZIP contains entries outside the expected app')
    return app


def check_appledouble(handle, name, item, app_entries):
    if name == '__MACOSX':
        if not item.is_dir():
            raise ValueError('AppleDouble metadata root must be a directory')
        return
    relative = name.removeprefix('__MACOSX/')
    if item.is_dir():
        if not any(value == relative or value.startswith(relative + '/') for value in app_entries):
            raise ValueError('AppleDouble directory does not mirror app')
        return
    parts = list(PurePosixPath(relative).parts)
    if not parts[-1].startswith('._') or len(parts[-1]) <= 2:
        raise ValueError('unexpected AppleDouble filename')
    parts[-1] = parts[-1][2:]
    mirrored = '/'.join(parts)
    if not any(value == mirrored or value.startswith(mirrored + '/') for value in app_entries):
        raise ValueError('AppleDouble file does not mirror app entry')
    with handle.open(item) as metadata:
        if metadata.read(8) != b'\x00\x05\x16\x07\x00\x02\x00\x00':
            raise ValueError('invalid AppleDouble metadata header')


def verify_zip(profile, archive, version, build, release, approved_app):
    archive = Path(archive)
    nested = archive_inventory(archive, profile['apple']['app_name'])
    with tempfile.TemporaryDirectory(prefix='velnor-app-verify-') as temporary:
        root = Path(temporary)
        run(['ditto', '-x', '-k', archive, root])
        files(root)
        extracted = safe_path(root, nested)
        if files(extracted) != files(approved_app):
            raise ValueError('ZIP bytes differ from approved signed app')
        verify_app(profile, version, build, release=release, app=extracted)
