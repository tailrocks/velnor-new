# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import hashlib
import json
import os
import re
import stat
import struct
import threading
import time
import tomllib
import urllib.parse


class UploadError(Exception):
    pass


WRAPPER = '''import os,sys
config=os.environ.pop("VELNOR_UPLOAD_CONFIG")
fd=os.open("/tmp/velnor-release-plz.toml",os.O_WRONLY|os.O_CREAT|os.O_EXCL|os.O_NOFOLLOW,0o600)
try:
 data=config.encode("utf-8")
 while data:
  n=os.write(fd,data)
  if n<=0: raise OSError("config write failed")
  data=data[n:]
finally:
 os.close(fd)
program,manifest,repo=sys.argv[1:]
os.execve(program,[program,"release","--config","/tmp/velnor-release-plz.toml","--manifest-path",manifest,"--repo-url",repo,"--forge","github"],os.environ)
'''


def read_upload_context(descriptor, executor):
    return executor.read_sealed_json(descriptor)


def credential(value):
    return isinstance(value, str) and 0 < len(value) <= 8192 and all(32 < ord(c) < 127 for c in value)


def oidc_url(value):
    if not credential(value):
        return False
    try:
        url = urllib.parse.urlsplit(value)
        return (url.scheme == 'https' and url.hostname is not None
                and url.hostname.endswith('.actions.githubusercontent.com')
                and not url.username and not url.password and not url.fragment and url.port in (None, 443))
    except ValueError:
        return False


def validate_config(config, package):
    if not isinstance(config, str) or len(config.encode('utf-8')) > 65536:
        raise UploadError('invalid upload configuration')
    try:
        value = tomllib.loads(config)
    except ValueError:
        raise UploadError('invalid upload configuration') from None
    if not isinstance(value, dict) or set(value) != {'workspace', 'package'}:
        raise UploadError('invalid upload configuration')
    workspace, packages = value['workspace'], value['package']
    expected = {'release': False, 'git_tag_enable': False, 'git_release_enable': False,
                'release_always': True, 'pr_branch_prefix': ':', 'semver_check': True,
                'publish_no_verify': False, 'publish_allow_dirty': False}
    if (not isinstance(workspace, dict) or set(workspace) != set(expected) | {'git_tag_name'}
            or any(workspace[key] is not expected[key] for key in expected if isinstance(expected[key], bool))
            or workspace.get('pr_branch_prefix') != ':' or not isinstance(workspace.get('git_tag_name'), str)
            or not isinstance(packages, list) or len(packages) != 1):
        raise UploadError('invalid upload configuration')
    item = packages[0]
    keys = {'name', 'release', 'publish', 'git_only'}
    if package.features:
        keys.add('publish_features')
    if (not isinstance(item, dict) or set(item) != keys or item.get('name') != package.name
            or item.get('release') is not True or item.get('publish') is not True
            or item.get('git_only') is not False
            or (package.features and item.get('publish_features') != list(package.features))):
        raise UploadError('invalid upload configuration')


def validate_request(value, context, package_index):
    keys = {'source_sha', 'intent_sha256', 'package', 'version', 'features', 'config',
            'release_plz_sha256', 'sequence'}
    if (not isinstance(value, dict) or set(value) != keys or context is None
            or type(package_index) is not int or not 0 <= package_index < len(context.packages)):
        raise UploadError('invalid upload request')
    package = context.packages[package_index]
    sequence = value['sequence']
    if (type(sequence) is not int or not 0 < sequence < 2**64
            or value['source_sha'] != context.source_sha
            or value['intent_sha256'] != context.identity.intent_sha256
            or value['package'] != package.name or value['version'] != package.version
            or value['features'] != list(package.features)
            or value['release_plz_sha256'] != dict(context.tools).get('release_plz')):
        raise UploadError('upload context mismatch')
    validate_config(value['config'], package)
    digest = hashlib.sha256(value['config'].encode('utf-8')).hexdigest()
    if digest != package.upload_config_sha256:
        raise UploadError('upload configuration mismatch')
    return package, sequence


