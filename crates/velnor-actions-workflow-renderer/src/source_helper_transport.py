"""Decode compiler-owned, size- and digest-bound helper transport."""
import base64
import hashlib
import json
import re

TRANSPORT_PREFIX = 'VELNOR_COMPILED_HELPER_'
CHUNK_SIZE = 8192


def decode_payload(environment, name, size, digest, maximum):
    if not isinstance(size, int) or size < 0 or size > maximum:
        raise SystemExit('source_helper_transport_size')
    encoded_size = ((size + 2) // 3) * 4
    count = (encoded_size + CHUNK_SIZE - 1) // CHUNK_SIZE
    if environment.get(TRANSPORT_PREFIX + name + '_COUNT') != str(count):
        raise SystemExit('source_helper_transport_count')
    chunks = []
    for index in range(count):
        key = TRANSPORT_PREFIX + name + '_' + format(index, '04d')
        chunk = environment.get(key)
        required = min(CHUNK_SIZE, encoded_size - index * CHUNK_SIZE)
        if not isinstance(chunk, str) or len(chunk) != required or not chunk.isascii():
            raise SystemExit('source_helper_transport_chunk')
        chunks.append(chunk)
    try:
        decoded = base64.b64decode(''.join(chunks), validate=True)
    except (ValueError, base64.binascii.Error) as error:
        raise SystemExit('source_helper_transport_encoding') from error
    if len(decoded) != size or hashlib.sha256(decoded).hexdigest() != digest:
        raise SystemExit('source_helper_transport_digest')
    return decoded, count


def decode_transport(environment, source_digest, source_size, args_digest, args_size, execution_digest, execution_size, bindings):
    if environment.get(TRANSPORT_PREFIX + 'SCHEMA') != '1':
        raise SystemExit('source_helper_transport_schema')
    source, source_count = decode_payload(environment, 'SOURCE', source_size, source_digest, 262144)
    arguments, args_count = decode_payload(environment, 'ARGUMENTS', args_size, args_digest, 1048576)
    execution, execution_count = decode_payload(environment, 'EXECUTION', execution_size, execution_digest, 1048576)
    expected = {TRANSPORT_PREFIX + 'SCHEMA', TRANSPORT_PREFIX + 'SOURCE_COUNT',
                TRANSPORT_PREFIX + 'ARGUMENTS_COUNT', TRANSPORT_PREFIX + 'EXECUTION_COUNT'}
    expected.update(TRANSPORT_PREFIX + 'SOURCE_' + format(index, '04d') for index in range(source_count))
    expected.update(TRANSPORT_PREFIX + 'ARGUMENTS_' + format(index, '04d') for index in range(args_count))
    expected.update(TRANSPORT_PREFIX + 'EXECUTION_' + format(index, '04d') for index in range(execution_count))
    actual = {key for key in environment if key.startswith(TRANSPORT_PREFIX)}
    if actual != expected:
        raise SystemExit('source_helper_transport_namespace')
    try:
        text = source.decode('utf-8', errors='strict')
        values = json.loads(arguments.decode('utf-8', errors='strict'))
        prefix = json.loads(execution.decode('utf-8', errors='strict'))
    except (UnicodeError, json.JSONDecodeError) as error:
        raise SystemExit('source_helper_transport_utf8_json') from error
    if not isinstance(values, list) or not isinstance(prefix, list) or len(values) > 2048 or len(prefix) > 2048 or any(
            not isinstance(value, str) or not value or any(ord(ch) < 32 or 127 <= ord(ch) <= 159 or 0xD800 <= ord(ch) <= 0xDFFF for ch in value)
            for value in values + prefix):
        raise SystemExit('source_helper_transport_arguments')
    if '\x00' in text or any(sum(len(value.encode('utf-8')) for value in vector) > 524288 for vector in (values, prefix)):
        raise SystemExit('source_helper_transport_size')
    if prefix and prefix[-1] != '--':
        raise SystemExit('source_helper_transport_execution')
    resolved = [re.sub(r'\$\{\{\s*runner\.temp\s*\}\}',
                       lambda _: environment['RUNNER_TEMP'], value) for value in values]
    root = environment.get('RUNNER_TEMP', '')
    if prefix and (not root.startswith('/') or any(part in ('', '.', '..') for part in root.split('/')[1:])):
        raise SystemExit('source_helper_transport_execution_root')
    if not isinstance(bindings, list) or any(not re.fullmatch(r'[A-Z_][A-Z0-9_]*', key) for key in bindings) or len(set(bindings)) != len(bindings):
        raise SystemExit('source_helper_transport_bindings')
    approved = set(bindings) | {'RUNNER_TEMP'}
    def resolve(match):
        key = match.group(1) or match.group(2)
        if key not in approved or key not in environment:
            raise SystemExit('source_helper_transport_binding')
        value = environment[key]
        if key == 'GITHUB_OUTPUT' and (not value.startswith('/') or len(value.encode('utf-8')) > 4096 or any(part in ('', '.', '..') for part in value.split('/')[1:])):
            raise SystemExit('source_helper_transport_output_path')
        return value
    pattern = r'\$\{([A-Z_][A-Z0-9_]*)\}|\$([A-Z_][A-Z0-9_]*)'
    if any('$' in re.sub(pattern, '', value) for value in prefix):
        raise SystemExit('source_helper_transport_execution_expansion')
    launcher = [re.sub(pattern, resolve, value) for value in prefix]
    return text, resolved, launcher
