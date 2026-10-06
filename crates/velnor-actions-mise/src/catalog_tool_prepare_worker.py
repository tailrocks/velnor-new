"""Closed requested-tool configuration, parsed as data before installation."""
import hashlib
import os
import pathlib
import re
import stat
import subprocess
import sys

FLAGS = ["--no-config", "--no-env", "--no-hooks"]


def relative(value, empty=False):
    if empty and value == "":
        return value
    parts = value.split("/")
    if not value or any(part in ("", ".", "..") for part in parts):
        raise ValueError("invalid owned relative path")
    return value


def digest(value):
    if not re.fullmatch("[0-9a-f]{64}", value):
        raise ValueError("invalid launch digest")
    return value


def configuration(text, selectors):
    if len(text.encode()) > 32768 or "\r" in text or "\0" in text:
        raise ValueError("invalid configuration size or encoding")
    rows = text.split("\n")
    if rows.pop(0) != "velnor-tool-prepare-v1":
        raise ValueError("unknown configuration schema")
    tools = {}
    order = []
    for row in rows:
        fields = row.split("\t")
        kind, selector = fields[:2]
        if kind == "tool" and len(fields) == 4:
            if selector in tools or not re.fullmatch("[a-z][a-z0-9-]*", fields[2]):
                raise ValueError("duplicate tool or invalid binary")
            if not fields[3] or len(fields[3]) > 256:
                raise ValueError("invalid version")
            tools[selector] = {"binary": fields[2], "version": fields[3],
                               "launch": [], "environment": {}, "plan": None}
            order.append(selector)
            continue
        tool = tools.get(selector)
        if tool is None:
            raise ValueError("configuration does not follow selected tool")
        if kind == "plan" and len(fields) == 7 and tool["plan"] is None:
            url, sha, strip, binary_path, root = fields[2:]
            if not url.startswith("https://") or '"' in url or strip not in ("0", "1"):
                raise ValueError("invalid qualified installation")
            digest(sha)
            relative(binary_path, empty=True)
            relative(root)
            match = re.fullmatch(r'http:([a-z][a-z0-9-]*)\[.*\]@([0-9]+(?:\.[0-9]+)+)', selector)
            if match is None:
                raise ValueError("invalid native selector")
            options = f'url="{url}",checksum="sha256:{sha}",strip_components={strip}'
            if binary_path:
                options += f',bin_path="{binary_path}"'
            if selector != f'http:{match[1]}[{options}]@{match[2]}':
                raise ValueError("selector does not bind qualified acquisition")
            tool["plan"] = root
        elif kind == "launch" and len(fields) == 4:
            path = relative(fields[2])
            if any(existing[0] == path for existing in tool["launch"]):
                raise ValueError("duplicate launch path")
            tool["launch"].append((path, digest(fields[3])))
        elif kind == "environment" and len(fields) == 4:
            name, path = fields[2:]
            if name not in ("PATH", "JAVA_HOME", "GRAALVM_HOME", "PYTHONHOME") or name in tool["environment"]:
                raise ValueError("invalid installation environment")
            tool["environment"][name] = relative(path, empty=True)
        else:
            raise ValueError("unknown or duplicate configuration field")
    if order != selectors or not selectors or sorted(set(selectors)) != selectors:
        raise ValueError("selected configuration differs from invocation")
    for selector, tool in tools.items():
        native = selector.startswith("http:")
        if native != (tool["plan"] is not None) or native != bool(tool["launch"]):
            raise ValueError("missing native installation closure")
        if not native and tool["environment"]:
            raise ValueError("foreign generic installation environment")
        if native and any(not path.startswith(tool["plan"] + "/")
                          for path, _ in tool["launch"]):
            raise ValueError("launch path outside qualified installation")
    return tools


def verify_launches(root, tools):
    for tool in tools.values():
        for relative_path, sha in tool["launch"]:
            path = root / relative_path
            for component in (path, *path.parents):
                if component.is_symlink():
                    raise ValueError("linked launch path")
            metadata = path.stat()
            if not stat.S_ISREG(metadata.st_mode) or metadata.st_nlink != 1:
                raise ValueError("invalid launch file")
            if hashlib.sha256(path.read_bytes()).hexdigest() != sha:
                raise ValueError("qualified launch bytes differ")


def main(install=True):
    root, mise, config, *selectors = sys.argv[1:]
    root = pathlib.Path(root)
    if len(config) > 65536 or not re.fullmatch("(?:[0-9a-f]{2})+", config):
        raise ValueError("invalid encoded configuration")
    config = bytes.fromhex(config).decode("utf-8", errors="strict")
    tools = configuration(config, selectors)
    if install:
        subprocess.run([mise, *FLAGS, "install", *selectors], check=True)
    verify_launches(root, tools)
    for selector, tool in tools.items():
        environment = dict(os.environ)
        for name, relative_path in tool["environment"].items():
            path = str(root / tool["plan"] / relative_path)
            environment[name] = path + ":" + environment.get("PATH", "") if name == "PATH" else path
        result = subprocess.run([mise, *FLAGS, "exec", selector, "--", tool["binary"], "--version"],
                                env=environment, check=True, stdout=subprocess.PIPE,
                                stderr=subprocess.STDOUT, text=True)
        pattern = r"(?<![0-9.])" + re.escape(tool["version"]) + r"(?![0-9.])"
        if not re.search(pattern, result.stdout):
            raise ValueError("qualified runtime version differs")


if __name__ == "__main__":
    main()
