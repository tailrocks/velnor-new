"""Resolve one installed catalog JDK and persist Gradle's five Java authorities."""
import hashlib
import json
import os
import pathlib
import stat
import subprocess


def directory(path, create=False):
    parts = pathlib.Path(path).parts
    if (not parts or parts[0] != '/' or str(pathlib.Path(path)) != path
            or any(part in ('.', '..') for part in parts)):
        raise ValueError('java_materialize_path')
    descriptor = os.open('/', os.O_RDONLY | os.O_DIRECTORY)
    walked = ''
    boundary = os.environ['RUNNER_TEMP']
    try:
        for part in parts[1:]:
            if create:
                try:
                    os.mkdir(part, 0o700, dir_fd=descriptor)
                except FileExistsError:
                    pass
            child = os.open(part, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW,
                            dir_fd=descriptor)
            os.close(descriptor)
            descriptor = child
            walked += '/' + part
            if walked == boundary or walked.startswith(boundary + '/'):
                info = os.fstat(descriptor)
                if info.st_uid != os.geteuid() or info.st_mode & 0o022:
                    raise ValueError('java_materialize_owner')
        info = os.fstat(descriptor)
        if info.st_uid != os.geteuid() or info.st_mode & 0o022:
            raise ValueError('java_materialize_owner')
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def regular(parent, name, executable=False):
    descriptor = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=parent)
    try:
        info = os.fstat(descriptor)
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.geteuid()
                or info.st_mode & 0o022 or info.st_nlink != 1
                or executable and not info.st_mode & 0o100):
            raise ValueError('java_materialize_file')
        return descriptor
    except BaseException:
        os.close(descriptor)
        raise


def empty_directory(path):
    descriptor = directory(path, create=True)
    try:
        if os.listdir(descriptor):
            raise ValueError('java_materialize_configuration')
    finally:
        os.close(descriptor)


def properties_value(value):
    result = []
    for char in value:
        code = ord(char)
        if code < 32 or 127 <= code <= 159 or char == ',':
            raise ValueError('java_materialize_home_encoding')
        if char in '\\ :=#!':
            result.append('\\' + char)
        elif code > 126:
            encoded = char.encode('utf-16-be')
            result.extend('\\u' + encoded[index:index + 2].hex()
                          for index in range(0, len(encoded), 2))
        else:
            result.append(char)
    return ''.join(result)


def write_properties(path, home):
    value = properties_value(home)
    properties = [
        ('org.gradle.java.home', value),
        ('org.gradle.java.installations.auto-detect', 'false'),
        ('org.gradle.java.installations.auto-download', 'false'),
        ('org.gradle.java.installations.fromEnv', 'JAVA_HOME'),
        ('org.gradle.java.installations.paths', value),
    ]
    payload = ''.join(key + '=' + item + '\n' for key, item in properties).encode('ascii')
    parent = directory(path, create=True)
    temporary = '.velnor-java-' + os.urandom(16).hex()
    descriptor = None
    try:
        try:
            os.stat('jdks', dir_fd=parent, follow_symlinks=False)
        except FileNotFoundError:
            pass
        else:
            raise ValueError('java_materialize_foreign_jdk_cache')
        try:
            existing = regular(parent, 'gradle.properties')
        except FileNotFoundError:
            pass
        else:
            os.close(existing)
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW,
                             0o600, dir_fd=parent)
        with os.fdopen(descriptor, 'wb') as output:
            descriptor = None
            output.write(payload)
            output.flush()
            os.fsync(output.fileno())
        os.replace(temporary, 'gradle.properties', src_dir_fd=parent, dst_dir_fd=parent)
        os.fsync(parent)
    finally:
        if descriptor is not None:
            os.close(descriptor)
        try:
            os.unlink(temporary, dir_fd=parent)
        except FileNotFoundError:
            pass
        os.close(parent)


def qualified_home(root, output, java):
    selected = output.removesuffix('\n')
    expected = root + '/' + java['install_root']
    if (selected != expected or len(output) > 4096
            or str(pathlib.Path(selected).resolve(strict=True)) != selected):
        raise ValueError('java_materialize_foreign_home')
    properties_value(selected)
    os.close(directory(selected))
    home = root + '/' + java['home']
    properties_value(home)
    os.close(directory(home))
    for entry in java['launch']:
        path = pathlib.Path(root + '/' + entry['path'])
        parent = directory(str(path.parent))
        try:
            descriptor = regular(parent, path.name, executable=True)
            try:
                with os.fdopen(os.dup(descriptor), 'rb') as source:
                    digest = hashlib.sha256(source.read(128 * 1024 * 1024 + 1)).hexdigest()
                if digest != entry['sha256']:
                    raise ValueError('java_materialize_launch_digest')
            finally:
                os.close(descriptor)
        finally:
            os.close(parent)
    return home


def materialize(domain, configuration):
    if domain not in configuration['domains']:
        raise ValueError('java_materialize_domain')
    temp = os.environ['RUNNER_TEMP']
    if (not temp.startswith('/') or any(part in ('', '.', '..') for part in temp.split('/')[1:])
            or any(ord(char) < 32 or 127 <= ord(char) <= 159 for char in temp)):
        raise ValueError('java_materialize_temp')
    owned = configuration['domains'][domain]
    root = temp + '/velnor/' + owned['mise']
    gradle = temp + '/velnor/' + owned['gradle']
    if os.environ.get('MISE_DATA_DIR') != root or os.environ.get('GRADLE_USER_HOME') != gradle:
        raise ValueError('java_materialize_binding')
    for path in (temp, temp + '/velnor', root):
        os.close(directory(path))
    binary_parent = directory(root + '/bin')
    try:
        binary = regular(binary_parent, 'mise', executable=True)
        try:
            with os.fdopen(os.dup(binary), 'rb') as source:
                digest = hashlib.sha256(source.read(128 * 1024 * 1024 + 1)).hexdigest()
            if digest != configuration['binary_sha256']:
                raise ValueError('java_materialize_mise_digest')
        finally:
            os.close(binary)
    finally:
        os.close(binary_parent)
    config = root + '/velnor-empty-config'
    system_config = root + '/velnor-empty-system-config'
    for path in (config, system_config):
        empty_directory(path)
    home_directory = root + '/velnor-java-home'
    os.close(directory(home_directory, create=True))
    environment = dict(configuration['isolation'])
    environment.update(HOME=home_directory, PATH='/usr/bin:/bin:/usr/sbin:/sbin',
                       MISE_DATA_DIR=root, MISE_CONFIG_DIR=config,
                       MISE_SYSTEM_CONFIG_DIR=system_config, MISE_CEILING_PATHS=root,
                       MISE_OFFLINE='true')
    result = subprocess.run([root + '/bin/mise', *configuration['flags'], 'where',
                             configuration['java']['selector']], cwd=root, env=environment,
                            stdin=subprocess.DEVNULL, capture_output=True, check=True, timeout=600)
    output = result.stdout.decode('utf-8', errors='strict')
    selected = qualified_home(root, output, configuration['java'])
    write_properties(gradle, selected)
    for variable in ('GITHUB_ENV', 'GITHUB_PATH'):
        with open(os.environ[variable], 'a', encoding='utf-8') as output:
            output.write(('JAVA_HOME=' + selected if variable == 'GITHUB_ENV'
                          else selected + '/bin') + '\n')
