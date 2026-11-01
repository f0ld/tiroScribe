#!/usr/bin/env python3
"""Writes THIRD-PARTY-LICENSES.txt: everything shipped in the release and its licence text.

usage: third_party_licenses.py <output> [extracted AppDir]
With an AppDir, also lists the system libraries bundled there and their copyright files.
"""
import glob, json, os, subprocess, sys

HEADER = """tiroScribe is licensed under GPL-3.0-or-later (see LICENSE). It includes the following.

Linked into or shipped with the program, beyond the packages listed below:
- FFmpeg 8.1.3 (ffmpeg, ffprobe), LGPL-2.1-or-later, https://ffmpeg.org. Its source and build
  recipe are published with every tiroScribe release. It uses libopus (BSD-3-Clause).
- whisper.cpp and ggml (MIT), https://github.com/ggml-org/whisper.cpp
- sherpa-onnx (Apache-2.0), https://github.com/k2-fsa/sherpa-onnx, with its bundled ONNX Runtime
  (MIT), kaldi-native-fbank (Apache-2.0), OpenFst (Apache-2.0), kissfft (BSD-3-Clause),
  piper-phonemize (MIT) and espeak-ng (GPL-3.0-or-later).

Models are not included. They are downloaded on request, under their own licences:
- Whisper (MIT), primeLine German fine-tune (Apache-2.0), Silero VAD (MIT),
  pyannote segmentation 3.0 (MIT), 3D-Speaker CAM++ (Apache-2.0).
"""
LICENSE_PREFIXES = ("license", "licence", "copying", "notice", "unlicense")


def licence_files(directory):
    found = []
    for name in sorted(os.listdir(directory)):
        path = os.path.join(directory, name)
        if os.path.isfile(path) and name.lower().startswith(LICENSE_PREFIXES):
            found.append(path)
    return found


def section(out, title, spdx, directory):
    out.write(f"\n{'=' * 78}\n{title}\nLicense: {spdx or 'see below'}\n{'=' * 78}\n")
    for path in licence_files(directory):
        with open(path, errors="replace") as f:
            out.write(f"\n--- {os.path.basename(path)} ---\n{f.read()}\n")


def rust_packages(out):
    tree = subprocess.check_output(
        ["cargo", "tree", "-p", "tiroscribe", "-e", "normal", "--prefix", "none", "--format", "{p}"],
        text=True,
    )
    used = {tuple(line.split()[:2]) for line in tree.splitlines() if line.strip()}
    meta = json.loads(subprocess.check_output(["cargo", "metadata", "--format-version", "1"], text=True))
    members = set(meta["workspace_members"])
    for p in sorted(meta["packages"], key=lambda p: (p["name"], p["version"])):
        if (p["name"], "v" + p["version"]) in used and p["id"] not in members:
            section(out, f"{p['name']} {p['version']}", p.get("license"), os.path.dirname(p["manifest_path"]))


def frontend_packages(out):
    # Only these end up in the page; build tools do not ship.
    for name in ("svelte", "@tauri-apps/api", "@tauri-apps/plugin-dialog", "esm-env", "clsx"):
        directory = os.path.join("ui", "node_modules", name)
        if os.path.isdir(directory):
            with open(os.path.join(directory, "package.json")) as f:
                pkg = json.load(f)
            section(out, f"{name} {pkg['version']}", pkg.get("license"), directory)


def system_packages(out, appdir):
    packages = set()
    for lib in glob.glob(os.path.join(appdir, "usr", "lib", "**", "*.so*"), recursive=True):
        name = os.path.basename(lib)
        owner = subprocess.run(["dpkg", "-S", name], capture_output=True, text=True).stdout
        packages |= {line.split(":")[0] for line in owner.splitlines() if ":" in line}
    for pkg in sorted(packages):
        version = subprocess.run(
            ["dpkg-query", "-W", "-f=${Version}", pkg], capture_output=True, text=True
        ).stdout
        out.write(f"\n{'=' * 78}\n{pkg} {version} (Ubuntu 24.04, source: apt source {pkg})\n{'=' * 78}\n")
        copyright_file = f"/usr/share/doc/{pkg}/copyright"
        if os.path.exists(copyright_file):
            with open(copyright_file, errors="replace") as f:
                out.write(f.read())


def main():
    with open(sys.argv[1], "w") as out:
        out.write(HEADER)
        rust_packages(out)
        frontend_packages(out)
        if len(sys.argv) > 2:
            system_packages(out, sys.argv[2])


main()
