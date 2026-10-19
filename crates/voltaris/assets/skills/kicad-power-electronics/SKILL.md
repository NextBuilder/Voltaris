---
name: kicad-power-electronics
description: |
  High-current and high-voltage boards the way Libre Solar (BMS, MPPT chargers), VESC and xESC
  motor controllers and Bitaxe miners build them: current paths as pours on several layers,
  via fields, switching hot loops, gate drive, Kelvin shunt sensing, thermal vias, voltage
  clearances and connectors. Triggers on: "high current", "10 A", "motor controller", "ESC",
  "BLDC", "BMS", "battery management", "MPPT", "solar charger", "inverter", "half bridge",
  "MOSFET", "gate driver", "shunt", "current sense", "busbar", "2 oz copper", "thermal vias",
  "creepage", "48 V", "high voltage".
argument-hint: "[voltage, continuous and peak current, switching frequency]"
---

# Power Electronics Layout

In a power board the copper is a component: its resistance heats the board, its loop area
radiates, and its spacing decides whether it arcs. The boards studied here — Libre Solar's
BMS C1 and MPPT chargers, VESC's BLDC 4, xESC and the Bitaxe miners — all solve the same four
problems the same way. kicad-power-design chooses the parts; this skill lays them out.

```
load_toolset('engineering')      # calc_trace_width calc_via calc_regulator_thermal calc_buck
load_toolset('pcb_routing')      # create_netclass assign_net_to_class add_copper_pour add_via
load_toolset('pcb_board')        # add_zone add_mounting_hole
load_toolset('verification')     # set_design_rules run_drc
load_toolset('pcb_export')       # refill_zones
load_toolset('smart_layout')     # audit_layout
load_toolset('visual_review')    # render_board_3d
```

## 1. Size every current path

1. List each path with its continuous and peak current: battery to switch, switch to load,
   phase outputs, charger input, and the return.
2. Size it: `calc_trace_width(current_a=20, copper_oz=2, temp_rise_c=20)` gives the width; above
   a few amps the answer is wider than a track should be, so the path becomes a **pour**.
3. **Pour the same net on several layers** and join them with a field of vias. Libre Solar's
   BMS C1 pours BAT+, BAT−, PACK+, PACK− and CHG+ on all four layers with about 80 vias on each
   of BAT+ and PACK+; xESC pours its motor supply on three layers and the three phases on both
   outer layers.
4. Count the vias: `calc_via(drill_mm=0.3, current_a=20)` returns how many are needed — roughly
   one per amp for a 0.3 mm via at a 10 °C rise. Spread them along the whole overlap, not in a
   clump at one end.
5. Order heavier copper when the pours cannot fit: 2 oz outer layers are a standard option at
   most fabs (`calc_trace_width` with `copper_oz=2`).
6. Connectors and terminals rated above the peak current, with their pads in the pour (no
   thermal spokes) — XT30/XT60 or screw terminals for batteries and motors.

## 2. Keep the switching loops small

- **Hot loop** (input capacitor → high-side switch → low-side switch → back to the capacitor):
  the pulsed current that radiates. Put the ceramic input capacitors directly across the
  half-bridge, on the same side, and on 4+ layers route the return on the plane straight
  underneath so the loop folds flat.
- **Switch node** (between the switches and the inductor or phase output): wide enough for the
  current, no larger — its edges are the noisiest copper on the board. Nothing sensitive under
  it on any layer.
- **Gate drive**: driver beside its MOSFETs, gate resistor at the gate, gate and source-return
  traces run as a pair; a separate Kelvin source pin goes straight back to the driver.
- **Bootstrap capacitor** at the driver pins, not at the MOSFET.
- `calc_buck` checks the inductor and capacitors before you place them.

## 3. Measure current properly

- **Kelvin connections** on every shunt: the sense traces leave from the inner edges of the
  shunt pads, run together as a pair to the amplifier, and carry no load current. Give them
  their own net names (Libre Solar's MPPT uses SHUNT_DCDC_P) so they cannot merge into the
  power pour.
- Filter (RC) at the amplifier input, not at the shunt.
- Join power ground and signal ground at one point — usually the shunt's ground side or the
  bridge's low-side source — so load current never flows through the controller's ground.
- Keep the amplifier and the MCU's analog inputs on the quiet side of the board, away from the
  switch nodes.

## 4. Get the heat out

- Exposed pads of regulators, drivers and MOSFETs get **thermal vias**: 0.3 mm drill on a 1.0–
  1.2 mm pitch into a ground or source pour on the other side; ask for plugged or tented vias
  so solder stays in the joint.
- Give each hot part the copper area its datasheet's θJA assumes, and check with
  `calc_regulator_thermal`.
- Put hot parts where air moves, away from the battery and any ambient sensor — and put an NTC
  beside them on purpose to measure them: the BMS C1 has TEMP_FETS and TEMP_SHUNT sensors on
  its MOSFETs and shunt.
- Mounting holes near the power stage, so a heatsink or the enclosure can take the heat.

## 5. Clearance by voltage

Spacing between conductors from IPC-2221B (DC or peak voltage, mm):

| Voltage | Inner layers | Outer layers, uncoated | Outer layers, conformal coated |
|---|---|---|---|
| 0–30 V | 0.05 | 0.1 | 0.05 |
| 31–100 V | 0.1 | 0.6 | 0.13 |
| 101–150 V | 0.2 | 0.6 | 0.4 |
| 151–300 V | 0.2 | 1.25 | 0.4 |
| 301–500 V | 0.25 | 2.5 | 0.8 |

- Put the high-voltage nets in their own net class (`create_netclass`) with this clearance —
  Libre Solar's BMS C1 assigns BAT+, CHG+, FUSE and PACK+ to an HV class at 0.3 mm.
- Mains and anything a person can touch follows its safety standard's creepage and
  clearance, not this table; cut slots (Edge.Cuts) under isolation barriers to add creepage.
- Keep fuses, TVS diodes and reverse-polarity protection at the input connector.

## 6. Check it

- `audit_layout` thin-power check: every remaining power track gets a `calc_trace_width`
  answer; poured nets are skipped because the pour carries the current.
- `run_drc` with the HV class clearance; refill zones first (`refill_zones`).
- `render_board_3d` both sides: pours continuous, via fields spread, no sensitive traces
  under switch nodes.
- Test points on the supply, each phase or switch node, and the shunt sense pair.

## Done when

Every path has a sized width or a multi-layer pour with a counted via field, hot loops and gate
loops are folded tight, shunts are Kelvin-connected, exposed pads have thermal vias, HV nets
have their clearance class, and DRC passes after a zone refill.
