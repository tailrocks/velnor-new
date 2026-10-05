//! Hermetic fake Mise and GitHub CLI commands for publisher tests.

pub(super) const FAKE_MISE: &str = r##"#!/usr/bin/env python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ["GH_FAKE_MISE_CALLS"], "a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\n")
if len(args) < 4 or args[3] not in ("install", "exec"):
    raise SystemExit(97)
if args[3] == "install":
    if any(os.environ.get(name) for name in ("GH_TOKEN", "GITHUB_TOKEN", "MISE_GITHUB_TOKEN")):
        raise SystemExit(99)
    raise SystemExit(0)
if args[4:6] != ["gh@2.102.0", "--"]:
    raise SystemExit(98)
if not os.environ.get("GH_TOKEN"):
    raise SystemExit(89)
os.environ["GH_FAKE_MISE_EXECUTED"] = "1"
os.execvpe(args[6], args[6:], os.environ)
"##;

pub(super) const FAKE_GH: &str = r##"#!/usr/bin/env python3
import json, os, shutil, sys
from pathlib import Path
args = sys.argv[1:]
if os.environ.get("GH_FAKE_MISE_EXECUTED") != "1":
    raise SystemExit(88)
with open(os.environ["GH_FAKE_CALLS"], "a", encoding="utf-8") as log:
    log.write(json.dumps(args) + "\n")
if args[0] == "api":
    endpoint_index = args.index("api") + 1
    endpoint = args[endpoint_index]
    if endpoint == "repos/tailrocks/velnor-new/commits/main":
        result = {"sha": os.environ["GH_FAKE_MAIN_SHA"]}
    elif endpoint == "repos/tailrocks/velnor-new/git/refs" and args[args.index("--method") + 1] == "POST":
        fields = [args[index + 1] for index, value in enumerate(args) if value == "-f"]
        values = dict(value.split("=", 1) for value in fields)
        if values.get("sha") != os.environ["GITHUB_SHA"]:
            raise SystemExit(86)
        reference = values.get("ref", "")
        tag = reference.rsplit("/", 1)[-1]
        Path(os.environ["GH_FAKE_TAG_PATH"]).write_text(tag, encoding="utf-8")
        result = {"ref": reference, "object": {"type": "commit", "sha": values["sha"]}}
    elif "/git/ref/tags/" in endpoint:
        tag = endpoint.rsplit("/", 1)[1]
        tag_path = Path(os.environ["GH_FAKE_TAG_PATH"])
        if not tag_path.exists() or tag_path.read_text(encoding="utf-8") != tag:
            raise SystemExit(95)
        result = {"ref": f"refs/tags/{tag}", "object": {"type": "commit", "sha": os.environ["GH_FAKE_TAG_SHA"]}}
    elif "/releases/tags/" in endpoint:
        raise SystemExit(96)
    elif endpoint == "repos/tailrocks/velnor-new/releases/741852963":
        if not Path(os.environ["GH_FAKE_RELEASE_CREATED"]).exists():
            raise SystemExit(96)
        if Path(os.environ["GH_FAKE_ACCEPTED_DIRECTORY"]).exists():
            raise SystemExit(83)
        index_file = Path(os.environ["GH_FAKE_RELEASE_INDEX"])
        index = int(index_file.read_text()) if index_file.exists() else 0
        index_file.write_text(str(index + 1))
        response = Path(os.environ["GH_FAKE_API_DIR"]) / f"release-{index}.json"
        result = json.loads(response.read_text(encoding="utf-8"))
    else:
        raise SystemExit(96)
    print(json.dumps(result, separators=(",", ":")))
elif args[0] == "release" and args[1] == "upload":
    if Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).exists():
        raise SystemExit(81)
    if Path(os.environ["GH_FAKE_ACCEPTED_DIRECTORY"]).exists():
        raise SystemExit(83)
    if args[args.index("--repo") + 1] != "tailrocks/velnor-new":
        raise SystemExit(90)
    uploaded = []
    index = 3
    while index < len(args):
        value = args[index]
        if value == "--repo":
            break
        path = Path(value)
        if not path.is_file():
            raise SystemExit(95)
        if path.name == "velnor-actions-release-manifest.json":
            shutil.copyfile(path, os.environ["GH_FAKE_MANIFEST_COPY"])
        if path.name == "velnor-actions-release-manifest.json.sha256":
            shutil.copyfile(path, os.environ["GH_FAKE_MANIFEST_CHECKSUM_COPY"])
        uploaded.append(path.name)
        index += 1
    with open(os.environ["GH_FAKE_UPLOADS"], "a", encoding="utf-8") as log:
        log.write("draft-upload:" + ",".join(uploaded) + "\n")
elif args[0] == "release" and args[1] in ("create", "edit"):
    if args[1] == "create":
        if args[args.index("--repo") + 1] != "tailrocks/velnor-new":
            raise SystemExit(90)
        target = args[args.index("--target") + 1]
        if target != os.environ["GITHUB_SHA"] or args[2] != "generator-" + target:
            raise SystemExit(93)
        if "--draft" not in args or "--latest=false" not in args:
            raise SystemExit(92)
        tag_path = Path(os.environ["GH_FAKE_TAG_PATH"])
        if not tag_path.exists() or tag_path.read_text(encoding="utf-8") != args[2]:
            raise SystemExit(87)
        Path(os.environ["GH_FAKE_RELEASE_CREATED"]).write_text(args[2], encoding="utf-8")
    else:
        if args[args.index("--repo") + 1] != "tailrocks/velnor-new":
            raise SystemExit(90)
        if "--draft=false" not in args:
            raise SystemExit(91)
        if Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).exists():
            raise SystemExit(82)
        if Path(os.environ["GH_FAKE_ACCEPTED_DIRECTORY"]).exists():
            raise SystemExit(83)
        Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).write_text("true", encoding="utf-8")
        with open(os.environ["GH_FAKE_UPLOADS"], "a", encoding="utf-8") as log:
            log.write("publish\n")
elif args[0] == "release" and args[1] == "view":
    tag = args[2]
    created = Path(os.environ["GH_FAKE_RELEASE_CREATED"])
    if not created.exists() or created.read_text(encoding="utf-8") != tag:
        raise SystemExit(84)
    print(json.dumps({"databaseId": 741852963, "tagName": tag, "isDraft": not Path(os.environ["GH_FAKE_RELEASE_PUBLISHED"]).exists()}))
else:
    raise SystemExit(94)
"##;
