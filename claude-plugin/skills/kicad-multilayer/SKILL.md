---
name: kicad-multilayer
description: |
  4-, 6- and 8-layer boards the way the best open-hardware teams build them (Great Scott Gadgets,
  Glasgow, Antmicro, OrangeCrab, ULX3S): choosing the layer count, stackups measured from shipped
  boards, plane assignment, reference planes and layer changes, fine vias and microvias, BGA
  fan-out, DDR and high-speed buses, both-side assembly and what to tell the fab. Triggers on:
  "4-layer", "6-layer", "8-layer", "multilayer", "stackup", "inner layer", "power plane",
  "ground plane", "BGA", "FPGA board", "DDR", "LPDDR", "HDI", "microvia", "blind via",
  "via in pad", "dense board", "both sides".
---

# Multilayer Boards

The 21 multilayer boards measured for this skill (see the
[stackup catalog](references/stackup-catalog.md)) share three habits: a solid ground plane next
to the layers that carry fast signals, power on its own planes or pours, and fine vias spent
freely. The numbers below are what those teams used, not textbook ranges.

```
load_toolset('engineering')      # list_stackups calc_impedance calc_via calc_trace_width
load_toolset('pcb_board')        # add_layer add_zone
load_toolset('pcb_routing')      # create_netclass assign_net_to_class add_copper_pour add_via
load_toolset('placement')        # plan_bga_fanout
load_toolset('verification')     # set_design_rules set_layer_constraints run_drc
load_toolset('pcb_export')       # refill_zones
load_toolset('signal_integrity') # analyze_net_lengths
load_toolset('smart_layout')     # route_board audit_layout
```

## 1. Choose the layer count

| The board has | Layers | Seen on |
|---|---|---|
| A QFN MCU, USB 2.0, a radio, one or two regulators | 2 is enough if the bottom stays solid ground; 4 if it must pass EMC first time | OLIMEX ESP32 boards, Framework expansion card, Solder Party keyboard |
| USB High Speed, Ethernet, several bucks, radio plus analog, 0.5 mm QFN-56+ | 4 | HackRF, Bitaxe, CatSniffer, ESP32-PoE, ULX3S |
| An FPGA or MCU in BGA, a USB 3 / HDMI / MIPI link, many rails | 6 | Cynthion, Glasgow, OrangeCrab |
| A SoM or SoC with DDR, PCIe, many high-speed lanes | 8 or more | Antmicro Kria and Jetson baseboards, LPDDR4 test board |

Moving from 4 to 6 layers usually costs less than the respin a crowded 4-layer board causes.

## 2. Stackups that shipped

Outer to outer. "Signal" layers also carry ground pours stitched to the planes.

| Layers | Stackup | Who |
|---|---|---|
| 4 | Signal / **GND** / power / signal | HackRF, Bitaxe, Hades, CatSniffer, Antmicro sensor and HDMI bridge |
| 4 | Signal / **GND** / **GND** / signal | Castor & Pollux, Antmicro signal-integrity board — both outer layers get a plane each |
| 4 | Signal / power / **GND** / signal | ULX3S — the bottom routes against ground, the top against power |
| 6 | Signal / **GND** / signal+power / power / **GND** / signal | Cynthion |
| 8 | Signal / **GND** / signal+power / **GND** / signal+power / **GND** / power / signal | Antmicro Kria baseboard — ground on every other layer |
| 8 | Signal / **GND** / **GND** / **GND** / power / power / power / signal | Antmicro LPDDR4 test board |

Rules behind them:

- **Every routing layer sits beside a ground plane.** A layer whose only neighbour is a split
  power plane gives its signals a broken return path.
- **Power planes sit beside a ground plane**, close together: the pair is a capacitor that
  decouples at frequencies the capacitors cannot reach.
- **Symmetric about the centre** (copper weights and dielectric), so the board stays flat
  through reflow.
