---
name: kicad-board-craft
description: |
  Make boards look and feel like products from Arduino, Seeed Studio, Adafruit, SparkFun,
  DFRobot and Waveshare: clean outlines, standard form factors, mounting holes, connector
  standards (Qwiic/STEMMA QT, Grove, Gravity), pin labels on both sides, silkscreen hierarchy,
  branding and a beauty review. Triggers on: "beautiful board", "clean PCB", "professional
  looking", "like Adafruit", "like Seeed", "like Arduino", "like Waveshare", "like DFRobot",
  "breakout board", "silkscreen", "pin labels", "logo", "Qwiic", "STEMMA QT", "Grove",
  "Gravity", "Feather", "XIAO", "HAT", "form factor", "mounting holes".
argument-hint: "[board type or style to match]"
---

# Board Craft — Maker-Grade Boards

The boards people love from Arduino, Seeed Studio, Adafruit, SparkFun, DFRobot and Waveshare
share one idea: **the board explains itself.** Anyone holding it can tell what it is, which way
it plugs in, what every pin does and which version it is, without opening a datasheet. Electrical
correctness comes first (kicad-pcb, kicad-pro-layout); this skill is the finish that turns a
working board into a product.

```
load_toolset('pcb_board')        # set_board_size add_board_outline add_mounting_hole add_board_text import_svg_logo
load_toolset('pcb_components')   # move_component rotate_component align_components get_component_pads
load_toolset('placement')        # score_placement
load_toolset('visual_review')    # render_board_3d
load_toolset('smart_layout')     # polish_board audit_layout
```

`polish_board` applies most of this skill in one call — pin labels on both sides, the name and
version block, a bottom brand line, the ground pour on both outer layers and stitching vias —
and `audit_layout` scores the result. Use the sections below to decide the style and to fix
what the engines report.

Decide the style at Gate 0 of kicad-pro-workflow, with the user, using suggested options:
form factor, mask colour and finish, connector standard, and whether labels go on both sides.

## 1. Outline and form factor

- Start from a known form factor when the board joins an ecosystem — see
  [form factors](references/form-factors.md) for exact sizes and hole positions (Arduino Uno and
  Nano, Raspberry Pi HAT, Pico, Feather, XIAO and others).
