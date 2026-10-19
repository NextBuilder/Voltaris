---
name: kicad-product-board
description: |
  Ship a board the way maker product companies do (PCBCupid's Glyph boards, CircuitDigest's open
  hardware, Adafruit, Seeed, Waveshare): form factor first, proven circuit blocks, net classes
  and rules measured from 15 shipped KiCad boards, single-side assembly, product silkscreen with
  function captions, labelled jumpers and test points, a full release package, and a measured
  scorecard instead of opinions. Triggers on: "product", "ship it", "production board",
  "like a product", "dev board", "breakout", "module", "Glyph", "PCBCupid", "CircuitDigest",
  "professional board", "release", "open hardware", "make it sellable", "best possible board".
---

# Product Board Playbook

A hobby board works on the bench. A product board works for a stranger: it fits the enclosure
it was promised for, assembles on a machine, survives the cable being plugged in wrong, tells
the user what every pin and jumper does, and ships with the files someone else needs to build
it. This playbook is distilled from measuring 15 KiCad boards that teams actually shipped —
PCBCupid's Glyph and G-Sense modules and dev kits, and CircuitDigest's LiteWing drone, FIDO2
key, ToF scanner and robot controller (see [reference board study](references/reference-board-study.md))
— and from auditing them with Voltaris's own engines.

It sits on top of kicad-pro-workflow (the gates) and kicad-pro-layout (the layout method):
use it to make the decisions those gates ask for the way product teams make them.

```
load_toolset('smart_layout')     # smart_place route_board polish_board audit_layout
load_toolset('pcb_board')        # set_board_size add_board_outline add_mounting_hole add_board_text import_svg_logo
load_toolset('pcb_routing')      # create_netclass assign_net_to_class add_copper_pour
load_toolset('engineering')      # calc_trace_width calc_via calc_impedance
load_toolset('verification')     # run_drc set_design_rules set_predefined_sizes
load_toolset('visual_review')    # render_board_3d
```

## 1. Decide the product before the circuit (Gate 0)

Ask the user, with suggested options, and write the answers into the brief:

| Decision | Product-team default |
|---|---|
| Who assembles it | A machine (JLCPCB PCBA): parts on one side, 0402/0603 passives. Hand assembly: 0805 hand-solder footprints, nothing under 0.5 mm pitch |
| Form factor | From the enclosure, mount or ecosystem — never "whatever fits". Sizes on 2.54 mm multiples, corners rounded 1–3 mm |
| Power | USB-C first; a LiPo with charger and automatic USB/battery switchover for portable boards; an input range printed on the board for anything else |
| Interfaces | Breadboard-pitch headers, plus one ecosystem connector (Qwiic/STEMMA QT, Grove) for sensors |
| Silkscreen style | Product style (designators hidden, function captions) or debug style (designators shown) — see section 6 |
| Configuration | Solder jumpers for options (I2C address, pull-ups, current limit, termination), each labelled |