def validate_custody(session, executor):
    context, root, deadline = session.bound, session.root, session._deadline
    session.verify_view()
    if (os.geteuid() != 0 or session.manager.root_bootstrap or not session.manager.deny_unix
            or not session.manager.bind_to or not os.path.isabs(root) or os.path.dirname(root) != '/run'
            or not re.fullmatch(r'velnor-publisher-[a-zA-Z0-9_-]+', os.path.basename(root))):
        raise UploadError('invalid upload custody')
    source = root + '/source'
    toolchain = root + '/rustup/toolchains/' + context.rust_version + '-x86_64-unknown-linux-gnu'
    toolbin = toolchain + '/bin'
    for directory in ('/run', root, source, root + '/rustup', root + '/rustup/toolchains', toolchain,
                      toolbin, root + '/git', root + '/release-plz'):
        info = os.stat(directory, follow_symlinks=False)
        if not stat.S_ISDIR(info.st_mode) or info.st_uid != 0 or info.st_mode & 0o022:
            raise UploadError('invalid upload custody')
    if any(os.path.lexists('/.cargo/' + name) for name in ('config', 'config.toml')):
        raise UploadError('host cargo config unsupported')
    executor.verify_immutable_tree(source, deadline, True)
    executor.verify_detached_head(source, context.source_sha)
    executor.verify_source_inventory(source, context.source_files, deadline)
    executor.validate_source_model(source, context.manifest)
    for directory in (toolchain, root + '/release-plz'):
        executor.verify_immutable_tree(directory, deadline, False)
    executor.verify_immutable_tree(root + '/git', deadline, True)
    if (set(os.listdir(root + '/git')) != {'git', 'git-remote-http', 'git-remote-https'}
            or not os.path.islink(root + '/git/git-remote-https')
            or os.readlink(root + '/git/git-remote-https') != 'git-remote-http'):
        raise UploadError('invalid git bundle')
    paths = {'cargo': toolbin + '/cargo', 'rustc': toolbin + '/rustc', 'rustdoc': toolbin + '/rustdoc',
             'git': root + '/git/git', 'git_http': root + '/git/git-remote-http',
             'release_plz': root + '/release-plz/release-plz'}
    for name, path in paths.items():
        executor.verify_tool(path, dict(context.tools)[name], deadline)
    session.verify_view()
    return source, toolbin, paths


def preflight(session, secrets):
    if (session.poisoned or time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= session._deadline
            or session.bound is None or not isinstance(secrets, dict)):
        raise UploadError('upload unavailable')
    session.verify_view()
    mode = session.bound.authentication
    trusted = (mode == 'trusted-publishing' and set(secrets) == {'github_token', 'oidc_token', 'oidc_url'}
               and credential(secrets['oidc_token']) and oidc_url(secrets['oidc_url']))
    bootstrap = (mode == 'bootstrap-token' and set(secrets) == {'github_token', 'cargo_token'}
                 and credential(secrets['cargo_token']))
    if (not (trusted or bootstrap) or not credential(secrets.get('github_token'))):
        raise UploadError('upload unavailable')


def upload(session, executor, broker_module, request, package_index, secrets):
    preflight(session, secrets)
    package, sequence = validate_request(request, session.bound, package_index)
    if sequence <= getattr(session, 'upload_sequence', 0) or package.name in getattr(session, 'uploaded_packages', set()):
        raise UploadError('upload already attempted')
    source, toolbin, paths = validate_custody(session, executor)
    owner, repository = session.bound.identity.repository.split('/')
    upstream = broker_module.GitHubTransport(secrets['github_token'])
    broker = broker_module.ForgeBroker(owner, repository, session.bound.source_sha, session._deadline, upstream)
    worker = threading.Thread(target=broker.serve, daemon=True)
    worker.start()
    session.poisoned = True
    session.upload_sequence = sequence
    session.uploaded_packages = getattr(session, 'uploaded_packages', set()) | {package.name}
    try:
        env = {'PATH': toolbin + ':' + session.root + '/git:/usr/bin:/bin', 'HOME': '/tmp',
               'CARGO_HOME': '/tmp/velnor-cargo-home', 'CARGO_TARGET_DIR': '/tmp/velnor-target',
               'CARGO': paths['cargo'], 'RUSTC': paths['rustc'], 'RUSTDOC': paths['rustdoc'],
               'GIT_EXEC_PATH': session.root + '/git', 'GIT_CONFIG_NOSYSTEM': '1',
               'GIT_CONFIG_GLOBAL': '/dev/null', 'GIT_TERMINAL_PROMPT': '0',
               'GIT_CONFIG_COUNT': '1', 'GIT_CONFIG_KEY_0': 'safe.directory', 'GIT_CONFIG_VALUE_0': source,
               'GIT_TOKEN': 'velnor-inert-no-authority', 'VELNOR_UPLOAD_CONFIG': request['config']}
        if session.bound.authentication == 'trusted-publishing':
            env.update({'GITHUB_ACTIONS': 'true', 'ACTIONS_ID_TOKEN_REQUEST_TOKEN': secrets['oidc_token'],
                        'ACTIONS_ID_TOKEN_REQUEST_URL': secrets['oidc_url']})
        else:
            env['CARGO_REGISTRY_TOKEN'] = secrets['cargo_token']
        repo = f'http://127.0.0.1:{broker.port}/{owner}/{repository}'
        payload = {'program': '/usr/bin/python3', 'args': ['-I', '-S', '-c', WRAPPER, paths['release_plz'],
                   source + '/' + session.bound.manifest, repo], 'cwd': source, 'env': env, 'stdin': ''}
        body = json.dumps(payload, separators=(',', ':')).encode('utf-8')
        if not 0 < len(body) <= 1024 * 1024:
            raise UploadError('invalid upload frame')
        namespace = os.stat('/proc/self/ns/time').st_ino
        frame = struct.pack('>QQI', session._deadline, namespace, len(body)) + body
        session.lifecycle.supervise(session.manager, frame, session._deadline)
        validate_custody(session, executor)
    finally:
        try:
            broker.close()
            worker.join(timeout=1)
        except Exception:
            raise UploadError('upload cleanup uncertain')
        if worker.is_alive():
            raise UploadError('upload cleanup uncertain')
    session.poisoned = False
