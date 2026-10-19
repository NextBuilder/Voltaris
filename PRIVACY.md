# Voltaris — Privacy Policy

Effective 18 October 2026. Applies to the Voltaris MCP server, its Claude plugin,
its Claude Desktop extension and its KiCad plugin.

## Data collection

Voltaris has no accounts, analytics, telemetry or advertising. It runs on your own
computer and the developer receives nothing from it.

## Usage and storage

- It reads and writes the KiCad projects you point it at (schematics, boards, libraries,
  project files) and talks to KiCad on your computer through KiCad's local IPC API.
- It stores, on your computer only: a local log of tool calls (`<Voltaris config folder>/logs/calls.jsonl`)
  for troubleshooting, design snapshots and check rules in your project's `.voltaris` folder,
  datasheets you ask it to read (cached in `.voltaris/datasheets` or your user cache folder),
  and the JLCPCB parts database if you download it.
- What Claude sees is whatever the tools return in your conversation; that conversation is
  handled by Anthropic under Anthropic's own privacy policy.

## Third-party sharing

Nothing is sent anywhere unless you use a tool that needs the internet. Those tools contact:

| Tool | Contacts | Sends |
|---|---|---|
| `download_jlcpcb_database` | bouni.github.io (kicad-jlcpcb-tools parts feed) | nothing beyond the download request |
| `get_datasheet_url`, `enrich_datasheets`, `read_datasheet` (by LCSC number) | wmsc.lcsc.com | the LCSC part numbers you look up |
| `read_datasheet` (by URL) | the datasheet URL you or the part record give | the download request |
| `import_lcsc_part` | EasyEDA / LCSC, through the open-source easyeda2kicad tool | the LCSC part numbers you import |

Your design files are never uploaded. Searches of the JLCPCB parts database run on the
local copy. Freerouting, ngspice and kicad-cli run as local programs.

## Data retention

All stored data stays on your computer until you delete it: remove the `.voltaris` folder in
a project, the datasheet cache, or the Voltaris config folder's `logs` directory at any time.

## Contact

Questions or requests: open an issue at https://github.com/NextBuilder/Voltaris/issues
(maintainer: Kishan Pratap Singh, Next Builder).
