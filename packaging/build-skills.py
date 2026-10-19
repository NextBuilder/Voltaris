#!/usr/bin/env python3
"""Zip each bundled skill for upload to Claude Desktop / claude.ai (Settings → Capabilities → Skills).

Usage: python packaging/build-skills.py [--out DIR]
Writes DIR/skills/<skill>.zip (default DIR: dist). Front-matter keys Claude's skill upload does not
accept are dropped; references/ files are kept.
"""
import argparse
import pathlib
import re
import zipfile

ALLOWED = {"name", "description", "license", "allowed-tools", "metadata"}


def clean_front_matter(text: str) -> str:
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        raise SystemExit("SKILL.md without front matter")
    kept, keep = [], True
    for line in m.group(1).splitlines():
        key = re.match(r"^([A-Za-z0-9_-]+):", line)
        if key:
            keep = key.group(1) in ALLOWED
        if keep:
            kept.append(line)
    return "---\n" + "\n".join(kept) + "\n---\n" + text[m.end():]


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default="dist")
    args = ap.parse_args()
    root = pathlib.Path(__file__).resolve().parent.parent
    src = root / "crates" / "voltaris" / "assets" / "skills"
    out = pathlib.Path(args.out) / "skills"
    out.mkdir(parents=True, exist_ok=True)
    for skill in sorted(p for p in src.iterdir() if (p / "SKILL.md").is_file()):
        target = out / f"{skill.name}.zip"
        with zipfile.ZipFile(target, "w", zipfile.ZIP_DEFLATED) as z:
            for f in sorted(skill.rglob("*")):
                if not f.is_file():
                    continue
                arc = f"{skill.name}/{f.relative_to(skill).as_posix()}"
                data = f.read_text(encoding="utf-8")
                if f.name == "SKILL.md" and f.parent == skill:
                    data = clean_front_matter(data)
                z.writestr(arc, data)
        print(target)


if __name__ == "__main__":
    main()
