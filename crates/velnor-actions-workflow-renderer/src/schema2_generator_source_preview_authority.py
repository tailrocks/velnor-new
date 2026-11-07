# SPDX-FileCopyrightText: 2026 Alexey Zhokhov
# SPDX-License-Identifier: Apache-2.0

def preview_authority():
    repository = os.environ["AUTHORITY_REPOSITORY"]
    path = os.environ["AUTHORITY_PATH"]
    ref = os.environ["AUTHORITY_REF"]
    sha = os.environ["AUTHORITY_SHA"]
    if (repository != "tailrocks/velnor-new" or path != ".github/workflows/source-preview-publication.yml"
            or re.fullmatch("[0-9a-f]{40}", sha) is None
            or ref != repository + "/" + path + "@" + sha):
        raise ValueError("source preview workflow authority")
    return {"repository": repository, "path": path, "ref": ref, "sha": sha}
