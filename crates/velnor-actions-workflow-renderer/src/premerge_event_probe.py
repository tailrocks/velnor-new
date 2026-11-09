# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

import json
import os
import pathlib
import re
import ssl
import stat
import sys
import urllib.parse
import urllib.request

MAX_JSON = 2 * 1024 * 1024


def require(value):
    if not value:
        raise ValueError('premerge event proof rejected')


def unique(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result)
        if key in ('id', 'number', 'run_id', 'run_attempt', 'total_count'):
            require(value is None or (type(value) is int and value >= 0))
        result[key] = value
    return result


def reject_constant(_value):
    raise ValueError('invalid JSON constant')


def decode(data):
    require(len(data) <= MAX_JSON)
    return json.loads(data, object_pairs_hook=unique, parse_constant=reject_constant)


def field(value, *keys):
    for key in keys:
        if not isinstance(value, dict):
            return None
        value = value.get(key)
    return value


def sha(value):
    return isinstance(value, str) and re.fullmatch('[0-9a-f]{40}', value) is not None


def number(value):
    require(isinstance(value, str) and re.fullmatch('[1-9][0-9]{0,19}', value) is not None)
    return int(value)


def context(env):
    repository = env['VELNOR_PROOF_REPOSITORY']
    owner = repository.split('/')[0]
    branch = env['VELNOR_PROOF_BASE_BRANCH']
    workflow = env['VELNOR_PROOF_CALLER_WORKFLOW']
    require(re.fullmatch('[A-Za-z0-9-]+/[A-Za-z0-9_.-]+', repository) is not None)
    require(env['GITHUB_REPOSITORY'] == repository and env['GITHUB_EVENT_NAME'] == 'pull_request')
    match = re.fullmatch('refs/pull/([1-9][0-9]*)/merge', env['GITHUB_REF'])
    require(match is not None and sha(env['GITHUB_SHA']) and sha(env['GITHUB_WORKFLOW_SHA']))
    require(env['GITHUB_WORKFLOW_REF'] == repository + '/' + workflow + '@' + env['GITHUB_REF'])
    signer_sha = env['VELNOR_PROOF_SIGNER_SHA']
    require(sha(signer_sha) and env['VELNOR_PROOF_SIGNER_REPOSITORY'] == repository
            and env['VELNOR_PROOF_SIGNER_PATH'] == '.github/workflows/premerge-event-proof.yml'
            and env['VELNOR_PROOF_SIGNER_REF'] == repository + '/.github/workflows/premerge-event-proof.yml@' + signer_sha)
    attempt = number(env['GITHUB_RUN_ATTEMPT'])
    require(attempt == 1)
    return {'repository': repository, 'owner': owner, 'repository_id': number(env['VELNOR_PROOF_REPOSITORY_ID']),
            'owner_id': number(env['VELNOR_PROOF_OWNER_ID']), 'base_ref': branch,
            'pull_request': int(match[1]), 'merge_sha': env['GITHUB_SHA'],
            'caller_workflow': workflow, 'caller_ref': env['GITHUB_REF'], 'caller_sha': env['GITHUB_WORKFLOW_SHA'],
            'signer_sha': signer_sha, 'signer_ref': env['VELNOR_PROOF_SIGNER_REF'],
            'run_id': number(env['GITHUB_RUN_ID']), 'run_attempt': attempt,
            'job_id': number(env['VELNOR_PROOF_JOB_ID']), 'environment': env['VELNOR_PROOF_ENVIRONMENT'],
            'reviewer_id': number(env['VELNOR_PROOF_REVIEWER_ID'])}


def identity(repository, expected):
    require(field(repository, 'id') == expected['repository_id']
            and type(field(repository, 'id')) is int
            and field(repository, 'full_name') == expected['repository']
            and field(repository, 'owner', 'id') == expected['owner_id']
            and type(field(repository, 'owner', 'id')) is int
            and field(repository, 'owner', 'login') == expected['owner'])


