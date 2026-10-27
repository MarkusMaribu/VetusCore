#!/usr/bin/env python3
# This Source Code Form is subject to the terms of the Mozilla Public
# License, v. 2.0. If a copy of the MPL was not distributed with this
# file, You can obtain one at https://mozilla.org/MPL/2.0/.
"""Collect supplied dependency notices; does not collect corresponding source."""

import argparse
import json
from pathlib import Path
import re
import subprocess


ROOT = Path(__file__).resolve().parent.parent
NATIVE = {
    "vendor/dynarmic": "LICENSE.txt",
    "vendor/openal-soft": "COPYING",
    "vendor/SDL": "LICENSE.txt",
    "vendor/libxml2": "Copyright",
    "vendor/stb": "LICENSE",
    "vendor/PVRTDecompress": "LICENSE.md",
    "touchHLE_fonts": "LICENSE.noto",
    "touchHLE_dylibs": "COPYING.libgcc",
}
# Published packages sometimes omit the repository's license files. Keep
# fallbacks version-specific so an update requires checking its notices again.
FALLBACKS = {
    ("gl_generator", "0.14.0"): ["Apache-2.0.txt", "gl-rs-NOTICE.txt"],
    ("khronos_api", "3.1.0"): ["Apache-2.0.txt", "gl-rs-NOTICE.txt"],
    ("mach_object", "0.1.17"): ["Apache-2.0.txt"],
    ("openh264", "0.9.8"): ["openh264-BSD-2-Clause.txt"],
    ("winapi-i686-pc-windows-gnu", "0.4.0"): ["winapi-MIT.txt"],
    ("winapi-x86_64-pc-windows-gnu", "0.4.0"): ["winapi-MIT.txt"],
}


def notice_files(directory):
    """Include nested native-code notices shipped inside Cargo packages too."""
    for path in sorted(directory.rglob("*")):
        if not path.is_file() or ".git" in path.relative_to(directory).parts:
            continue
        name = path.name.lower()
        if name.startswith(("license", "licence", "copying", "copyright", "notice", "authors")):
            yield path


def read_notice(path):
    data = path.read_bytes()
    try:
        return data.decode("utf-8-sig")
    except UnicodeDecodeError:
        return data.decode("latin-1")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("output", type=Path)
    parser.add_argument("--offline", action="store_true")
    args = parser.parse_args()
    command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    if args.offline:
        command.append("--offline")
    metadata = json.loads(subprocess.check_output(command, cwd=ROOT))
    sections = [read_notice(ROOT / name) for name in (
        "NOTICE", "LICENSE", "LICENSING.md", "dev-scripts/licenses/Boost.txt"
    )]
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True
    ).strip()
    sections.insert(0, f"VetusCore dependency notices\nSource revision: {revision}\n"
                    "Includes resolved build/development dependencies.\n"
                    "Corresponding source/relinking requirements: see LICENSING.md.\n")
    missing = []
    members = set(metadata["workspace_members"])
    for package in sorted(metadata["packages"], key=lambda p: (p["name"], p["version"])):
        if package["id"] in members:
            continue
        label = f"{package['name']} {package['version']}"
        directory = Path(package["manifest_path"]).parent
        files = set(notice_files(directory))
        fallbacks = FALLBACKS.get((package["name"], package["version"]), [])
        if package.get("license_file"):
            files.add(directory / package["license_file"])
        if not (files or fallbacks) or not (package.get("license") or package.get("license_file")):
            missing.append(label)
        sections.append(f"{label}\nLicense: {package.get('license') or 'see supplied file'}\n"
                        f"Authors: {', '.join(package['authors'])}\n"
                        f"Source: {package.get('repository') or package.get('source') or 'local'}")
        for path in sorted(files):
            sections.append(f"{label} / {path.relative_to(directory)}\n\n{read_notice(path)}")
        for name in fallbacks:
            sections.append(f"{label} / supplied license supplement\n\n"
                            + read_notice(ROOT / "dev-scripts/licenses" / name))
        if package["name"] == "khronos_api":
            # Registry and WebGL IDL inputs carry notices inside their headers,
            # rather than in separate LICENSE files.
            for path in sorted(directory.rglob("*")):
                if path.suffix not in (".xml", ".idl"):
                    continue
                content = read_notice(path)
                for match in re.finditer(r"<comment>(.*?)</comment>|/\*(.*?)\*/|<!--(.*?)-->",
                                         content, re.DOTALL):
                    block = next(part for part in match.groups() if part is not None)
                    if "copyright" in block.lower():
                        sections.append(f"{label} / {path.relative_to(directory)}\n\n{block}")
    for relative, required in NATIVE.items():
        directory = ROOT / relative
        if not (directory / required).is_file():
            missing.append(f"{relative}/{required} (initialize recursive submodules)")
            continue
        files = set(notice_files(directory))
        if relative == "vendor/dynarmic":
            # Its README carries additional third-party notices.
            files.add(directory / "README.md")
        if relative == "touchHLE_dylibs":
            # Contains source locations and runtime-exception acknowledgements.
            files.add(directory / "README.md")
        if relative == "touchHLE_fonts":
            # The Heros-derived guest substitutes need their modification and
            # original-source notices in binary distributions as well.
            files.add(directory / "README.md")
            files.add(directory / "README-Alpine-Emulation-Sans.txt")
        for path in sorted(files):
            sections.append(f"{path.relative_to(ROOT)}\n\n{read_notice(path)}")
    if missing:
        raise SystemExit("Missing license material:\n" + "\n".join(missing))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n\n" + ("\n\n" + "=" * 72 + "\n\n").join(sections), encoding="utf-8")
    print(f"Wrote dependency notices to {args.output}")


if __name__ == "__main__":
    main()
