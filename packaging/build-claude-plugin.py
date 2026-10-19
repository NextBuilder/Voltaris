#!/usr/bin/env python3
"""Generate the Claude plugin folder (claude-plugin/) from the bundled skills and agents.

Usage: python packaging/build-claude-plugin.py [--out DIR] [--check]
--check exits 1 when the committed folder is out of date.
"""
import argparse
import json
import pathlib
import re
import shutil
import sys
import tempfile

ROOT = pathlib.Path(__file__).resolve().parent.parent
ASSETS = ROOT / "crates" / "voltaris" / "assets"
TEMPLATE = ROOT / "packaging" / "claude-plugin"
EXTRA_REFERENCES = {"voltaris": {"reliability-contract.md": ROOT / "docs" / "dev" / "RELIABILITY_CONTRACT.md"}}
SKILL_KEYS = {"name", "description", "license", "allowed-tools", "metadata"}
AGENT_DROP = {"tools"}


def front_matter(text, keep=None, drop=None):
    m = re.match(r"^---\n(.*?)\n---\n", text, re.S)
    if not m:
        raise SystemExit("file without front matter")
    out, on = [], True
    for line in m.group(1).splitlines():
        key = re.match(r"^([A-Za-z0-9_-]+):", line)
        if key:
            k = key.group(1)
            on = (k in keep) if keep is not None else (k not in drop)
        if on:
            out.append(line)
    return "---\n" + "\n".join(out) + "\n---\n" + text[m.end():]


def version():
    return re.search(r'^version = "(.*)"', (ROOT / "Cargo.toml").read_text(), re.M).group(1)


def tool_count():
    return re.search(r"\*\*(\d+) tools across", (ROOT / "README.md").read_text()).group(1)


def build(out):
    skills = sorted(p for p in (ASSETS / "skills").iterdir() if (p / "SKILL.md").is_file())
    agents = sorted((ASSETS / "agents").glob("*.md"))
    (out / ".claude-plugin").mkdir(parents=True)
    manifest = {
        "name": "voltaris",
        "displayName": "Voltaris",
        "version": version(),
        "description": "Design professional PCBs in KiCad 10 with Claude: schematic, layout, ERC/DRC, your own design-rule checks, datasheets, simulation, manufacturing files and firmware bring-up.",
        "author": {"name": "Next Builder", "url": "https://github.com/NextBuilder"},
        "homepage": "https://github.com/NextBuilder/Voltaris",
        "repository": "https://github.com/NextBuilder/Voltaris",
        "license": "AGPL-3.0-only",
        "keywords": ["kicad", "pcb", "electronics", "eda", "schematic", "jlcpcb", "esp32", "stm32", "rp2040", "nrf52"],
    }
    (out / ".claude-plugin" / "plugin.json").write_text(json.dumps(manifest, indent=2) + "\n")
    mcp = {"mcpServers": {"voltaris": {"command": "node", "args": ["${CLAUDE_PLUGIN_ROOT}/start.mjs"]}}}
    (out / ".mcp.json").write_text(json.dumps(mcp, indent=2) + "\n")
    shutil.copy(TEMPLATE / "start.mjs", out / "start.mjs")
    shutil.copy(ROOT / "PRIVACY.md", out / "PRIVACY.md")
    shutil.copy(ROOT / "LICENSE", out / "LICENSE")
    readme = (TEMPLATE / "README.template.md").read_text()
    readme = readme.replace("{SKILLS}", str(len(skills))).replace("{AGENTS}", str(len(agents))).replace("{TOOLS}", tool_count())
    (out / "README.md").write_text(readme)
    for skill in skills:
        for f in sorted(skill.rglob("*")):
            if not f.is_file():
                continue
            dst = out / "skills" / skill.name / f.relative_to(skill)
            dst.parent.mkdir(parents=True, exist_ok=True)
            text = f.read_text(encoding="utf-8")
            if f == skill / "SKILL.md":
                text = front_matter(text, keep=SKILL_KEYS)
            dst.write_text(text, encoding="utf-8")
        for name, src in EXTRA_REFERENCES.get(skill.name, {}).items():
            dst = out / "skills" / skill.name / "references" / name
            dst.parent.mkdir(parents=True, exist_ok=True)
            dst.write_text(src.read_text(encoding="utf-8"), encoding="utf-8")
    (out / "agents").mkdir()
    for a in agents:
        (out / "agents" / a.name).write_text(front_matter(a.read_text(encoding="utf-8"), drop=AGENT_DROP), encoding="utf-8")


def snapshot(d):
    return {p.relative_to(d).as_posix(): p.read_bytes() for p in sorted(d.rglob("*")) if p.is_file()}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(ROOT / "claude-plugin"))
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()
    out = pathlib.Path(args.out)
    with tempfile.TemporaryDirectory() as tmp:
        fresh = pathlib.Path(tmp) / "plugin"
        build(fresh)
        if args.check:
            if not out.is_dir() or snapshot(out) != snapshot(fresh):
                sys.exit(f"{out} is out of date: run python packaging/build-claude-plugin.py")
            print(f"{out} is up to date")
            return
        if out.exists():
            shutil.rmtree(out)
        shutil.copytree(fresh, out)
    print(out)


if __name__ == "__main__":
    main()