def parse_event(data, expected):
    event = decode(data)
    identity(field(event, 'repository'), expected)
    require(field(event, 'number') == expected['pull_request'] and type(field(event, 'number')) is int)
    pull = field(event, 'pull_request')
    require(field(pull, 'number') == expected['pull_request'] and type(field(pull, 'number')) is int
            and field(pull, 'state') == 'open' and field(pull, 'draft') is False
            and field(pull, 'merged') is False and field(pull, 'merge_commit_sha') == expected['merge_sha'])
    for side in ('head', 'base'):
        identity(field(pull, side, 'repo'), expected)
        require(sha(field(pull, side, 'sha')))
    require(field(pull, 'base', 'ref') == expected['base_ref'])
    head_ref = field(pull, 'head', 'ref')
    require(isinstance(head_ref, str) and 0 < len(head_ref) <= 255)
    return {'head_sha': field(pull, 'head', 'sha'), 'head_ref': head_ref,
            'base_sha': field(pull, 'base', 'sha'), 'base_ref': expected['base_ref'],
            'merge_sha': expected['merge_sha']}


class RejectRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, _request, _response, _code, _message, _headers, _url):
        return None


class Github:
    def __init__(self, token, opener):
        require(isinstance(token, str) and token and len(token) <= 65536 and '\r' not in token and '\n' not in token)
        self.token = token
        self.opener = opener

    def get(self, route):
        require(isinstance(route, str) and route.startswith('repos/') and '\r' not in route and '\n' not in route)
        request = urllib.request.Request('https://api.github.com/' + route, method='GET', headers={
            'Authorization': 'Bearer ' + self.token, 'Accept': 'application/vnd.github+json',
            'X-GitHub-Api-Version': '2022-11-28', 'User-Agent': 'velnor-premerge-event-proof'})
        with self.opener.open(request, timeout=15) as response:
            require(response.status == 200)
            return decode(response.read(MAX_JSON + 1))


def observe(api, expected, event):
    prefix = 'repos/' + expected['repository']
    repository = api.get(prefix)
    identity(repository, expected)
    require(field(repository, 'default_branch') == expected['base_ref'])
    pull = api.get(prefix + '/pulls/' + str(expected['pull_request']))
    current = parse_event(json.dumps({'repository': repository, 'number': expected['pull_request'], 'pull_request': pull}).encode(), expected)
    require(current == event)
    commit = api.get(prefix + '/git/commits/' + expected['merge_sha'])
    require(field(commit, 'sha') == expected['merge_sha'] and sha(field(commit, 'tree', 'sha')))
    parents = field(commit, 'parents')
    require(isinstance(parents, list) and [field(parent, 'sha') for parent in parents] == [event['base_sha'], event['head_sha']])
    run = api.get(prefix + '/actions/runs/' + str(expected['run_id']))
    for side in ('repository', 'head_repository'):
        identity(field(run, side), expected)
    require(field(run, 'id') == expected['run_id'] and field(run, 'run_attempt') == expected['run_attempt']
            and field(run, 'event') == 'pull_request' and field(run, 'head_sha') == event['head_sha']
            and field(run, 'head_branch') == event['head_ref'] and field(run, 'path') == expected['caller_workflow'])
    job = api.get(prefix + '/actions/jobs/' + str(expected['job_id']))
    require(field(job, 'id') == expected['job_id'] and field(job, 'run_id') == expected['run_id']
            and field(job, 'run_attempt') == expected['run_attempt'] and field(job, 'status') == 'in_progress')
    caller = api.get(prefix + '/git/commits/' + expected['caller_sha'])
    require(field(caller, 'sha') == expected['caller_sha'])
    return field(commit, 'tree', 'sha')


