"""Fixed terminal observation; transport identity never grants payload authority."""
import os
import re


def observe(env):
    """Report availability separately from preparation and publication failure."""
    value = lambda name: env.get("VELNOR_TOOL_" + name, "")
    digest = value("AFTER_DIGEST")
    before = value("BEFORE_DIGEST")
    changed = value("CHANGED")
    prepared = (
        value("INSTALL_OUTCOME") == "success"
        and value("VERIFIED") == "true"
        and value("BEFORE_AVAILABLE") == "true"
        and value("AFTER_AVAILABLE") == "true"
        and value("BEFORE_OUTCOME") == "success"
        and value("AFTER_OUTCOME") == "success"
        and re.fullmatch(r"[0-9a-f]{64}", before) is not None
        and re.fullmatch(r"[0-9a-f]{64}", digest) is not None
        and changed in ("true", "false")
        and (changed != "false" or digest == before)
    )
    available = False
    error = "PREPARATION_FAILED"
    if prepared:
        # Exact current bytes and compatibility identity; no origin inference.
        expected = re.escape(value("IDENTITY") + "-snapshot-" + digest + "-")
        warm = (
            changed == "false"
            and value("RESTORE_OUTCOME") == "success"
            and re.fullmatch(expected + r"[1-9][0-9]*-[1-9][0-9]*",
                             value("MATCHED_KEY")) is not None
        )
        published = (
            value("PUBLICATION_OUTCOME") == "success"
            and value("PUBLICATION_EXACT_HIT") == "true"
            and value("PUBLICATION_EXPECTED_KEY") != ""
            and value("PUBLICATION_MATCHED_KEY") == value("PUBLICATION_EXPECTED_KEY")
        )
        available = published or warm
        save = value("SAVE_OUTCOME")
        if save not in ("success", "skipped"):
            error = "CACHE_TRANSPORT_FAILED"
        elif save == "success" and not published:
            error = "CACHE_TRANSPORT_FAILED" if available else "CACHE_TRANSPORT_UNAVAILABLE"
        else:
            error = "NONE" if available else "CACHE_NOT_PUBLISHED"
    return {
        "cache_available": str(available).lower(),
        "verified": str(bool(prepared)).lower(),
        "toolidentity": value("IDENTITY"),
        "descriptoridentity": value("DESCRIPTOR_IDENTITY"),
        "error": error,
    }


def main():
    report = observe(os.environ)
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        for name, value in report.items():
            output.write(name + "=" + value + "\n")


if __name__ == "__main__":
    main()
