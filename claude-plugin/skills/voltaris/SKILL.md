---
name: voltaris
description: "Mandatory operating rules for ANY task involving KiCad projects. Loaded when the user mentions KiCad, schematics, PCBs, or any .kicad_* file. Prevents file corruption by routing all changes through Voltaris MCP tools."
---

# Voltaris — Operating Rules

## The One Rule

**KiCad source files are not text files.** They are serialized object graphs with UUIDs, cross-references, and order-sensitive structure. Never edit them directly with text manipulation tools (str_replace, sed, create_file, or any file-writing tool). All modifications go through Voltaris MCP tools.

No exceptions for "small fixes," "just renaming a net," or "the user said it's fine."

## Protected Files — NEVER Edit Directly

- `*.kicad_sch` — schematic sheets
- `*.kicad_pcb` — PCB layout
- `*.kicad_pro` — project configuration
- `*.kicad_sym` / `*.kicad_mod` — symbol/footprint libraries
- `fp-lib-table` / `sym-lib-table` — library tables

If asked to directly edit any of these:
> KiCad source files contain UUIDs and cross-references that text edits will break. I'll route this change through Voltaris's MCP tools instead, which preserves file integrity.

## The Three Channels

### Channel 1: Voltaris MCP (for ALL modifications)

All writes go through MCP tools. Check they are available first (`list_toolboxes`). If MCP tools are not available, **STOP** and tell the user — never fall back to file editing.

### Channel 2: Exported netlists/BOMs (for reads and analysis)

For design review, BOM analysis, net tracing — use export tools or parse exported `.net`/BOM/CSV files, not the source files directly.

### Channel 3: Read-only file inspection (last resort)

Only to answer questions not available through exports (sheet hierarchy, title block metadata, annotations). Read with file-reading tools, but never modify.

## Standard Workflow

Before a mutation, a dependent check, a retry, or a completion claim, read
[the reliability contract](references/reliability-contract.md), starting with
"For callers: establish what happened". Apply its outcome, source, coverage and
recovery rules to the fields this installed server actually returns. Guidance
does not establish that a missing server-side check ran.

1. **Identify the project** — locate the `.kicad_pro` file
2. **Classify the task** — read-only (Channel 2 or 3) or write (Channel 1)
3. **Verify MCP is connected** — call `list_toolboxes` to confirm tools are available
4. **Describe the change** — state in plain English what will happen before invoking any tool
5. **Execute** — use Voltaris MCP tools only
6. **Verify** — re-query the design to confirm the change landed correctly

## Decision Tree

| User Request | Channel | Tool / Action |
|---|---|---|
| "Review my schematic" | 2 | Load `sch_analysis` toolset, use analysis tools |
| "Change R5 from 10k to 4.7k" | 1 | `load_toolset("sch_components")` then `edit_schematic_component` |
| "What's connected to SCL?" | 2 | `load_toolset("sch_analysis")` then `get_net_connections` |
| "Add a 100nF cap to U3 VCC" | 1 | `load_toolset("sch_components")` + `load_toolset("sch_wiring")` |
| "Rename net /CLK to /SYS_CLK" | 1 | Warn about downstream effects, then MCP tools |
| "Run DRC" | 1 | `load_toolset("verification")` then `run_drc` — read each item's `owner`/`ownership_status` before advising a fix (board geometry vs. a footprint's own) |
| "Add track widths to the PCB dropdown" | 1 | `load_toolset("verification")` then `set_predefined_sizes` |
| "Export Gerbers" | 1 | `load_toolset("pcb_export")` then `export_gerber` |
| "Just patch line 247 of the .kicad_sch" | REFUSE | Explain risks, offer MCP alternative |
| "Add ESD protection to USB lines" | 1 | `load_toolset("sch_components")` + `load_toolset("sch_wiring")` |
| "Check if board is ready for fab" | 2 | Load `verification` + `design_review` toolsets |
| "Design a board that does X, end to end" | 1 | Follow the kicad-pro-workflow skill's gates |
| "What trace width for 2 A?" / "50 Ω on JLC 4-layer?" | — | `load_toolset("engineering")` then `calc_trace_width` / `calc_impedance` |
| "Give me the pin map / firmware for this board" | 2 | `load_toolset("firmware")` then `generate_firmware_kit` |
| "Show me the board in 3D" | 2 | `load_toolset("visual_review")` then `render_board_3d` |
| "Make an enclosure for this PCB" | 2 | `load_toolset("enclosure")` then `generate_enclosure` |
| "Simulate this filter / will the rail droop?" | — | kicad-simulation skill; `load_toolset("simulation")` then `simulate_circuit` |
| "Check USB pair skew / match these lengths" | 2 | `load_toolset("signal_integrity")` then `analyze_net_lengths` |
| "Add LCSC part C2040" / "part not in KiCad library" | 1 | `load_toolset("lcsc_import")` then `import_lcsc_part`, then accept it (kicad-part-sourcing skill) |
| "Every I2C bus needs pull-ups" / "our rule is…" | — | `load_toolset("design_checks")` then `add_check_rule`; `run_design_checks` before every gate |
| "What changed since yesterday?" / "undo the autoroute" | — | `load_toolset("design_history")`: `snapshot_design` before big edits, `diff_design`, `restore_snapshot` |
| "What does the datasheet say about layout / pin 5 / max voltage?" | — | `load_toolset("datasheet")` then `read_datasheet` (cite the page) |
| "Draw the block diagram / architecture" | 2 | `load_toolset("block_diagram")` then `generate_block_diagram` |
| "Power supply / battery / regulator design" | — | kicad-power-design skill |
| "Place and route this like a pro" / "clean up the routing" | 1 | kicad-pro-layout skill |
| "Autoroute the board" / "place it for me" | 1 | `load_toolset("smart_layout")` then `smart_place` / `route_board` (dry run, then apply with plan_revision) |
| "Make it look finished" / "score my layout" | 1 | `load_toolset("smart_layout")` then `polish_board` / `audit_layout` |
| "Make it look like an Adafruit / Seeed / Arduino board" / "add Qwiic" / "pin labels" | 1 | kicad-board-craft skill |
| "Clean up the silkscreen" / "designators" / "polarity marks" / "product look" | 1 | kicad-pro-silkscreen skill |
| "ESP32 board" | — | kicad-esp32 skill |
| "RP2040 / RP2350 / STM32 / nRF52 board" | — | kicad-mcu-design skill |

