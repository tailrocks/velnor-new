# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Runs the legacy source preview "Admit" script (the four renderer sources the engine joins
# into the job) against a recorded GitHub, for the parity test of velnor-ci admit-preview.
# Usage:
#   python3 -I -S admit.py RENDERER_SRC_DIR CASES_JSON
# CASES_JSON is {"cases": [{"api", "zips", "env", "owner_login"}]}; each case runs with exactly its
# environment, in one interpreter. Prints one JSON line, a list with one
# {"ok": bool, "calls": [GET paths of api.github.com]} per case.
import copy
import http.client
import json
import os
import re
import signal
import ssl
import sys

renderer, cases_path = sys.argv[1:3]
world = {}
calls = []
ARTIFACT = re.compile(r"^/repos/tailrocks/velnor-new/actions/artifacts/([0-9]+)/zip$")


class Response:
    def __init__(self, status, body, location=None):
        self.status, self.data, self.location, self.at = status, body, location, 0

    def read(self, limit=None):
        end = len(self.data) if limit is None else self.at + limit
        chunk = self.data[self.at:end]
        self.at += len(chunk)
        return chunk

    def getheader(self, name, default=None):
        return self.location if name == "Location" and self.location else default


def answer(path):
    value = world["api"].get(path)
    if value is None:
        return None
    if isinstance(value, dict) and "__seq" in value:
        items = value["__seq"]
        value = items.pop(0) if len(items) > 1 else items[0]
    return copy.deepcopy(value)


class Connection:
    sock = None

    def __init__(self, host, timeout=None, context=None):
        self.host = host

    def request(self, method, path, body=None, headers=None):
        assert method == "GET"
        self.path, self.headers = path, headers or {}

    def getresponse(self):
        if self.host != "api.github.com":
            assert "Authorization" not in self.headers, "the token went to a blob host"
            return Response(200, bytes.fromhex(world["zips"][self.path.split("/")[2].split("?")[0]]))
        assert "Authorization" in self.headers
        calls.append(self.path)
        zip_id = ARTIFACT.match(self.path)
        if zip_id and zip_id.group(1) in world["zips"]:
            return Response(302, b"", "https://productionresultssa1.blob.core.windows.net/zip/" + zip_id.group(1) + "?sig=x")
        value = answer(self.path)
        if value is None:
            return Response(404, b"{}")
        return Response(200, json.dumps(value).encode())

    def close(self):
        pass


http.client.HTTPSConnection = Connection
# Loading the CA store for every request would dominate the run; no connection is made.
ssl.create_default_context = lambda *args, **kwargs: None
names = ("artifact", "tag", "authority", "source")
text = "\n".join(open(renderer + "/schema2_generator_source_preview_" + name + ".py").read() for name in names)
results = []
for case in json.load(open(cases_path))["cases"]:
    world.clear()
    world.update(case)
    calls.clear()
    os.environ.clear()
    os.environ.update(case["env"])
    # The legacy script pins the owner's login twice: `OWNER_LOGIN = "..."` in the tag source,
    # and `path == "/users/..."` in the request allowlist of the source source. The case names
    # the account the deployment is pinned to; the script is otherwise run as written.
    pinned = re.search(r'^OWNER_LOGIN = "(.*)"$', text, flags=re.M).group(1)
    assert text.count('path == "/users/' + pinned + '"') == 1
    owner = case["owner_login"]
    script = text.replace('OWNER_LOGIN = "' + pinned + '"', "OWNER_LOGIN = " + json.dumps(owner))
    script = script.replace('path == "/users/' + pinned + '"', 'path == "/users/' + owner + '"')
    namespace = {"__name__": "legacy_admit"}
    exec(compile(script, "admit", "exec"), namespace)
    ok = False
    try:
        namespace["main"]()
        ok = True
    except Exception as error:
        print(repr(error), file=sys.stderr)
    signal.setitimer(signal.ITIMER_REAL, 0)
    results.append({"ok": ok, "calls": list(calls)})
print(json.dumps(results))
