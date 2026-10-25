"""Bump a Gradle version catalog's [versions] to the newest stable releases."""
import re
import sys
import tomllib
import urllib.request
import xml.etree.ElementTree as ET

UNSTABLE = re.compile(r"(?i)(alpha|beta|rc|cr|milestone|preview|snapshot|incubating|dev|ea)|[.-]m\d")
CENTRAL = "https://repo.maven.apache.org/maven2"
PORTAL = "https://plugins.gradle.org/m2"


def releases(base, group, artifact):
    url = f"{base}/{group.replace('.', '/')}/{artifact}/maven-metadata.xml"
    try:
        with urllib.request.urlopen(url, timeout=20) as response:
            root = ET.fromstring(response.read())
    except Exception:
        return set()
    return {v.text for v in root.iter("version") if v.text and not UNSTABLE.search(v.text)}


def key(version):
    return [int(p) for p in re.findall(r"\d+", version)]


def shape(version):
    """Release line: `32.1.3-jre` and `33.0.0-jre` match, `r09` does not."""
    return re.sub(r"[\d.]+", "#", version)


def module_of(entry):
    if isinstance(entry, str):
        parts = entry.split(":")
        return (parts[0], parts[1], None) if len(parts) >= 2 else None
    if "module" in entry:
        group, artifact = entry["module"].split(":", 1)
    elif "group" in entry and "name" in entry:
        group, artifact = entry["group"], entry["name"]
    else:
        return None
    version = entry.get("version")
    ref = version.get("ref") if isinstance(version, dict) else entry.get("version.ref")
    return group, artifact, ref


def main(path):
    text = open(path, encoding="utf-8").read()
    data = tomllib.loads(text)
    versions = data.get("versions", {})
    candidates = {}
    for entry in data.get("libraries", {}).values():
        found = module_of(entry)
        if found and found[2] in versions:
            candidates.setdefault(found[2], []).append(releases(CENTRAL, found[0], found[1]))
    for entry in data.get("plugins", {}).values():
        if isinstance(entry, dict) and "id" in entry:
            version = entry.get("version")
            ref = version.get("ref") if isinstance(version, dict) else entry.get("version.ref")
            if ref in versions:
                plugin = entry["id"]
                candidates.setdefault(ref, []).append(releases(PORTAL, plugin, f"{plugin}.gradle.plugin"))
    for ref, sets in sorted(candidates.items()):
        current = versions[ref]
        if not isinstance(current, str) or not all(sets):
            continue
        common = {v for v in set.intersection(*sets) if shape(v) == shape(current)}
        newest = max(common, key=key, default=None)
        if newest is None or key(newest) <= key(current):
            continue
        pattern = re.compile(r'^(\s*"?' + re.escape(ref) + r'"?\s*=\s*")' + re.escape(current) + r'(")', re.M)
        text, count = pattern.subn(lambda m: m.group(1) + newest + m.group(2), text, count=1)
        if count:
            print(f"- {path}: {ref} {current} -> {newest}")
    open(path, "w", encoding="utf-8").write(text)


if __name__ == "__main__":
    main(sys.argv[1])
