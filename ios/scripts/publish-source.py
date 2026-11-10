#!/usr/bin/env python3
"""Adds this CI build to the sideloading source (AltStore format, which LiveContainer reads).

usage: publish-source.py <apps.json> <Horizon.ipa> <version> <tag>

Rewrites <apps.json> in place (it is made when missing) with this build first and the ten newest
builds kept, and prints the release tags that fell off the list, one a line, for the workflow to
delete. Reads GITHUB_REPOSITORY, GITHUB_REF_NAME and GITHUB_SHA.
"""
import datetime
import json
import os
import sys

KEEP = 10
BUNDLE_ID = "io.github.worrcc.horizon"
SOURCE_BRANCH = "ios-source"


def main() -> None:
    path, ipa, version, tag = sys.argv[1:5]
    repo = os.environ["GITHUB_REPOSITORY"]
    branch = os.environ.get("GITHUB_REF_NAME", "")
    commit = os.environ.get("GITHUB_SHA", "")[:9]
    raw = f"https://raw.githubusercontent.com/{repo}/{SOURCE_BRANCH}"
    try:
        with open(path, encoding="utf-8") as f:
            versions = json.load(f)["apps"][0]["versions"]
    except (OSError, ValueError, KeyError, IndexError):
        versions = []
    build = {
        "version": version,
        "buildVersion": version,
        "date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "localizedDescription": f"Branch {branch}, commit {commit}.",
        "downloadURL": f"https://github.com/{repo}/releases/download/{tag}/Horizon.ipa",
        "size": os.path.getsize(ipa),
        "minOSVersion": "15.0",
    }
    versions = [build] + [v for v in versions if v.get("version") != version]
    dropped = versions[KEEP:]
    versions = versions[:KEEP]
    description = (
        "Horizon, a Rust rewrite of Minecraft: Java Edition, for iPhone and iPad (unofficial fork "
        "builds). It holds no game files: you add your own. Each build is listed with its branch."
    )
    app = {
        "name": "Horizon",
        "bundleIdentifier": BUNDLE_ID,
        "developerName": repo.split("/")[0],
        "subtitle": "Rust Minecraft 26.3 for iOS",
        "localizedDescription": description,
        "iconURL": f"{raw}/icon.png",
        "tintColor": "3C8527",
        "category": "games",
        "screenshots": [],
        "versions": versions,
        "appPermissions": {"entitlements": [], "privacy": {}},
        # The newest build again, for readers of the older source format.
        "version": build["version"],
        "versionDate": build["date"],
        "versionDescription": build["localizedDescription"],
        "downloadURL": build["downloadURL"],
        "size": build["size"],
    }
    source = {
        "name": "Horizon iOS (fork builds)",
        "identifier": BUNDLE_ID + ".source",
        "sourceURL": f"{raw}/apps.json",
        "website": f"https://github.com/{repo}",
        "iconURL": f"{raw}/icon.png",
        "apps": [app],
        "news": [],
    }
    with open(path, "w", encoding="utf-8") as f:
        json.dump(source, f, indent=2)
        f.write("\n")
    for old in dropped:
        # downloadURL is .../releases/download/<tag>/Horizon.ipa
        print(old.get("downloadURL", "").split("/")[-2:-1][0] if old.get("downloadURL") else "")


if __name__ == "__main__":
    main()
