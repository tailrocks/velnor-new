# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0


def index_path(package):
    name = package.lower()
    if len(name) == 1:
        return f"/1/{name}"
    if len(name) == 2:
        return f"/2/{name}"
    if len(name) == 3:
        return f"/3/{name[0]}/{name}"
    return f"/{name[:2]}/{name[2:4]}/{name}"


def approved_baseline(index, package, version, checksum):
    entries = [entry for entry in index if entry.get("name") == package and entry.get("vers") == version]
    if len(entries) != 1 or entries[0].get("cksum") != checksum or entries[0].get("yanked") is not False:
        raise ValueError("baseline is missing, ambiguous, changed, or yanked")
