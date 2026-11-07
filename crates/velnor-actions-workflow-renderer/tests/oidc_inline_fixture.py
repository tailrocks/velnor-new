# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import base64
import contextlib
import copy
import io
import json
import os
import pathlib
import unittest
from unittest.mock import patch
import urllib.parse


ROOT = pathlib.Path(__file__).resolve().parents[1] / 'src'
SCOPE = {}
exec(compile((ROOT / 'crate_oidc_probe.py').read_text(), str(ROOT / 'crate_oidc_probe.py'), 'exec'), SCOPE)
exec(compile((ROOT / 'crate_oidc_admission.py').read_text(), str(ROOT / 'crate_oidc_admission.py'), 'exec'), SCOPE)

SHA = 'a' * 40
HEAD = 'b' * 40
TREE = 'c' * 40
REPO = 'example/project'
PREFIX = f'repos/{REPO}'


class Response(io.BytesIO):
    status = 200

    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


class Remote:
    def __init__(self):
        self.calls = []
        self.override = {}
        owner = {'id': 11, 'login': 'example'}
        self.repo = {'id': 12, 'full_name': REPO, 'owner': owner, 'default_branch': 'main'}
        self.repo_identity = {'id': 12, 'full_name': REPO, 'owner': owner}
        self.pull = {'number': 42, 'state': 'closed', 'merged': True, 'merged_at': '2026-01-03T00:00:00Z',
                     'merge_commit_sha': SHA, 'user': {'id': 21, 'login': 'author'},
                     'head': {'sha': HEAD, 'repo': self.repo_identity},
                     'base': {'ref': 'main', 'repo': self.repo_identity}}
        self.run = {'id': 30, 'run_attempt': 1, 'head_sha': SHA, 'head_branch': 'main',
                    'event': 'workflow_dispatch', 'path': '.github/workflows/release.yml',
                    'repository': self.repo_identity, 'head_repository': self.repo_identity}
        self.claims = {'iss': 'https://token.actions.githubusercontent.com', 'aud': 'crates.io',
                       'repository': REPO, 'repository_id': '12', 'repository_owner_id': '11',
                       'run_id': '30', 'run_attempt': '1', 'environment': 'release',
                       'event_name': 'workflow_dispatch', 'ref': 'refs/heads/main', 'sha': SHA,
                       'workflow_sha': SHA, 'workflow_ref': f'{REPO}/.github/workflows/release.yml@refs/heads/main',
                       'runner_environment': 'github-hosted', 'check_run_id': '40'}
        self.environment = {'id': 50, 'name': 'release', 'can_admins_bypass': False,
                            'deployment_branch_policy': {'custom_branch_policies': True},
                            'protection_rules': [{'type': 'required_reviewers', 'reviewers': [
                                {'type': 'User', 'reviewer': {'id': 60}}]}]}

    def open(self, request, timeout):
        del timeout
        url = urllib.parse.urlsplit(request.full_url)
        self.calls.append((request.method, url.hostname, url.path))
        if url.hostname == 'api.github.com':
            route = url.path.lstrip('/')
            data = self.lookup(route, urllib.parse.parse_qs(url.query))
            return Response(json.dumps(data).encode())
        if url.hostname == 'runner.actions.githubusercontent.com':
            encoded = base64.urlsafe_b64encode(json.dumps(self.claims).encode()).rstrip(b'=').decode()
            return Response(json.dumps({'value': 'header.' + encoded + '.signature'}).encode())
        if url.hostname == 'crates.io' and request.method == 'POST':
            return Response(b'{"token":"test-token"}')
        if url.hostname == 'crates.io' and request.method == 'DELETE':
            return Response(b'{}')
        raise AssertionError('unexpected endpoint')

    def lookup(self, route, query):
        if route in self.override:
            return copy.deepcopy(self.override[route])
        if route == PREFIX:
            return self.repo
        if route == f'{PREFIX}/commits/{SHA}/pulls':
            return [self.pull]
        if route == f'{PREFIX}/pulls/42':
            return self.pull
        if route == f'{PREFIX}/git/commits/{SHA}':
            return {'sha': SHA, 'tree': {'sha': TREE}}
        if route in (f'{PREFIX}/compare/{SHA}...main', f'{PREFIX}/compare/{SHA}...{SHA}'):
            return {'status': 'identical'}
        if route == f'{PREFIX}/rules/branches/main':
            return [{'type': 'required_status_checks', 'parameters': {'required_status_checks': [{'context': 'Required'}]}},
                    {'type': 'pull_request', 'parameters': {'required_review_thread_resolution': True}}]
        if route == f'{PREFIX}/pulls/42/reviews':
            return [{'id': 22, 'state': 'APPROVED', 'user': {'id': 23, 'login': 'reviewer', 'type': 'User'},
                     'author_association': 'MEMBER', 'submitted_at': '2026-01-02T00:00:00Z', 'commit_id': HEAD}]
        if route == f'{PREFIX}/collaborators/reviewer/permission':
            return {'user': {'id': 23, 'type': 'User'}, 'permission': 'write'}
        if route.startswith(f'{PREFIX}/actions/jobs/'):
            job_id = int(route.rsplit('/', 1)[1])
            return {'id': job_id, 'run_id': 30 if job_id == 40 else 31, 'run_attempt': 1, 'head_sha': SHA,
                    'name': 'OIDC qualification', 'status': 'in_progress'}
        if route == f'{PREFIX}/actions/runs/30':
            return self.run
        if route == f'{PREFIX}/environments/release':
            return self.environment
        if route == f'{PREFIX}/environments/release/deployment-branch-policies':
            return {'total_count': 1, 'branch_policies': [{'type': 'branch', 'name': 'main'}]}
        if route == f'{PREFIX}/actions/runs/30/approvals':
            return [{'environments': [{'id': 50, 'name': 'release'}], 'state': 'approved',
                     'user': {'id': 60, 'type': 'User'},
                     'comment': f'I reviewed the complete release policy at {SHA} and source {SHA}, and authorize this source for run 30.'}]
        if route == f'{PREFIX}/actions/runs':
            source = query['head_sha'][0]
            run_id = 100 if source == HEAD else 101
            suite = 200 if source == HEAD else 201
            return {'total_count': 1, 'workflow_runs': [{'id': run_id, 'run_number': 1, 'run_attempt': 1,
                     'check_suite_id': suite, 'head_sha': source, 'head_branch': 'main', 'event': 'push',
                     'path': '.github/workflows/ci.yml', 'status': 'completed', 'conclusion': 'success',
                     'repository': self.repo_identity, 'head_repository': self.repo_identity}]}
        if route in (f'{PREFIX}/actions/runs/100/attempts/1/jobs', f'{PREFIX}/actions/runs/101/attempts/1/jobs'):
            run_id = int(route.split('/')[-4])
            job_id = run_id + 200
            return {'total_count': 1, 'jobs': [{'id': job_id, 'name': 'Required', 'run_id': run_id,
                     'run_attempt': 1, 'check_run_url': f'https://api.github.com/{PREFIX}/check-runs/{job_id}',
                     'status': 'completed', 'conclusion': 'success'}]}
        if route in (f'{PREFIX}/commits/{SHA}/check-runs', f'{PREFIX}/commits/{HEAD}/check-runs'):
            source = route.split('/')[-2]
            suite = 200 if source == HEAD else 201
            job_id = 300 if source == HEAD else 301
            return {'total_count': 1, 'check_runs': [{'id': job_id, 'name': 'Required', 'head_sha': source,
                     'check_suite': {'id': suite}, 'app': {'id': 15368, 'slug': 'github-actions'},
                     'started_at': '2026-01-02T00:00:00Z', 'status': 'completed', 'conclusion': 'success'}]}
        raise AssertionError('unexpected GitHub route: ' + route)


