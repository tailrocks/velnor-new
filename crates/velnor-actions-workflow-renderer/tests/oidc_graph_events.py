#!/usr/bin/env python3
"""Evaluate fixed generated conditions against synthetic GitHub events."""
import json
import math
import re
import sys

TOKEN = re.compile(r"\s*(==|!=|&&|\|\||!|\(|\)|'[^']*'|[A-Za-z_][A-Za-z0-9_.-]*)")


def evaluate(expression, context):
    tokens = TOKEN.findall(expression)
    if ''.join(tokens) != re.sub(r"\s+", "", expression):
        raise ValueError(f"unsupported expression: {expression}")
    index = 0

    def atom():
        nonlocal index
        token = tokens[index]
        index += 1
        if token == '!':
            return not atom()
        if token == '(':
            result = disjunction()
            if tokens[index] != ')':
                raise ValueError('unbalanced expression')
            index += 1
            return result
        if token == 'always':
            if tokens[index:index + 2] != ['(', ')']:
                raise ValueError('unsupported function')
            index += 2
            return True
        if token.startswith("'"):
            return token[1:-1]
        value = context
        for segment in token.split('.'):
            value = value.get(segment, {}) if isinstance(value, dict) else {}
        return value if not isinstance(value, dict) else None

    def compare():
        nonlocal index
        left = atom()
        if index < len(tokens) and tokens[index] in ('==', '!='):
            op = tokens[index]
            index += 1
            right = atom()
            def number(value):
                if value is None:
                    return 0.0
                if isinstance(value, bool):
                    return float(value)
                if isinstance(value, (int, float)):
                    return float(value)
                if isinstance(value, str):
                    try:
                        return float(value.strip()) if value.strip() else 0.0
                    except ValueError:
                        return math.nan
                return math.nan
            equal = left.casefold() == right.casefold() if isinstance(left, str) and isinstance(right, str) else number(left) == number(right)
            return equal if op == '==' else not equal
        return bool(left)

    def conjunction():
        nonlocal index
        result = compare()
        while index < len(tokens) and tokens[index] == '&&':
            index += 1
            rhs = compare()
            result = result and rhs
        return result

    def disjunction():
        nonlocal index
        result = conjunction()
        while index < len(tokens) and tokens[index] == '||':
            index += 1
            rhs = conjunction()
            result = result or rhs
        return result

    result = disjunction()
    if index != len(tokens):
        raise ValueError('trailing expression')
    return result


def run_graph(graph, event, eligible='true', failures=()):
    jobs = graph['jobs']
    pending = set(jobs)
    state = {}
    while pending:
        ready = sorted(job for job in pending if all(dep in state for dep in jobs[job].get('needs', [])))
        if not ready:
            raise ValueError('cyclic job graph')
        for name in ready:
            job = jobs[name]
            needs = {dep: state[dep] for dep in job.get('needs', [])}
            expression = job.get('if', 'always()')
            implicit_success = 'always()' in expression or all(v['result'] == 'success' for v in needs.values())
            active = implicit_success and evaluate(expression, {'github': event, 'needs': needs})
            result = 'failure' if active and (name == 'reject_invalid_dispatch' or name in failures) else ('success' if active else 'skipped')
            outputs = {'eligible': eligible} if name == 'release-preflight' else {'changed': 'true'}
            state[name] = {'result': result, 'outputs': outputs}
            pending.remove(name)
    return {k: v['result'] for k, v in state.items()}


def event(kind, mode='', **changes):
    values = {'plan': 'release-aaaaaaaaaaaa', 'source_sha': 'a' * 40, 'version': '0.1.1', 'mode': mode}
    values.update(changes)
    return {'repository': 'owner/project', 'event_name': kind, 'ref': 'refs/heads/main', 'event': {'inputs': values}}


def check(graph, ev, active, rejected=False, eligible='true'):
    state = run_graph(graph, ev, eligible)
    actual = {name for name, result in state.items() if result != 'skipped'}
    expected = set(active)
    if rejected:
        expected.add('reject_invalid_dispatch')
        assert state['reject_invalid_dispatch'] == 'failure'
    assert actual == expected, (ev, actual, expected, state)


def main(recovery, normal):
    with open(recovery) as f:
        recovery_graph = json.load(f)
    with open(normal) as f:
        normal_graph = json.load(f)
    qualification = {'oidc_qualification'}
    publish = {'release-preflight', 'release-publish', 'release-reconcile'}
    preparation = {'release-preflight', 'release-preparation', 'release-pr-create'}
    for graph in (normal_graph, recovery_graph):
        check(graph, event('workflow_dispatch', 'oidc-qualification'), qualification)
        for changes in ({'plan': 'wrong'}, {'source_sha': 'b' * 40}, {'version': '0.1.2'}, {'plan': ''}, {'source_sha': ''}, {'version': ''}):
            check(graph, event('workflow_dispatch', 'release', **changes), set(), rejected=True)
        check(graph, event('workflow_dispatch', 'bad'), set(), rejected=True)
        check(graph, event('workflow_dispatch', ''), set(), rejected=True)
        for nonstring in (None, False, 0, True):
            check(graph, event('workflow_dispatch', nonstring), set(), rejected=True)
        check(graph, event('pull_request', 'oidc-qualification'), set())
        for key, val in [('ref', 'refs/heads/feature'), ('repository', 'other/project')]:
            ev = event('workflow_dispatch', 'oidc-qualification')
            ev[key] = val
            check(graph, ev, set(), rejected=True)
    # GitHub Actions comparisons are case-insensitive. These tests model routing;
    # the Rust admission command separately requires canonical qualification mode.
    check(recovery_graph, event('workflow_dispatch', 'RELEASE'), publish)
    check(recovery_graph, event('workflow_dispatch', 'release', source_sha='A' * 40), publish)
    for graph in (normal_graph, recovery_graph):
        check(graph, event('workflow_dispatch', 'OIDC-QUALIFICATION'), qualification)
        ev = event('workflow_dispatch', 'oidc-qualification')
        ev['repository'] = 'OWNER/PROJECT'
        check(graph, ev, qualification)
    check(recovery_graph, event('workflow_dispatch', 'release'), publish)
    failed_preflight = run_graph(recovery_graph, event('workflow_dispatch', 'release'), failures={'release-preflight'})
    assert failed_preflight['release-publish'] == 'skipped' and failed_preflight['release-reconcile'] == 'skipped'
    failed_publish = run_graph(recovery_graph, event('workflow_dispatch', 'release'), failures={'release-publish'})
    assert failed_publish['release-publish'] == 'failure' and failed_publish['release-reconcile'] == 'success'
    check(recovery_graph, event('push'), set())
    check(normal_graph, event('push'), publish)
    check(normal_graph, event('push'), preparation, eligible='false')
    check(normal_graph, event('workflow_dispatch', 'release'), set(), rejected=True)
    print('PASS: rendered graph event, input, branch, repository and dependency scenarios')


if __name__ == '__main__':
    main(*sys.argv[1:])
