#!/usr/bin/env python3
"""Check release metadata before building; print the single agreed version."""
import argparse
import json
from pathlib import Path
import re
import sys
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def version():
    manifests = ("Cargo.toml", "desktop/src-tauri/Cargo.toml")
    versions = {
        name: tomllib.loads((ROOT / name).read_text())["package"]["version"]
        for name in manifests
    }
    for name in ("desktop/package.json", "desktop/src-tauri/tauri.conf.json"):
        versions[name] = json.loads((ROOT / name).read_text())["version"]
    for name, package in (("Cargo.lock", "vnetctl"),
                          ("desktop/src-tauri/Cargo.lock", "vnet-studio-desktop")):
        packages = tomllib.loads((ROOT / name).read_text())["package"]
        versions[name] = next(p["version"] for p in packages if p["name"] == package)
    if len(set(versions.values())) != 1:
        raise ValueError("版本号不一致：" + json.dumps(versions, ensure_ascii=False))
    result = versions["Cargo.toml"]
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-[0-9A-Za-z]+(?:[.-][0-9A-Za-z]+)*)?", result):
        raise ValueError("版本号必须为 MAJOR.MINOR.PATCH，可附带预发布标识")
    return result


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--tag", help="Require a matching vMAJOR.MINOR.PATCH tag")
    args = parser.parse_args()
    try:
        value = version()
        if args.tag is not None and args.tag != f"v{value}":
            raise ValueError(f"标签 {args.tag!r} 与项目版本 v{value} 不一致")
        print(value)
    except (ValueError, KeyError, StopIteration) as error:
        print(str(error), file=sys.stderr)
        sys.exit(1)