- Order the board with the fab's stackup (`list_stackups`) and solve every controlled width on
  it with `calc_impedance`. Antmicro names each net class after its impedance and interface, so
  the class says what the trace is:

  | Net class (Antmicro) | Width / gap (mm) | Board |
  |---|---|---|
  | 90Ohm-diff_USB | 0.182 / 0.196 | environment sensor, 4 layers |
  | 100Ohm-diff-HDMI, 100Ohm-diff-CSI | 0.1 / 0.1 | HDMI–MIPI bridge, 0.9 mm 4 layers |
  | 85Ohm-diff_PCIE, 85Ohm-diff_USB_SS, 100Ohm-diff_MDI | from the stackup | Jetson Orin baseboard, 8 layers |
  | 50Ohm-se_LPDDR4_DQ, 80Ohm-diff_LPDDR4_DQS | 0.182; 0.1 / 0.182 | LPDDR4 test board, 8 layers |

  Their default clearance is 0.1–0.15 mm and default track 0.1–0.15 mm on these boards.
- Thickness follows the connectors and the fab: 1.6 mm for most; the Antmicro boards ship at
  0.9–1.4 mm to make thin dielectrics and narrow 50 Ω traces possible.

## 3. Reference planes and layer changes

A signal's return current flows in the plane next to it, directly under the trace. When the
signal changes layer the return must change plane too:

- Changing between two layers that **share** a ground plane (top to layer 3 around a layer-2
  ground) needs no extra via: the return stays on that plane.
- Changing between layers referenced to **two different ground planes** needs a ground
  stitching via close by; on boards with hundreds of ground vias the nearest one is usually
  within 2–3 mm.
- Changing from a ground-referenced layer to a **power-referenced** layer needs a ground via
  and a stitching capacitor between the planes near the transition — or avoid it.

`audit_layout` applies exactly this: its return-via check skips layer changes that share a
plane, uses a 2.5 mm radius and advice severity between two ground planes, and warns at
`return_via_mm` (1 mm) everywhere else.

## 4. Vias

| Board | Via pad / drill (mm) |
|---|---|
| 4-layer maker and product boards | 0.6 / 0.3, 0.8 / 0.4 for power |
| 4-layer dense (ULX3S, xESC, Antmicro) | 0.42–0.55 / 0.2–0.25 |
| 6-layer FPGA (Cynthion, OrangeCrab, Glasgow) | 0.46 / 0.2, 0.3 / 0.15, 0.3 / 0.2 (Glasgow uses 1500 of them) |
| 8-layer SoM baseboards (Antmicro) | 0.45 / 0.1 laser microvias for fan-out, 0.5–0.6 / 0.2–0.25 through vias |

- Check every new via size with `calc_via` for current and with the fab's capability table
  before using it; 0.15 mm mechanical drills and 0.1 mm laser vias are advanced options.
- **Via in pad** is normal on dense boards (Glasgow has hundreds, Cynthion and the Antmicro
  boards dozens) — but only ordered as filled and capped (IPC-4761 type VII). On a standard
  order a via in a pad wicks solder away.
- Back-drill or use blind vias only when a stub really hurts (multi-gigabit links); for USB 2,
  Ethernet and DDR3/4 at maker speeds a through via is fine.

## 5. Fan-out, buses and both sides

- **BGA fan-out first**, before any other routing: `plan_bga_fanout` proposes dog-bone or
  in-pad vias per ball; outer rows escape on the top layer, inner rows drop to inner layers.
- **Decoupling under the BGA** on the bottom side, one capacitor per via pair — this is why the
  6- and 8-layer boards put a third to three quarters of their parts on the bottom
  (Cynthion 253 of 348, Glasgow 626 of 782).
- DDR and parallel buses: route each byte lane on one layer pair, match lengths inside a lane
  with `analyze_net_lengths`, keep address/command as a fly-by group, and give the bus an
  unbroken ground reference over its whole length.
- Differential lanes (USB 3, HDMI, MIPI, PCIe): `route_board` with `pairs` couples and tunes
  them; keep pairs on layers beside ground and change layer only where a ground via sits
  beside the pair.
- Curved routing is a mark of these boards: the Jetson baseboard carries 1720 arcs, Castor &
  Pollux 828. Arcs avoid impedance steps at corners on fast lanes.

## 6. Tell the fab

Write the stackup, impedance list (layer, width, gap, target), via types and fill, finish
(ENIG on HackRF, Glasgow and the Antmicro baseboards — flat pads for fine pitch and BGA), and both-side assembly into the
fabrication notes. `validate_for_manufacturing` checks the rest.

## Done when

The stackup is chosen with every routing layer beside ground, controlled widths are in
impedance-named net classes, BGA fan-out and decoupling are placed, `audit_layout` shows no
return-via warnings, and the fab notes list stackup, impedances and via fill.
