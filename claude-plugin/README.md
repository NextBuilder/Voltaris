# Voltaris

Design professional printed circuit boards in **KiCad 10** by talking to Claude. This plugin
connects Claude to KiCad on your own computer: it reads and edits schematics and boards, runs
ERC and DRC, checks your own plain-English design rules, reads datasheets with page
citations, draws block diagrams, simulates circuits, sizes traces and impedance, imports
LCSC parts, snapshots and diffs designs, writes Gerber, BOM and placement files for JLCPCB
and other fabs, generates an enclosure, and creates firmware pin maps and bring-up code for
ESP32, RP2040/RP2350, STM32 and nRF52. It works with your normal Claude plan — no API key.

It adds 15 skills (a gated professional design workflow plus specialists for power,
ESP32 and other microcontrollers, high-speed/RF, part sourcing, simulation, schematic, PCB,
libraries, review and manufacturing), 3 agents, and the Voltaris MCP server
with 257 tools loaded on demand.

## Requirements

- KiCad 10 with **Preferences → Plugins → Enable KiCad API** turned on (for live PCB edits)
- The Voltaris server: in KiCad open **Plugin and Content Manager → Install from
  File** and pick `voltaris-pcm-v<version>-<platform>.zip` from
  [Releases](https://github.com/NextBuilder/Voltaris/releases), then restart KiCad
- Node.js 18 or newer (it starts the server)
- Works in **Cowork** on your computer and in **Claude Code**. In chat the skills load but the
  KiCad tools need Cowork or Claude Code, because they run on your machine

## Install

- **Claude app (Cowork):** Customize → Plugins → Discover, search "Voltaris", Install.
  Before the directory listing is live: Customize → Plugins → Add → Add marketplace →
  `https://github.com/NextBuilder/Voltaris`, then install Voltaris.
- **Claude Code:**

  ```
  /plugin install voltaris --marketplace NextBuilder/Voltaris
  ```

If the server is installed somewhere unusual, set the environment variable
`VOLTARIS_BIN` to the full path of `voltaris.exe` (`voltaris` on macOS/Linux).

## Use it

Ask Claude things like:

- "Design a USB-C powered ESP32-S3 board with an SHT30 sensor and a Li-ion charger."
- "Every I2C line needs exactly one pull-up — check that from now on."
- "What does the TPS563200 datasheet say about layout?"
- "Snapshot the board, autoroute it, then show me what changed."
- "Generate the Gerbers and BOM for JLCPCB."

Claude follows the kicad-pro-workflow skill: requirements, research, datasheet rules,
schematic, placement, routing, an independent review and manufacturing files — reporting
evidence at every gate.

## What runs and what is sent

`start.mjs` (readable source in this folder) finds the Voltaris server installed by
KiCad's Plugin and Content Manager and starts it; it downloads nothing. If the server is
missing, it starts a one-tool helper that explains how to install it.

The server works on your computer. It contacts the internet only for these tools:
`download_jlcpcb_database` (bouni.github.io parts feed), `get_datasheet_url`,
`enrich_datasheets` and `read_datasheet` (wmsc.lcsc.com for LCSC numbers, or the datasheet
URL itself), and `import_lcsc_part` (EasyEDA/LCSC through easyeda2kicad). Your design files
are never uploaded.

## Privacy Policy

See [PRIVACY.md](PRIVACY.md) — also at
https://github.com/NextBuilder/Voltaris/blob/main/PRIVACY.md. In short: no
accounts, analytics or telemetry; everything is stored on your computer; the only network
traffic is listed above.

## Support and license

Issues: https://github.com/NextBuilder/Voltaris/issues. Licensed AGPL-3.0 — see NOTICE.md for credits. KiCad is a
trademark of the KiCad project; this plugin is independent and not affiliated with it.
