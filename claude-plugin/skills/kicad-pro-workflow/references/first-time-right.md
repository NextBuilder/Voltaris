# First time right: lessons from our own boards

Every row is something that had to be fixed or redone on Festix, Mini UPS V2 or the Scribe main
board. The rule beside it prevents it, and the last column names the check that catches it.
Apply these at the gate shown — not at review time.

## Schematic (Gates 1–2)

| What went wrong | Rule | Caught by |
|---|---|---|
| Scribe's first schematic put a net label on almost every pin (290 labels, 4 wires) and had to be rebuilt | Draw it presentable the first time: numbered, titled blocks ("1 │ POWER INPUT — USB-C PD 20 V (CH224A)"), a one-line note under each block with the key values, power symbols (GND, +3V3, +5V, VM) at every power pin, short wires between parts in one block, labels only for signals that leave a block | `audit_power_rails` presentation findings |
| Mini UPS rev A fitted two 0 Ω option links (R48, R49) that met on one net, tying both outputs together | An option link is DNP unless chosen; never two fitted links feeding one net from two sources | `audit_power_rails` (0 Ω links both fitted) |
| A 0402 0 Ω link carried 3 A | A link carries its net's current: 0402 ≈ 1 A, 0603 ≈ 1–2 A, 1206 ≈ 2–3 A, or replace it with copper | Gate 6 review, `calc_trace_width` |
| 19 parts carried a stale "DNP" text field while their real DNP flag was off; the BOM disagreed with the board | Use KiCad's DNP flag only, never a text field; regenerate the BOM after every value change | Pre-order BOM = schematic = pick-and-place check |

## Part ratings (Gate 1)

| What went wrong | Rule | Caught by |
|---|---|---|
| Boost inductors with Isat 10 A under a converter whose switch current limit is 13 A | Inductor Isat ≥ the IC's peak current limit (not the load current); rated current ≥ worst average; check DCR loss | `calc_buck`, Gate 6 |
| Battery fuse breaking capacity 150 A against a ~270 A cell short | Fuse breaking capacity ≥ the prospective short-circuit current (cell voltage ÷ cell + wiring resistance) | Gate 6 |
| BQ77915 gate resistors not the datasheet values for the chosen recovery mode | Every value around a protection or power IC comes from its datasheet table for the chosen mode — record the page in `.voltaris/project.json` | `read_datasheet`, `run_design_checks` |
| Top-cell sense and VDD taken after the fuse, reading ~90 mV low under load | Sense and reference connections come from the point they measure (before fuses, shunts, switches) | Gate 6 |
| Both ports could draw 120 W from a 60 W battery path | Write the power budget per source before choosing outputs; firmware limits go in the bring-up guide | Gate 0 brief |

## Placement (Gate 4)

| What went wrong | Rule | Caught by |
|---|---|---|
| Scribe was placed on a 100 × 66 mm board, then 64 × 45, then 60 × 44 | Size the board from the parts: sum the courtyards, aim for 45–60 % fill on a product board, round up to the enclosure's grid — then lock the outline before placing | `smart_place` report, Gate 0 |
| Parts dropped where the netlist import left them, scattered with empty areas | Place by function: connectors on edges first, then each IC with its own passives as a block, identical channels as copies of one block (Motor A = Motor B) | `smart_place`, `score_placement` |
| Labels had no room next to the connectors | Keep a label lane beside every connector | `smart_place` `label_lane` (default 2.6 mm) |
| Inductors L2/L3 sat on the boot capacitors, hidden by DRC exclusions | Never exclude a courtyard overlap; move the part | `audit_layout` courtyard check (ignores exclusions) |
| Charger bulk capacitors 8 mm from the IC | The switching loop capacitors go at the pins in the datasheet's order — before any other part of that block | `audit_layout` decoupling check |

## Routing and copper (Gate 5)

| What went wrong | Rule | Caught by |
|---|---|---|
| Current-sense inputs tapped the power pour, measuring 20 mΩ of copper with a 10 mΩ shunt | Kelvin sense: a separate pair from the inner edges of the shunt pads to the amplifier | kicad-power-electronics; Gate 6 |
| 15 GND pads were left isolated by the pours | After pouring, every ground pad gets a via to the ground plane beside it | `audit_layout` unrouted (pours counted), DRC unconnected |
| About 110 vias ended inside SMD pads | Keep vias beside pads on a standard order; if dense parts need via-in-pad, order filled and capped vias (IPC-4761 VII) and say so in the fab notes | `audit_layout` via-in-pad check |

## Silkscreen (Gate 5)

| What went wrong | Rule | Caught by |
|---|---|---|
| Every label was drawn as SVG artwork and imported one by one (~30 files per board) | Labels are KiCad text and frames from `polish_board` (product style, `captions`); only the logo is artwork | `audit_layout` beauty |
| Captions and pin legends redone after review | Decide captions with the user before placement and reserve lanes | kicad-pro-silkscreen house style |

## Tools and process

| What went wrong | Rule |
|---|---|
| Snapshots taken while the board was open in KiCad saved the stale file | Fixed in Voltaris: `snapshot_design` now saves the live board; still take one before every large change |
| Credits spent redoing whole stages | Decide form factor, captions and layer count at Gate 0; check each gate's evidence before moving on |
| Review findings fixed after the PCB was "done" | Run the expert checklist at each gate it belongs to, not only at Gate 6 |