**Form factor rules from shipped boards:** PCBCupid sizes its modules on the 0.1" grid
(50.8 × 22.9 and 25.4 × 22.9 mm) with four rounded corners; a drop-in replacement copies the
original module's outline and pin positions exactly and is checked with a measure tool; a board
that sits in a 3D-printed part is drawn about 1 mm smaller per side than the pocket it sits in
(PCBCupid's guide uses a 17 mm radius for an 18 mm pocket); a board that sits behind a motor or display takes that part's footprint (a NEMA 17 board is
42.3 mm square). Lock the outline once agreed.

## 2. Build from proven blocks (Gate 1–2)

Every block a maker board needs — USB-C sink, ESD, LDO, buck, LiPo charger with power path,
reverse-polarity and input protection, crystal, QSPI flash, MOSFET loads, status LEDs,
BOOT/RESET, RS-485, LoRa/Wi-Fi module placement — has a proven circuit and a layout rule in
[circuit blocks](references/circuit-blocks.md). Use it, then confirm every value against the
part's datasheet with `read_datasheet` (the datasheet wins). Prefer parts in stock at the
assembly house (`search_jlcpcb_parts`); the boards studied here use common, cheap, available
parts — AP2127K/SPX3819/MIC5219 LDOs, TP4056 and BQ24074 chargers, AO3401A and IRLML6344
MOSFETs, SS34 Schottky, SMAJ TVS, CH340K, W25Q flash.

Design for the mistakes users make: reverse battery, wrong adapter, ESD on the USB port, a
short on an output pin. A product survives each one (section 2 of circuit blocks).

## 3. Rules and net classes (Gate 3)

These are the values the shipped boards use; they are inside every mainstream fab's standard
process, so the board is cheap everywhere:

| Rule | Value |
|---|---|
| Layers / thickness | 2 layers, 1.6 mm (thinner only when the mechanics demand it, e.g. a USB plug edge) |
| Clearance | 0.15–0.2 mm (0.2 default; 0.15 near fine-pitch parts) |
| Signal track | 0.2–0.3 mm (0.15 only on very dense boards) |
| Power track | 0.5 mm for ≤1 A rails, 0.8–1.0 mm for 1–2 A, 2 mm or a pour for battery/motor current — size with `calc_trace_width` |
| Via | 0.6 mm pad / 0.3 mm drill everywhere (13 of 15 boards; route_board's default); 0.8/0.4 for hand-soldered boards and high current, several in parallel |
| Copper to edge | 0.3–0.5 mm |
| USB pair | 90 Ω differential: width and gap from `calc_impedance` for the stackup; route with `route_board` (it couples and length-matches pairs) |

Create the classes before routing (`create_netclass`, `assign_net_to_class`): Default, Power,
and USB/DiffPair at least; route_board then reads widths from `net_widths` and `pair_width`.

## 4. Placement (Gate 4)

- Connectors on edges, facing out; the USB-C receptacle centred on its edge for a clean look.
- One assembly side for machine assembly; bottom side only for tiny boards that cannot fit
  otherwise, and then only small passives there.
- The MCU or module in the middle; antenna of a Wi-Fi/BLE/LoRa module over the board edge with
  the keep-out from its datasheet on every layer.
- Decoupling within 2–3 mm of each power pin; crystal and its load caps within 3 mm of the MCU.
- Power stage (charger, regulator, MOSFETs) grouped near the power input, heat-making parts
  away from sensors (temperature, humidity, IMU).
- Rows of identical parts aligned and evenly spaced; then `smart_place` (mode improve) to
  shorten the ratsnest without breaking the arrangement.

## 5. Copper (Gate 5)

- Ground pour on both outer layers, connected to every ground pad, stitched with vias around
  the board and beside every signal via on a fast net — every studied board pours ground on
  both sides.
- High-current paths as pours, not thin tracks: battery, charger output, motor and LED supply
  (the LiteWing drone pours +BATT and VBUS on both layers).
- `route_board` for the rest; then teardrops on pads and vias (KiCad's Edit menu) — PCBCupid's
  newer boards have them.
- Arcs or 45° corners only; no tracks between passive pads; no thin power stubs longer than a
  few millimetres.

## 6. Silkscreen: the product style

What the studied product boards print, and what Voltaris applies (kicad-pro-silkscreen has
the full rules):

- **Designators hidden** on finished products (all PCBCupid boards hide every reference
  designator) — `polish_board` with `references` of hide. Keep them (tidy) on debug and
  development boards.
- **Function captions instead:** what a block is and does — "FIXED 5V BUCK 2A",
  "RGB INDICATOR", "USB PORT", "DC JACK", "+ SOLAR −", "INPUT 12V-24V", "CHRG", current
  ratings beside current-select jumpers ("0.5 A", "1.0 A").
- **Knockout titles:** product name and section titles as inverted text (`polish_board` prints
  the name block as knockout by default).
- **Pin labels** on every header pin, and a pin table on the bottom
  ("COM A+ B- VAR GND VCC") for boards that mount face down.
- **Configuration notes** where the user needs them: "LR = HIGH [R-CH] / LR = LOW [L-CH]",
  jumper names (INT PULLUP), switch positions (OFF ↔ ON).
- **Identity:** brand, URL, product code, "MADE IN INDIA" (or the real origin), version.
- Sizes: 0.8–1.0 mm for labels; 1.5–2.5 mm for the product name. Smaller text (0.6 mm) appears
  on the studied boards but is below JLCPCB's 0.8 mm minimum — do not copy that.

## 7. Prove it (Gates 6–7)

Product quality is measured, not claimed. Run the [scorecard](references/scorecard.md): DRC
clean, `audit_layout` engineering and beauty grades, `run_design_checks`, the decoupling,
return-via and edge checks, the 3D render review and the cost estimate. Then ship the
[release package](references/release-package.md) — the README with a specs table and a pin
table that matches the silkscreen, schematic PDF, Gerbers, BOM, pick-and-place file, renders,
enclosure and firmware — the way CircuitDigest publishes every open-hardware board.

## What the audits of shipped boards taught

Running `audit_layout` on the 15 boards found the same few issues again and again — avoid
them and the board is ahead of most shipped hardware:

1. Fast nets (crystal, SPI clock, USB) changing layer with no ground via beside the signal via.
2. Decoupling capacitors 4–30 mm from the pin they serve.
3. Vias inside SMD pads without plugging (solder wicks away; order plugged vias or move them).
4. Fast traces within 1 mm of the board edge.
5. Seven or eight different track widths instead of one per net class.
6. Text below the fab's minimum height, and designators sitting on vias.
