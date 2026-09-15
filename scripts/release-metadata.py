#!/usr/bin/env python3
"""Select explicit Docker aliases and GitHub prerelease status for a release ref."""

import os
from pathlib import Path
import re

VERSION = re.compile(
    r"v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


def release_policy(ref):
    if ref in ("refs/heads/main", "refs/heads/develop"):
        return [ref.removeprefix("refs/heads/")], False
    if not ref.startswith("refs/tags/"):
        raise ValueError("release ref must be main, develop or a version tag")
    tag = ref.removeprefix("refs/tags/")
    match = VERSION.fullmatch(tag)
    if not match:
        raise ValueError("release tags must be vMAJOR.MINOR.PATCH[-PRERELEASE]")
    major, minor, _, prerelease = match.groups()
    if prerelease and any(
        part.isdigit() and len(part) > 1 and part.startswith("0")
        for part in prerelease.split(".")
    ):
        raise ValueError("numeric prerelease identifiers cannot have leading zeroes")
    aliases = [tag[1:]]
    if not prerelease:
        aliases.extend([f"{major}.{minor}", "latest"])
    return aliases, bool(prerelease)


def main():
    aliases, prerelease = release_policy(os.environ["GITHUB_REF"])
    # Docker metadata-action adds the registry, variant suffixes, SHA and OCI labels.
    # Its automatic latest alias is disabled; only this policy may select aliases.
    docker_tags = "\n".join([*(f"type=raw,value={tag}" for tag in aliases), "type=sha"])
    with Path(os.environ["GITHUB_OUTPUT"]).open("a") as output:
        output.write(f"prerelease={str(prerelease).lower()}\n")
        output.write(f"docker_tags<<GRASS_RELEASE_TAGS\n{docker_tags}\nGRASS_RELEASE_TAGS\n")


if __name__ == "__main__":
    main()
