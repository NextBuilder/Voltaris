# Release package

A board is finished when someone else can build, use and repair it from the files alone. This
is the package CircuitDigest publishes with each open-hardware board, and what PCBCupid ships
in its docs — produce all of it at Gate 7.

```
load_toolset('manufacturing')    # validate_for_manufacturing export_manufacturing_package
load_toolset('enclosure')        # generate_enclosure
load_toolset('firmware')         # generate_firmware_kit
load_toolset('design_review')    # check_bom_health run_design_review
```

## Folder layout

```
<project>/
├── README.md              overview, specs, pinout, power, how to use, how to build
├── hardware/
│   ├── <project>.kicad_pro / .kicad_sch / .kicad_pcb
│   ├── schematic.pdf      every sheet, readable on paper
│   ├── production/        Gerbers + drill (zip), BOM, pick-and-place (CPL), assembly notes
│   ├── renders/           top, bottom and iso renders; the board in its enclosure
│   └── enclosure/         STEP and STL of the case
├── firmware/              pin map header and bring-up example
└── LICENSE                hardware licence (CERN-OHL-S/-P or CC BY-SA) and software licence
```

## README sections

1. **What it is** — one paragraph and a top render.
2. **Specifications** — a table: MCU, radio, supply range, battery and charge current, I/O
   count, interfaces, board size, mounting, operating temperature.
3. **Pinout** — a table whose names match the silkscreen exactly (Pin, Silkscreen, Function,
   Voltage), plus a pinout image.
4. **Power** — every way to power it, what it must never be connected to (for example "never
   more than 4.2 V on BAT"), and the current each rail can supply.
5. **Jumpers and options** — each solder jumper, its default and what changing it does.
6. **Getting started** — board package or toolchain, first upload, a blink or sensor example.
7. **Build it** — order the PCB (fab settings: layers, thickness, finish, colour), the BOM with
   LCSC numbers, assembly notes, programming and test steps.
8. **Revision history** — what changed in each version printed on the board.

## Fab order settings to record

Layers, thickness, finish (HASL lead-free or ENIG for fine pitch/edge contacts), mask and
silkscreen colour, copper weight, via treatment (tented, or plugged when vias sit in pads),
castellated holes or edge plating if used, panelisation, and the assembly side.

## Final checks before release

- `validate_for_manufacturing` and `export_manufacturing_package` with no warnings left open.
- `check_bom_health`: every part in stock, no end-of-life parts, alternates for the critical
  ones.
- Gerbers opened in a viewer (KiCad's Gerber viewer) — silkscreen readable, nothing clipped,
  mask openings where they belong.
- Renders checked against the README pinout.
- `generate_firmware_kit` pin map matches the schematic; `generate_enclosure` fits with the
  connectors reachable.