def context():
    return {'GITHUB_REPOSITORY': REPO, 'VELNOR_QUALIFICATION_DEFAULT_BRANCH': 'main',
            'VELNOR_QUALIFICATION_ENVIRONMENT': 'release', 'VELNOR_QUALIFICATION_REQUIRED_WORKFLOW': '.github/workflows/ci.yml',
            'GITHUB_SHA': SHA, 'GITHUB_EVENT_NAME': 'workflow_dispatch', 'VELNOR_QUALIFICATION_MODE': 'oidc-qualification',
            'GITHUB_REF': 'refs/heads/main', 'GITHUB_WORKFLOW_SHA': SHA, 'GITHUB_JOB': 'oidc_qualification',
            'GITHUB_WORKFLOW_REF': f'{REPO}/.github/workflows/release.yml@refs/heads/main',
            'VELNOR_QUALIFICATION_REVIEWER_ID': '60', 'GITHUB_RUN_ID': '30', 'GITHUB_RUN_ATTEMPT': '1',
            'GIT_TOKEN': 'test-github-token', 'ACTIONS_ID_TOKEN_REQUEST_URL': 'https://runner.actions.githubusercontent.com/id',
            'ACTIONS_ID_TOKEN_REQUEST_TOKEN': 'test-oidc-token'}


