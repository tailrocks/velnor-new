# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import ast
import base64
import hashlib
import json
import time


def unique(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError('exported closure duplicate key')
        result[key] = value
    return result


def decoded(value):
    if type(value) is not str:
        raise ValueError('exported closure payload type')
    result = base64.b64decode(value, validate=True)
    if (not 0 < len(result) <= 512 * 1024
            or base64.b64encode(result).decode('ascii') != value):
        raise ValueError('exported closure payload encoding')
    return result


def constants(exported, names):
    assignments = {name: [] for name in names}
    for node in ast.walk(ast.parse(exported)):
        if isinstance(node, (ast.Assign, ast.AnnAssign, ast.AugAssign, ast.NamedExpr)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            for target in targets:
                if isinstance(target, ast.Name) and target.id in assignments:
                    assignments[target.id].append(node)
    if any(len(nodes) != 1 or not isinstance(nodes[0], ast.Assign) or len(nodes[0].targets) != 1
           or not isinstance(nodes[0].value, ast.Constant) or type(nodes[0].value.value) is not str
           for nodes in assignments.values()):
        raise ValueError('exported closure constants unavailable')
    return {name: nodes[0].value.value for name, nodes in assignments.items()}


def verify(exported, entry, operation, deadline):
    def check():
        if time.clock_gettime_ns(time.CLOCK_BOOTTIME) >= deadline:
            raise ValueError('exported closure deadline')
    check()
    contract = json.loads(decoded(entry['EXPORT_CONTRACT']), object_pairs_hook=unique)
    if (type(contract) is not dict or set(contract) != {'schema', 'bootstrap', 'supervisor'}
            or contract['schema'] != 1 or type(contract['bootstrap']) is not list
            or type(contract['supervisor']) is not dict):
        raise ValueError('trusted export closure shape')
    names = {'SUPERVISOR', 'EXPORT_CONTRACT', 'WATCH_OPERATION'}
    markers = set()
    for item in contract['bootstrap']:
        if (type(item) is not dict or set(item) != {'key', 'constant', 'marker', 'sha256'}
                or any(type(value) is not str for value in item.values())
                or item['constant'] in names or item['marker'] in markers):
            raise ValueError('trusted export closure descriptor')
        names.add(item['constant'])
        markers.add(item['marker'])
    values = constants(exported, names)
    if values['WATCH_OPERATION'] != operation or values['EXPORT_CONTRACT'] != entry['EXPORT_CONTRACT']:
        raise ValueError('exported closure identity mismatch')
    expected = entry['EXPORT_TEMPLATE']
    for item in contract['bootstrap']:
        check()
        value = values[item['constant']]
        if hashlib.sha256(decoded(value)).hexdigest() != item['sha256']:
            raise ValueError('exported module bytes mismatch')
        marker = '__VELNOR_' + item['marker'] + '_PAYLOAD__'
        if expected.count(marker) != 1:
            raise ValueError('trusted export closure marker')
        expected = expected.replace(marker, value)
    supervisor = json.loads(decoded(values['SUPERVISOR']), object_pairs_hook=unique)
    if type(supervisor) is not dict or set(supervisor) != set(contract['supervisor']):
        raise ValueError('exported supervisor closure shape')
    for name, digest in contract['supervisor'].items():
        check()
        if hashlib.sha256(decoded(supervisor[name])).hexdigest() != digest:
            raise ValueError('exported supervisor bytes mismatch')
    expected = expected.replace('__VELNOR_SUPERVISOR_PAYLOAD__', values['SUPERVISOR'])
    expected = expected.replace('__VELNOR_EXPORT_CONTRACT_PAYLOAD__', entry['EXPORT_CONTRACT'])
    expected = expected.replace('__VELNOR_JOB_TIMEOUT_SECONDS__', str(entry['JOB_TIMEOUT_SECONDS']))
    if exported != expected.encode('utf-8'):
        raise ValueError('exported outer entry mismatch')
    check()
