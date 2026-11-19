# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

# Runs the legacy source preview publication script (authority and publication parts)
# against a recorded GitHub, for the parity test of velnor-ci run-authority. Usage, in the
# directory the artifacts were downloaded to, with the step's environment set:
#   python3 -I -S run_authority.py RENDERER_SRC_DIR WORLD_JSON
# Prints one JSON line {"ok": bool, "writes": [...], "release": the release it created}.
import json
import os
import sys
import copy
import http.client

renderer, world_path = sys.argv[1:3]
world = json.load(open(world_path))
writes = []
REPO = "/repos/tailrocks/velnor-new"


class Response:
    def __init__(self, status, value):
        self.status = status
        self.data = json.dumps(value).encode()

    def read(self, limit=None):
        return self.data if limit is None else self.data[:limit]

    def getheader(self, name, default=None):
        return default


class Connection:
    def __init__(self, host, timeout=None, context=None):
        self.host = host

    def request(self, method, path, body=None, headers=None):
        self.method, self.path, self.body = method, path, body

    def getresponse(self):
        api = world["api"]
        if self.method == "GET":
            if self.path in api:
                return Response(200, copy.deepcopy(api[self.path]))
            return Response(404, {})
        writes.append([self.method, self.path[len(REPO):]])
        if self.method == "POST" and self.path == REPO + "/releases":
            sent = json.loads(self.body)
            item = {"id": 9000, "tag_name": sent["tag_name"], "body": sent["body"], "draft": True, "prerelease": True, "assets": []}
            api[REPO + "/releases/9000"] = item
            return Response(201, item)
        return Response(500, {})

    def close(self):
        pass


http.client.HTTPSConnection = Connection
text = open(renderer + "/schema2_generator_source_preview_authority.py").read() + "\n" + open(renderer + "/schema2_generator_source_preview_publication.py").read()
namespace = {"__name__": "legacy_publication"}
exec(compile(text, "publication", "exec"), namespace)
ok = False
try:
    namespace["main"]()
    ok = True
except (OSError, ValueError, KeyError, TypeError, TimeoutError, http.client.HTTPException):
    pass
print(json.dumps({"ok": ok, "writes": writes, "release": world["api"].get(REPO + "/releases/9000")}))