class AdmissionBehavior(unittest.TestCase):
    def test_approved_source_exchanges_and_revokes(self):
        remote = Remote()
        SCOPE['run_qualification'](context(), remote, remote)
        self.assertEqual([call[0] for call in remote.calls if call[1] != 'api.github.com'], ['GET', 'POST', 'DELETE'])

    def test_rejected_admission_never_requests_a_credential(self):
        cases = (
            (f'{PREFIX}/actions/runs/30/approvals', []),
            (f'{PREFIX}/pulls/42/reviews', []),
            (f'{PREFIX}/rules/branches/main', []),
            (f'{PREFIX}/commits/{SHA}/pulls', []),
            (f'{PREFIX}/environments/release/deployment-branch-policies', {'total_count': 0, 'branch_policies': []}),
        )
        for route, replacement in cases:
            with self.subTest(route=route):
                remote = Remote()
                remote.override[route] = replacement
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))


class BoundaryMutations(unittest.TestCase):
    def test_invalid_context_or_evidence_cannot_request_a_credential(self):
        def reject(remote, env):
            with self.assertRaises(SCOPE['AdmissionError']):
                SCOPE['run_qualification'](env, remote, remote)
            self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

        for key, bad in (('GITHUB_RUN_ATTEMPT', '2'), ('GITHUB_WORKFLOW_SHA', 'd' * 40),
                         ('GITHUB_REF', 'refs/heads/other'), ('GITHUB_EVENT_NAME', 'push'),
                         ('VELNOR_QUALIFICATION_MODE', 'release'), ('GITHUB_JOB', 'other')):
            with self.subTest(context=key):
                env = context()
                env[key] = bad
                reject(Remote(), env)

        mutations = (
            ('foreign repository', lambda r: r.repo.update(id=99)),
            ('unmerged pull', lambda r: r.pull.update(merged=False)),
            ('wrong head repository', lambda r: r.pull['head'].update(repo={'id': 99, 'full_name': REPO})),
            ('workflow run source', lambda r: r.run.update(head_sha='d' * 40)),
            ('workflow event', lambda r: r.run.update(event='push')),
            ('environment bypass', lambda r: r.environment.update(can_admins_bypass=True)),
            ('environment branches', lambda r: r.environment.update(deployment_branch_policy={'custom_branch_policies': False})),
            ('reviewer mismatch', lambda r: r.environment['protection_rules'][0]['reviewers'][0]['reviewer'].update(id=61)),
        )
        for label, mutate in mutations:
            with self.subTest(evidence=label):
                remote = Remote()
                mutate(remote)
                reject(remote, context())

    def test_review_and_required_ci_mutations_block_exchange(self):
        for route, value in (
            (f'{PREFIX}/pulls/42/reviews', [{'id': 22, 'state': 'APPROVED', 'user': {'id': 23, 'login': 'reviewer', 'type': 'User'},
                'author_association': 'MEMBER', 'submitted_at': '2026-01-02T00:00:00Z', 'commit_id': 'd' * 40}]),
            (f'{PREFIX}/collaborators/reviewer/permission', {'user': {'id': 23, 'type': 'User'}, 'permission': 'read'}),
            (f'{PREFIX}/actions/runs/101/attempts/1/jobs', {'total_count': 1, 'jobs': []}),
            (f'{PREFIX}/commits/{SHA}/check-runs', {'total_count': 1, 'check_runs': [{'id': 301, 'name': 'Required',
                'head_sha': SHA, 'check_suite': {'id': 201}, 'app': {'id': 1, 'slug': 'other'},
                'started_at': '2026-01-02T00:00:00Z', 'status': 'completed', 'conclusion': 'success'}]}),
        ):
            with self.subTest(route=route):
                remote = Remote()
                remote.override[route] = value
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))