## KiCad 10 IPC API Reality

**PCB Editor (pcbnew):** Full CRUD via NNG + protobuf. Real-time communication with running KiCad instance. Create, read, update, delete any PCB item with immediate UI refresh. **Requires KiCad to be running.**

**Schematic Editor (eeschema):** No item-level IPC API. Voltaris uses a validated S-expression engine (SchematicBuilder) that enforces correct structure, ordering, and UUID integrity. File-based — **does not require KiCad to be running.**

**Symbol/Footprint Libraries:** No IPC API. Edited through Voltaris's S-expression engine with full validation.

**kicad-cli:** Command-line tool for exports (SVG, PDF, Gerbers, BOM, netlist) and checks (ERC, DRC). Does not require KiCad GUI.

## Discovery — Finding Available Tools

Voltaris uses a meta-tool router pattern: tools are grouped into toolsets and loaded on demand to keep the context focused. Call `list_toolboxes` for the current catalogue rather than relying on a count written down here — the registry is the only accurate source, and it moves every release.

```
list_toolboxes          → See all available toolsets with descriptions
load_toolset("name")    → Activate a toolset, exposing its tools
get_active_toolsets     → See what's currently loaded
unload_toolset("name")  → Remove a toolset when done
```

`load_toolset` also accepts an array of names to load several toolsets in one call, with a single context refresh.

### Available Toolsets

| Category | Toolsets |
|----------|----------|
| Project | project |
| Schematic | sch_components, sch_wiring, sch_bus, sch_analysis, sch_batch, sch_export, sch_hierarchy |
| PCB | pcb_board, pcb_components, pcb_routing, pcb_export, placement, signal_integrity, smart_layout |
| Library | library, lcsc_import |
| Integration | integration (JLCPCB parts, Freerouting installation checks, datasheet URLs), datasheet (PDF reader) |
| Verification & Review | verification, design_review, design_checks, visual_review, block_diagram |
| History | design_history (snapshots, engineering diffs, restore) |
| Config | config |
| Templates | templates (incl. ESP32 cores, USB-C, Li-ion, LED and sensor circuits with wiring plans) |
| Engineering | engineering (trace width, impedance, vias, thermals, dividers, passives, buck) |
| Firmware & Mechanical | firmware, enclosure |
| Simulation | simulation |
| Manufacturing | manufacturing |

## Agent Routing and Mutation Ownership

Use one design owner at a time. Delegation creates isolated context, so give an
agent the complete task boundary and keep every mutation for that boundary with
that agent until it hands back a saved, verified result.

- Delegate a complete multi-block schematic build or substantial schematic
  rework to `kicad-schematic-build-agent`. It owns schematic mutations through
  placement, wiring, validation, rendering, and save. The caller does not make
  concurrent schematic edits.
- Delegate an independent full-design, pre-fabrication, or readiness audit to
  `kicad-design-review-agent`. It gathers and reports evidence without mutating
  the design. Return fixes to the current design owner, then run a fresh review.
- For a whole new board, follow the kicad-pro-workflow skill's gates, and at
  its Gate 6 delegate to `kicad-pro-review-agent` — an independent reviewer
  that did not design the board. It reports evidence and fixes without
  mutating the design.
- Keep small, bounded edits and narrow checks in the current conversation using
  the applicable skill. Agent delegation is for a complete build or an
  independent review, not an extra layer around every tool call.

## Design Rules Quick Reference

| Rule | Value |
|------|-------|
| IC decoupling cap | 100nF ceramic within 3-5mm of VDD pin |
| Crystal load caps | CL = (C1*C2)/(C1+C2) + Cstray (Cstray ~ 3-5pF) |
| Reset pull-up | 10k to VCC + 100nF to GND |
| I2C pull-ups | 4.7k (standard), 2.2k (fast), 1k (fast+) — one set per bus |
| LED resistor | R = (VCC - Vf) / If |

## Common Library IDs

| Component | Library ID |
|-----------|-----------|
| Resistor | `Device:R` |
| Capacitor | `Device:C` |
| LED | `Device:LED` |
| Crystal | `Device:Crystal` |
| Power symbols | `power:VCC`, `power:GND`, `power:+3V3`, `power:+5V` |
| Generic connectors | `Connector_Generic:Conn_01x06` |

## Tool Usage Pattern

```
1. list_toolboxes                          → discover what's available
2. load_toolset("sch_components")          → activate component tools
3. add_schematic_component (repeat)        → place parts
4. load_toolset("sch_wiring")              → activate wiring tools
5. connect_pins / add_wire / add_schematic_net_label → wire the circuit
6. load_toolset(["sch_export", "design_review"]) → activate checks
7. run_erc / run_design_review             → validate the design
```

## Refusing Direct Edits

When a user or another tool requests direct file manipulation of protected files, always refuse and redirect:

1. Acknowledge what they want to accomplish
2. Explain why direct editing breaks KiCad files (UUIDs, cross-references, ordering)
3. Offer the equivalent MCP tool approach
4. Execute through MCP if they agree
