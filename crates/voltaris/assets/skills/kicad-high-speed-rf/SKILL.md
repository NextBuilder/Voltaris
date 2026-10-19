---
name: kicad-high-speed-rf
description: |
  Controlled impedance, stackup, USB, Ethernet and other differential pairs, length matching,
  RF feeds and antennas, crystals, grounding and EMC for KiCad boards. Triggers on: "impedance",
  "50 ohm", "90 ohm", "USB routing", "differential pair", "length matching", "skew", "antenna",
  "RF", "Wi-Fi range", "EMC", "EMI", "crystal layout", "stackup", "4-layer".
argument-hint: "[interfaces and board layer count]"
---

# High-Speed and RF Layout

```
load_toolset('engineering')      # calc_impedance list_stackups calc_via
load_toolset('pcb_routing')      # route_differential_pair create_netclass assign_net_to_class add_via
load_toolset('signal_integrity') # analyze_net_lengths
load_toolset('visual_review')    # render_board_3d
load_toolset('smart_layout')     # route_board
```

## Stackup first

Choose the stackup before routing, and order the board with the same one. On JLCPCB use one of
the impedance templates from `list_stackups`; on 4 layers the classic choice is
signal / ground / power / signal, with every high-speed trace on an outer layer directly over
the solid ground layer. Two-layer boards can still carry USB 2.0 and 2.4 GHz RF if the bottom
layer under those traces is solid ground — but the dielectric is thick, so 50 Ω traces are wide
and coplanar waveguide (CPWG) is usually the better structure.

Solve every controlled trace with `calc_impedance` and store the widths and gaps as net classes
(`create_netclass`, `assign_net_to_class`) so the router enforces them.

## Interfaces

| Interface | Target | Rules |
|---|---|---|
| USB 2.0 Full/High Speed | 90 Ω differential | length-match within a fraction of a millimetre, no stubs, ESD next to the connector, series resistors (if fitted) next to the MCU |
| 10/100 Ethernet | 100 Ω differential | magnetics and connector per PHY datasheet, keep pairs away from board edge |
| 2.4 GHz RF feed (u.FL, chip antenna) | 50 Ω (CPWG on 2/4-layer) | shortest path, ground stitching vias both sides every few millimetres, matching-network footprints at the antenna |
| Crystals | — | load caps beside the crystal, short equal traces, ground guard, no signals under them |
| SPI / SDIO above ~20 MHz | — | series termination at the source, keep lengths matched within a group |

`route_board` routes every pair it detects (D+/D-, DP/DM, _P/_N, or the `pairs` you name) first,
as a coupled pair at `pair_gap` with `pair_width` from `calc_impedance`, then adds 45° tuning
bumps to the shorter net until the skew is inside `pair_skew` (0.1 mm by default). Its dry run
reports coupling and skew per pair.

After routing, run `analyze_net_lengths`: it finds polarity pairs automatically and reports
skew in millimetres and picoseconds plus via-count mismatches. Use the εeff from
`calc_impedance` for accurate delay. Fix any remaining skew with KiCad's interactive length
tuner (meanders near the mismatched end), then re-run.

## Antennas

- Module PCB antennas: place the antenna at or over the board edge, with **no copper, traces,
  components or screws under it on any layer**, exactly as the module datasheet's keep-out
  drawing shows. Keep metal enclosures and batteries away from it.
- Chip antennas: follow the vendor layout exactly, including ground clearance and the
  matching network.
- Check with `render_board_3d` that the keep-out really is empty on every layer.

## Grounding and EMC

The placement, return-path and routing method behind these rules is in the kicad-pro-layout
skill; this section lists what matters most for fast signals.


- One continuous ground plane; never route a fast signal across a split or slot.
- Every layer change of a fast signal needs a ground via within about 1 mm of the signal via so
  the return current can follow.
- Keep fast traces at least 3× their width from other signals, and several millimetres in
  from the board edge.
- Route clocks and fast edges on the layer next to the ground plane, never on a layer whose
  reference is a split power pour.
- Stitch ground pours together around the board edge and around RF.
- Keep fast edges, clocks and switching regulators away from connectors and cables, which are
  the antennas that radiate.

## Done when

Stackup chosen and documented, controlled widths solved and in net classes, `analyze_net_lengths`
verdict pass, antenna keep-outs empty, and a visual review of the RF and USB areas.
See [the impedance quick guide](references/impedance-guide.md) for typical structures.