class TransportAndDrift(unittest.TestCase):
    def test_actual_entrypoint_constructs_isolated_transport(self):
        remote = Remote()
        handlers = []
        def opener(*items):
            handlers.extend(items)
            return remote
        with patch.dict(os.environ, context(), clear=True), patch('urllib.request.build_opener', side_effect=opener), \
             patch('ssl.create_default_context', return_value=object()) as tls:
            self.assertEqual(SCOPE['qualification_main'](), 0)
            tls.assert_called_once_with(cafile='/etc/ssl/certs/ca-certificates.crt')
        self.assertTrue(any(isinstance(item, SCOPE['RejectRedirect']) for item in handlers))
        self.assertTrue(any(getattr(item, 'proxies', None) == {} for item in handlers))
        self.assertEqual([call[0] for call in remote.calls if call[1] != 'api.github.com'], ['GET', 'POST', 'DELETE'])

    def test_entrypoint_does_not_print_transport_errors_or_credentials(self):
        class Failing(Remote):
            def open(self, request, timeout):
                raise RuntimeError('private-test-error-token')
        error = io.StringIO()
        with patch.dict(os.environ, context(), clear=True), patch('urllib.request.build_opener', return_value=Failing()), \
             patch('ssl.create_default_context', return_value=object()), contextlib.redirect_stderr(error):
            self.assertEqual(SCOPE['qualification_main'](), 1)
        self.assertEqual(error.getvalue(), 'OIDC qualification failed\n')

    def test_revocation_failure_never_reports_success(self):
        class RevokeFails(Remote):
            def open(self, request, timeout):
                if urllib.parse.urlsplit(request.full_url).hostname == 'crates.io' and request.method == 'DELETE':
                    self.calls.append(('DELETE', 'crates.io', '/revocation'))
                    raise OSError('sensitive network error')
                return super().open(request, timeout)
        remote = RevokeFails()
        error = io.StringIO()
        output = io.StringIO()
        with patch.dict(os.environ, context(), clear=True), patch('urllib.request.build_opener', return_value=remote), \
             patch('ssl.create_default_context', return_value=object()), contextlib.redirect_stderr(error), \
             contextlib.redirect_stdout(output):
            self.assertEqual(SCOPE['qualification_main'](), 1)
        self.assertEqual(output.getvalue(), '')
        self.assertEqual(error.getvalue(), 'OIDC qualification failed\n')
        self.assertTrue(any(method == 'DELETE' and host == 'crates.io' for method, host, _ in remote.calls))

    def test_invalid_json_cannot_request_a_credential(self):
        class Malformed(Remote):
            def __init__(self, payload):
                super().__init__()
                self.payload = payload
            def open(self, request, timeout):
                if urllib.parse.urlsplit(request.full_url).hostname == 'api.github.com':
                    self.calls.append((request.method, 'api.github.com', 'malformed'))
                    return Response(self.payload)
                return super().open(request, timeout)
        for payload in (b'{"id":1,"id":2}', b'{"id":NaN}', b'{"id":Infinity}', b'['):
            with self.subTest(payload=payload):
                remote = Malformed(payload)
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_duplicate_key_in_otherwise_valid_repository_response_is_rejected(self):
        class DuplicateKey(Remote):
            def open(self, request, timeout):
                url = urllib.parse.urlsplit(request.full_url)
                if url.hostname == 'api.github.com' and url.path == '/' + PREFIX:
                    self.calls.append((request.method, url.hostname, url.path))
                    body = json.dumps(self.repo).replace('"id": 12', '"id": 12, "id": 12', 1)
                    return Response(body.encode())
                return super().open(request, timeout)
        remote = DuplicateKey()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_independent_security_evidence_cannot_be_substituted(self):
        baseline = Remote()
        check_route = f'{PREFIX}/commits/{SHA}/check-runs'
        check = baseline.lookup(check_route, {})
        check['check_runs'][0]['app']['id'] = 1
        approval_route = f'{PREFIX}/actions/runs/30/approvals'
        approvals = baseline.lookup(approval_route, {})
        approvals[0]['user']['id'] = 61
        reviews_route = f'{PREFIX}/pulls/42/reviews'
        reviews = baseline.lookup(reviews_route, {})
        other = copy.deepcopy(reviews[0])
        other.update(id=24, state='CHANGES_REQUESTED')
        other['user'] = {'id': 24, 'login': 'other', 'type': 'User'}
        reviews.append(other)
        for route, replacement in ((check_route, check), (approval_route, approvals), (reviews_route, reviews)):
            with self.subTest(route=route):
                remote = Remote()
                remote.override[route] = replacement
                remote.override[f'{PREFIX}/collaborators/other/permission'] = {
                    'user': {'id': 24, 'type': 'User'}, 'permission': 'write'}
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_ambiguous_and_untrusted_review_evidence_denied(self):
        base = Remote()
        review = {'id': 22, 'state': 'APPROVED', 'user': {'id': 23, 'login': 'reviewer', 'type': 'User'},
                  'author_association': 'MEMBER', 'submitted_at': '2026-01-02T00:00:00Z', 'commit_id': HEAD}
        bot = copy.deepcopy(review)
        bot['user']['type'] = 'Bot'
        cases = ((f'{PREFIX}/commits/{SHA}/pulls', [base.pull, base.pull]),
                 (f'{PREFIX}/pulls/42/reviews', [bot]),
                 (f'{PREFIX}/pulls/42/reviews', [dict(review, state='DISMISSED')]),
                 (f'{PREFIX}/pulls/42/reviews', [dict(review, submitted_at=base.pull['merged_at'])]))
        for route, payload in cases:
            with self.subTest(route=route, payload=payload):
                remote = Remote()
                remote.override[route] = payload
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_later_review_resolves_prior_timestamp_tie(self):
        base = Remote()
        review = copy.deepcopy(base.lookup(f'{PREFIX}/pulls/42/reviews', {})[0])
        tied = dict(review, id=24, state='CHANGES_REQUESTED')
        later = dict(review, id=25, submitted_at='2026-01-02T01:00:00Z')
        unresolved = Remote()
        unresolved.override[f'{PREFIX}/pulls/42/reviews'] = [review, tied]
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), unresolved, unresolved)
        self.assertFalse(any(call[1] != 'api.github.com' for call in unresolved.calls))
        resolved = Remote()
        resolved.override[f'{PREFIX}/pulls/42/reviews'] = [review, tied, later]
        SCOPE['run_qualification'](context(), resolved, resolved)
        self.assertTrue(any(call[1] == 'crates.io' for call in resolved.calls))

    def test_mutable_pull_and_reviews_are_reobserved_before_exchange(self):
        class Drifting(Remote):
            def __init__(self, target):
                super().__init__()
                self.target = target
                self.count = 0
            def lookup(self, route, query):
                result = copy.deepcopy(super().lookup(route, query))
                if route == self.target:
                    self.count += 1
                    if self.count >= 2:
                        if isinstance(result, list):
                            return []
                        result['state'] = 'open'
                return result
        for target in (f'{PREFIX}/pulls/42', f'{PREFIX}/pulls/42/reviews'):
            with self.subTest(target=target):
                remote = Drifting(target)
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_wrong_oidc_job_or_environment_never_reaches_registry(self):
        for key, value in (('environment', 'other'), ('check_run_id', '41'), ('run_id', '31'),
                           ('workflow_sha', 'd' * 40), ('runner_environment', 'self-hosted'),
                           ('iss', 'https://other.example'), ('aud', 'other'), ('repository', 'other/project'),
                           ('check_run_id', True), ('check_run_id', None), ('repository_id', '13'),
                           ('repository_owner_id', '14'), ('run_attempt', '2'), ('event_name', 'push'),
                           ('ref', 'refs/heads/other'), ('sha', 'd' * 40),
                           ('workflow_ref', 'example/project/.github/workflows/other.yml@refs/heads/main')):
            with self.subTest(claim=key):
                remote = Remote()
                remote.claims[key] = value
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] == 'crates.io' for call in remote.calls))

    def test_unbound_job_never_reaches_registry(self):
        good = {'id': 40, 'run_id': 30, 'run_attempt': 1, 'head_sha': SHA,
                'name': 'OIDC qualification', 'status': 'in_progress'}
        for field, value in (('id', 41), ('run_id', 31), ('run_attempt', 2), ('head_sha', 'd' * 40),
                             ('name', 'Other job'), ('status', 'completed')):
            with self.subTest(field=field):
                remote = Remote()
                remote.override[f'{PREFIX}/actions/jobs/40'] = dict(good, **{field: value})
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] == 'crates.io' for call in remote.calls))

    def test_job_change_before_registry_exchange_is_rejected(self):
        class JobChanged(Remote):
            def __init__(self):
                super().__init__()
                self.job_reads = 0
            def lookup(self, route, query):
                result = super().lookup(route, query)
                if route == f'{PREFIX}/actions/jobs/40':
                    self.job_reads += 1
                    if self.job_reads > 1:
                        return dict(result, status='completed')
                return result
        remote = JobChanged()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] == 'crates.io' for call in remote.calls))

    def test_policy_change_after_identity_request_never_reaches_registry(self):
        class ChangedAfterIdentity(Remote):
            def open(self, request, timeout):
                response = super().open(request, timeout)
                if urllib.parse.urlsplit(request.full_url).hostname == 'runner.actions.githubusercontent.com':
                    self.environment['can_admins_bypass'] = True
                return response
        remote = ChangedAfterIdentity()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] == 'crates.io' for call in remote.calls))

    def test_replaced_valid_review_or_permission_is_not_admitted(self):
        class Changed(Remote):
            def __init__(self, target):
                super().__init__()
                self.target = target
                self.count = 0
            def lookup(self, route, query):
                result = copy.deepcopy(super().lookup(route, query))
                if route == self.target:
                    self.count += 1
                    if self.count >= 2:
                        if isinstance(result, list):
                            result[0]['id'] = 25
                        else:
                            result['permission'] = 'admin'
                return result
        for target in (f'{PREFIX}/pulls/42/reviews', f'{PREFIX}/collaborators/reviewer/permission'):
            with self.subTest(target=target):
                remote = Changed(target)
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_valid_but_changed_environment_evidence_after_identity_is_rejected(self):
        class ChangedAfterIdentity(Remote):
            def open(self, request, timeout):
                response = super().open(request, timeout)
                if urllib.parse.urlsplit(request.full_url).hostname == 'runner.actions.githubusercontent.com':
                    self.environment['revision'] = 'changed'
                return response
        remote = ChangedAfterIdentity()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] == 'crates.io' for call in remote.calls))

    def test_ambiguous_workflow_or_environment_approval_is_rejected(self):
        base = Remote()
        approval = base.lookup(f'{PREFIX}/actions/runs/30/approvals', {})[0]
        suite = base.lookup(f'{PREFIX}/actions/runs', {'head_sha': [SHA]})['workflow_runs'][0]
        cases = ((f'{PREFIX}/actions/runs/30/approvals', [approval, approval]),
                 (f'{PREFIX}/actions/runs', {'total_count': 2, 'workflow_runs': [suite, suite]}))
        for route, replacement in cases:
            with self.subTest(route=route):
                remote = Remote()
                remote.override[route] = replacement
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_duplicate_selected_suite_in_other_workflow_is_rejected(self):
        class DuplicateSuite(Remote):
            def lookup(self, route, query):
                result = super().lookup(route, query)
                if route == f'{PREFIX}/actions/runs':
                    first = result['workflow_runs'][0]
                    second = dict(first, id=first['id'] + 1, path='.github/workflows/other.yml')
                    return {'total_count': 2, 'workflow_runs': [first, second]}
                return result
        remote = DuplicateSuite()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_reviewer_identity_cannot_change_user_to_bot(self):
        class ChangedIdentity(Remote):
            def __init__(self):
                super().__init__()
                self.permission_reads = 0
            def lookup(self, route, query):
                if route == f'{PREFIX}/pulls/42/reviews':
                    first = super().lookup(route, query)[0]
                    second = copy.deepcopy(first)
                    second.update(id=24, submitted_at='2026-01-02T01:00:00Z')
                    second['user']['type'] = 'Bot'
                    third = copy.deepcopy(first)
                    third.update(id=25, submitted_at='2026-01-02T02:00:00Z')
                    return [first, second, third]
                if route == f'{PREFIX}/collaborators/reviewer/permission':
                    self.permission_reads += 1
                    kind = 'Bot' if self.permission_reads % 3 == 2 else 'User'
                    return {'user': {'id': 23, 'type': kind}, 'permission': 'write'}
                return super().lookup(route, query)
        remote = ChangedIdentity()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_newer_unsuccessful_or_pending_required_workflow_is_rejected(self):
        class Newer(Remote):
            def lookup(self, route, query):
                result = super().lookup(route, query)
                if route == f'{PREFIX}/actions/runs':
                    old = result['workflow_runs'][0]
                    new = dict(old, id=old['id'] + 10, run_number=2, check_suite_id=old['check_suite_id'] + 10,
                               status='in_progress', conclusion=None)
                    return {'total_count': 2, 'workflow_runs': [old, new]}
                return result
        remote = Newer()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_pagination_and_boolean_ids_fail_closed(self):
        cases = ((f'{PREFIX}/actions/runs', {'total_count': 2, 'workflow_runs': []}),
                 (f'{PREFIX}/commits/{SHA}/check-runs', {'total_count': 2, 'check_runs': []}),
                 (f'{PREFIX}/actions/runs/30', dict(Remote().run, id=True)))
        for route, replacement in cases:
            with self.subTest(route=route):
                remote = Remote()
                remote.override[route] = replacement
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](context(), remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_incomplete_pagination_cannot_hide_extra_workflow_runs(self):
        class MissingPage(Remote):
            def lookup(self, route, query):
                result = super().lookup(route, query)
                if route == f'{PREFIX}/actions/runs':
                    result['total_count'] = 2
                    if query.get('page') == ['2']:
                        result['workflow_runs'] = []
                return result
        remote = MissingPage()
        with self.assertRaises(SCOPE['AdmissionError']):
            SCOPE['run_qualification'](context(), remote, remote)
        self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_untrusted_oidc_endpoint_is_rejected_without_request(self):
        for value in ('http://runner.actions.githubusercontent.com/id',
                      'https://attacker.example/id',
                      'https://user@runner.actions.githubusercontent.com/id',
                      'https://runner.actions.githubusercontent.com/id?audience=other',
                      'https://runner.actions.githubusercontent.com/id#fragment'):
            with self.subTest(url=value):
                env = context()
                env['ACTIONS_ID_TOKEN_REQUEST_URL'] = value
                remote = Remote()
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](env, remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))

    def test_input_grammar_rejects_untrusted_names(self):
        for key, value in (('GITHUB_REPOSITORY', '../project'), ('GITHUB_REPOSITORY', 'owner/repo.git'),
                           ('VELNOR_QUALIFICATION_DEFAULT_BRANCH', 'main/../evil'),
                           ('VELNOR_QUALIFICATION_DEFAULT_BRANCH', 'HEAD'),
                           ('VELNOR_QUALIFICATION_REVIEWER_ID', '٠١')):
            with self.subTest(key=key):
                env = context()
                env[key] = value
                remote = Remote()
                with self.assertRaises(SCOPE['AdmissionError']):
                    SCOPE['run_qualification'](env, remote, remote)
                self.assertFalse(any(call[1] != 'api.github.com' for call in remote.calls))
        self.assertFalse(SCOPE['valid_branch']('main/../evil'))
        self.assertFalse(SCOPE['utc']('٢٠٢٦-٠١-٠٢T٠٠:٠٠:٠٠Z'))


if __name__ == '__main__':
    unittest.main()