- Otherwise size the board on a **2.54 mm (0.1") grid** and round the corners: 1–2 mm radius for
  small boards, 3 mm for anything you hold in a hand. `add_board_outline` takes the radius.
- Every connector sits **on an edge, facing out**, flush or overhanging by the amount its
  datasheet shows. A USB-C receptacle overhangs about 0.5–1 mm on most mid-mount parts — check
  the drawing.
- Keep a 0.5 mm copper-free border along the outline (fab routing tolerance), and 1 mm or more
  around V-score lines when panelising.

## 2. Mounting holes

- Holes go **in the corners**, centred on the 2.54 mm grid, the same distance from both edges.
  Pick one size per product family: M2 (2.2 mm drill) for tiny boards, M2.5 (2.7 mm) for
  Raspberry Pi and Adafruit-style boards, M3 (3.2 mm) for anything bolted to an enclosure.
- Place them with `add_mounting_hole` before any other part, then keep a clear ring for the
  screw head and washer: 5 mm diameter for M2.5, 6 mm for M3, on both sides.
- A ground ring around plated holes is a nice touch for ESD and enclosures; NPTH is fine when
  the enclosure is plastic.

## 3. Placement that reads well

- **One component side.** Every SMD part on top unless the board is too small; it is cheaper to
  assemble and looks deliberate. Put the hand-solder parts (headers, terminals) on top too.
- **Align and repeat.** Passives in the same row share one orientation and one centre line
  (`align_components`). Resistor and capacitor rows look like rows. Values read left to right
  or bottom to top, never upside down.
- **Signal flow left to right** (or input edge to output edge): power and input connectors on
  one side, the brain in the middle, outputs on the far side. The layout then matches the
  schematic and the silkscreen story.
- **Status LEDs** (power, charge, user) sit together near an edge where they are visible in the
  enclosure, each labelled. A power LED next to the power input is standard on every
  Arduino-class board.
- **Buttons and switches** at the edge, labelled with their function (RESET, BOOT, USER), with
  enough clearance for a finger.
- Re-run `score_placement` after styling moves — beauty never costs a hard failure.

## 4. Connector standards

Use the ecosystem connector instead of a generic header whenever the board talks I2C or plugs
into a sensor system — this is the single biggest "pro" signal on a maker board. Pinouts, wire
colours and KiCad footprints are in [connector standards](references/connector-standards.md):

- **Qwiic / STEMMA QT** (SparkFun / Adafruit): JST SH 1.0 mm, 4 pins, 3.3 V I2C. Put two on
  the board (in and out) on opposite edges so boards daisy-chain.
- **Grove** (Seeed Studio): 2.0 mm 4-pin; the same connector carries I2C, UART, digital or
  analog depending on the port.
- **Gravity** (DFRobot): PH2.0 connectors with colour-coded headers; keep the module's pin
  order and label every pin.
- **0.1" headers**: breadboard-friendly boards keep rows on the 2.54 mm grid with an even
  spacing between rows (multiples of 2.54 mm) and leave at least one free breadboard row on each
  side of the board.

## 5. Silkscreen hierarchy

Silkscreen is the user interface. The kicad-pro-silkscreen skill has the full rules and the
brand styles; `polish_board` tidies every reference designator (one size, upright, beside its
part, off copper, rows consistent). Write it in three levels and keep it consistent:

| Level | Content | Size (text height / line) |
|---|---|---|
| Product | Product name, version (e.g. "v1.0"), brand or logo, optional URL | 1.5–2.5 mm / 0.2–0.3 mm |
| Function | Pin labels (5V, GND, SDA, SCL, TX, RX, D0…), connector names, button names, LED names, "+" and "−" at power inputs | 0.8–1.2 mm / 0.15 mm |
| Reference | Designators (R1, C3) — only where they fit cleanly; hide them on dense boards and rely on the assembly drawing | 0.8 mm / 0.15 mm |

Rules that keep it clean:
- Never put silkscreen on pads, vias in the mask opening, or under a part body. KiCad's DRC
  reports silk over copper and silk overlap — fix every one; do not leave warnings behind.
- Minimum text height 0.8 mm and line width 0.15 mm at JLCPCB-class fabs; prefer 1.0 mm for pin
  labels people will read.
- Two orientations only: text reads left to right, or bottom to top. Never upside down.
- **Label every pin of every connector on both sides of the board** when it is a breakout or
  module — the user sees the bottom when the board is in a breadboard. Bottom-side text is
  mirrored automatically on B.SilkS.
- Polarity is unmistakable: "+" by the positive terminal, a cathode bar or dot by diodes and LEDs,
  pin-1 dot by every IC and connector, a polarity outline for electrolytics.
- Board name and version on top, company/URL/date on the bottom, together with an
  "Open Source Hardware" mark if the project is open.
- Logos with `import_svg_logo`: monochrome SVG, no thin hairlines (≥ 0.15 mm), placed in empty
  space, never across copper features.

## 6. Copper that looks designed

What makes Adafruit and Seeed boards look expensive is mostly copper discipline (the
kicad-pro-layout skill has the electrical reasons):
- 45° bends or smooth arcs, no acute angles, no random doglegs.
- One track width per net class; widths change only at a pad (neck-down) or for current.
- Buses and pairs run parallel with even spacing, turning together.
- Solid ground pours on both outer layers on 2-layer boards, stitched with vias on a regular
  grid (every 5–10 mm) and along the edges; no isolated copper islands.
- Teardrops on pad and via entries (KiCad 10 adds them under Edit → Add Teardrops) — they look
  finished and survive drilling tolerance.
- Vias on a grid, tented where they are not test points.

## 7. Colour and finish

- Pick one mask colour per product family and keep it: e.g. SparkFun red, Adafruit black,
  Arduino teal-blue, Raspberry Pi green, Waveshare blue. White silkscreen on dark masks; black
  silkscreen on white or yellow masks.
- **ENIG** (gold) for fine-pitch parts, edge connectors, castellated holes and anything people
  will touch; **HASL lead-free** is fine for simple through-hole boards.
- Black and white masks hide traces; use them when the board is a display piece, green or blue
  when someone will debug it.

## 8. Beauty review

Run `audit_layout` first: its beauty score covers off-angle tracks, width sprawl, unlabelled
connectors, the name block, corners, mounting holes, stitching and part rotation.

Before Gate 6 of kicad-pro-workflow, render and inspect:

```
render_board_3d(views=['top', 'bottom', 'iso'], quality='high', floor=true)
```

Check, and fix each finding before the electrical review:
- [ ] Outline on the 2.54 mm grid, corners rounded, holes in the corners on the grid
- [ ] Every connector on an edge, facing out, with a label for each pin on both sides
- [ ] Product name, version, brand/logo on top; URL/date/OSHW on the bottom
- [ ] Rows of passives aligned, one orientation per row, no rotated stragglers
- [ ] LEDs grouped and labelled; buttons labelled; polarity marks visible on every polar part
- [ ] No silkscreen on pads or under bodies; DRC shows no silk warnings
- [ ] Copper: 45° bends, consistent widths, ground pour with stitching, no islands
- [ ] Mask colour and finish chosen and recorded for the order

Report the review as a short checklist with the render views attached.
