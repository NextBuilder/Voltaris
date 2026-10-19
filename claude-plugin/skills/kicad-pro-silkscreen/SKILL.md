---
name: kicad-pro-silkscreen
description: |
  Product-grade silkscreen like Arduino, Adafruit, SparkFun, Seeed Studio, Waveshare and DFRobot
  boards: one designator size, upright text beside each part and off copper, aligned rows,
  polarity and pin-1 marks, pin and interface labels on both sides, a product block and a
  bottom info block, within JLCPCB's printing limits. Triggers on: "silkscreen", "silk",
  "designators", "reference designators", "labels", "pin labels", "polarity marks", "pin 1",
  "logo", "product look", "clean board", "professional silkscreen", "like Waveshare",
  "like Arduino", "like Seeed", "like DFRobot", "like Adafruit", "like SparkFun".
---

# Professional Product Silkscreen

On a product board the silkscreen is the user interface: it says what the board is, which way
it goes, what every pin does and where every part sits — and nothing else. It is quiet, aligned,
one size per level, and never touches copper. This skill sets the rules; the engines apply most
of them in one call.

```
load_toolset('smart_layout')     # polish_board audit_layout
load_toolset('pcb_board')        # add_board_text import_svg_logo
load_toolset('visual_review')    # render_board_3d
load_toolset('verification')     # run_drc
```

## The fast path

1. `polish_board(board, title, version, back_text, references, ref_size)` — dry run first.
   With the default `references` of tidy it puts every reference designator at one size
   (1.0 mm, 0.8 mm where space is tight), upright, beside its own part, off pads, vias, other
   parts' bodies and other silkscreen; rows and columns of identical parts get their
   designators on the same side; a designator with no clean spot is hidden (it stays in the BOM
   and the assembly drawing) and named in the notes. It also adds connector pin labels on both
   sides, the name and version block, the bottom brand line, the mirrored ground pour and
   stitching vias. Use `references` of hide for a display board with no designators, keep to
   leave them alone.
2. Review the dry run: hidden designators, notes, then apply with its plan_revision.
3. `audit_layout` — its beauty findings now include the silkscreen checks:
   designators on copper, overlapping, upside down, too small, far from their part, and mixed
   sizes. Fix each with the tool it names until the beauty grade is A.
4. `run_drc`: no silk-over-copper, silk-overlap or silk-clipped warnings.
5. `render_board_3d(views=['top', 'bottom'], quality='high')` and read it like a customer would.

## Printing limits (JLCPCB)

| | Minimum | Use |
|---|---|---|
| Text height | 0.8 mm | 1.0 mm designators and pin labels, 1.2–2.5 mm product name |
| Stroke (line) width | 0.15 mm | height ÷ 6, never under 0.15 mm |
| Width-to-height ratio | 1:5 | 1:6 |
| Gap to pads / mask openings | 0.15 mm (trimmed by the fab inside this) | 0.2 mm or more |
| Gap to board edge | — | 0.3 mm or more |

Text under the minimum prints as a blur; text closer than 0.15 mm to a pad is cut away by the
fab, leaving broken labels. Use KiCad's default stroke font — it is what the fab expects.

## Three levels, one size each

| Level | What | Size |
|---|---|---|
| Product | Name, version ("v1.0"), logo, URL, date, OSHW mark | 1.2–2.5 mm |
| Function | Pin labels (5V, 3V3, GND, SDA, SCL, TX, RX, D0…), connector, button and LED names, "+"/"−" at power inputs, jumper labels | 0.8–1.2 mm |
| Reference | R1, C3, U2 — one size for the whole board | 0.8–1.0 mm |

Never mix sizes inside a level. A board with three designator sizes looks hand-made.

## Reference designators

- Beside the part, never under its body, never on a pad, via or test point.
- Upright: reads left to right, or bottom to top. Never upside down, never at odd angles.
- Rows and columns of identical parts (resistor arrays, LED strings, decoupling rows) put
  every designator on the same side, at the same offset.
- One convention for the board: above wide parts, left of tall parts.
- On dense boards hide designators rather than squeeze them; the assembly drawing (F.Fab) and
  the BOM still carry them. Never shrink below 0.8 mm to make them fit.

## Polarity and pin 1 — must survive assembly

Each mark must stay visible after the part is soldered — outside the body, not under it.

| Part | Mark |
|---|---|
| ICs, modules | Pin-1 dot or chamfer outside the body corner |
| Diodes, LEDs | Cathode bar or "K"/triangle beside the cathode pad, consistent direction within a group |
| Electrolytic and tantalum caps | "+" beside the positive pad |
| Connectors | Pin-1 mark; power inputs get "+" and "−" (or VIN/GND) |
| Batteries | "+" and "−" at both terminals, and the cell type |
| Switches, jumpers | Function (RESET, BOOT) and the default position or "cut to disable" |

KiCad's standard footprints already carry most of these on their silkscreen. Check each one in
the top render; add a mark with `add_board_text` where a footprint has none.

## Pins and interfaces — the brand look

- Every connector pin labelled, on both sides of a breakout or module (the user sees the
  bottom when it sits in a breadboard). `polish_board` does this from the net names, so name
  your nets the way the label should read: 3V3, 5V, GND, SDA, SCL, TX, RX.
- Group headers by function with a short caption — Arduino boards print POWER, ANALOG IN and
  DIGITAL (PWM~), with "~" marking PWM pins; do the same for your own groups.
- Interfaces named where they leave the board: USB, I2C, UART, the Grove or Qwiic port.
- Inverted (knockout) labels — white box, text cut out — are KiCad's text "Knockout" property;
  use them for one or two key labels, not everywhere.
- See [brand styles](references/brand-styles.md) for what each maker brand does.

## Product block and bottom side

- Top: product name and version together, in a clear area near an edge; the logo
  (`import_svg_logo`) beside it, never across copper features.
- Bottom (mirrored): brand or URL, date or revision code, the OSHW mark if the design is open,
  and a short pin table if the top is too dense for labels.
- Regulatory marks (CE, FCC, UKCA) only when the product is actually certified.

## Clean-up rules

- Nothing on pads, in via holes or in mask openings; nothing across the board edge.
- Nothing under parts — it is invisible after assembly and lifts parts on reflow.
- No lines between the two pads of a small passive.
- Keep silkscreen off fast traces and RF lines.
- Delete stray library text (duplicate values, "REF**", user notes) from F.SilkS.

## Review checklist

- [ ] One designator size, upright, beside each part, none on copper, rows consistent
- [ ] Every connector pin labelled on both sides; power inputs marked "+"/"−"
- [ ] Polarity and pin-1 marks visible after assembly on every polar part
- [ ] Name, version and logo on top; brand/URL/date on the bottom
- [ ] `audit_layout` beauty grade A; `run_drc` clean of silkscreen warnings
- [ ] Top and bottom renders read like a finished product