def approval(api, expected):
    prefix = 'repos/' + expected['repository']
    path = prefix + '/environments/' + urllib.parse.quote(expected['environment'], safe='')
    environment = api.get(path)
    require(type(field(environment, 'id')) is int and field(environment, 'id') > 0)
    require(field(environment, 'name') == expected['environment'] and field(environment, 'can_admins_bypass') is False
            and field(environment, 'deployment_branch_policy', 'custom_branch_policies') is True)
    rules = field(environment, 'protection_rules')
    require(isinstance(rules, list))
    reviewers = [item for item in rules if field(item, 'type') == 'required_reviewers']
    require(len(reviewers) == 1)
    allowed = field(reviewers[0], 'reviewers')
    require(isinstance(allowed, list) and len(allowed) == 1 and field(allowed[0], 'type') == 'User'
            and field(allowed[0], 'reviewer', 'id') == expected['reviewer_id'])
    branches = api.get(path + '/deployment-branch-policies?per_page=100')
    policies = field(branches, 'branch_policies')
    require(field(branches, 'total_count') == 1 and isinstance(policies, list) and len(policies) == 1
            and field(policies[0], 'type') == 'branch' and field(policies[0], 'name') == 'refs/pull/*/merge')
    approvals = api.get(prefix + '/actions/runs/' + str(expected['run_id']) + '/approvals')
    require(isinstance(approvals, list))
    relevant = [item for item in approvals if any(field(target, 'id') == field(environment, 'id')
        for target in (field(item, 'environments') or []))]
    require(len(relevant) == 1 and field(relevant[0], 'state') == 'approved'
            and field(relevant[0], 'user', 'type') == 'User' and field(relevant[0], 'user', 'id') == expected['reviewer_id'])
    return (environment, branches, relevant)


def receipt(api, expected, event):
    first_tree = observe(api, expected, event)
    first_approval = approval(api, expected)
    require(observe(api, expected, event) == first_tree and approval(api, expected) == first_approval)
    return {'schema': 1, 'scenario': 'premerge-event', 'repository': expected['repository'],
            'repository_id': expected['repository_id'], 'owner_id': expected['owner_id'],
            'pull_request': expected['pull_request'], **event, 'merge_tree_sha': first_tree,
            'caller_workflow': expected['caller_workflow'], 'caller_ref': expected['caller_ref'], 'caller_sha': expected['caller_sha'],
            'signer_ref': expected['signer_ref'], 'signer_sha': expected['signer_sha'],
            'run_id': expected['run_id'], 'run_attempt': expected['run_attempt'], 'job_id': expected['job_id'],
            'environment': expected['environment'], 'reviewer_id': expected['reviewer_id']}


def read_event(path):
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    try:
        item = os.fstat(fd)
        require(stat.S_ISREG(item.st_mode) and item.st_uid == os.getuid() and not item.st_mode & 0o022 and item.st_size <= MAX_JSON)
        with os.fdopen(fd, 'rb', closefd=False) as stream:
            return stream.read(MAX_JSON + 1)
    finally:
        os.close(fd)


def main():
    try:
        require('SSLKEYLOGFILE' not in os.environ)
        expected = context(os.environ)
        event = parse_event(read_event(os.environ['GITHUB_EVENT_PATH']), expected)
        tls = ssl.create_default_context(cafile='/etc/ssl/certs/ca-certificates.crt')
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), RejectRedirect(), urllib.request.HTTPSHandler(context=tls))
        result = receipt(Github(os.environ['GIT_TOKEN'], opener), expected, event)
        data = json.dumps(result, sort_keys=True, separators=(',', ':')).encode() + b'\n'
        require(len(data) <= 16384)
        path = pathlib.Path(os.environ['VELNOR_PREMERGE_SOURCE'])
        require(path.is_absolute() and path.resolve(strict=True) == path)
        directory = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW)
        try:
            item = os.fstat(directory)
            require(item.st_uid == os.getuid() and stat.S_IMODE(item.st_mode) == 0o700)
            fd = os.open('receipt.json', os.O_WRONLY | os.O_CREAT | os.O_EXCL | os.O_NOFOLLOW, 0o600, dir_fd=directory)
            with os.fdopen(fd, 'wb') as output:
                output.write(data)
                output.flush()
                os.fsync(output.fileno())
        finally:
            os.close(directory)
    except Exception:
        print('premerge event proof failed', file=sys.stderr)
        return 1
    return 0
