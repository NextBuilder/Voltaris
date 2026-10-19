---
name: kicad-power-design
description: |
  Power-tree design for KiCad boards: choosing LDO vs buck, thermal limits, input protection,
  battery charging, protection and power paths, decoupling and power layout. Triggers on:
  "power supply", "regulator", "LDO", "buck", "battery", "Li-ion", "18650", "LiPo", "charging",
  "USB power", "power budget", "it gets hot", "brownout", "reverse polarity".
argument-hint: "[rails, loads and power source]"
---

# Power Design

Most first-revision board failures are power failures: a regulator that overheats, a rail
that sags during radio bursts, a battery that can be over-discharged. Work the power tree
before anything else on the board.

```
load_toolset('engineering')      # calc_regulator_thermal calc_buck calc_trace_width calc_via calc_passive
load_toolset('simulation')       # simulate_circuit run_ngspice
load_toolset('templates')        # search_templates apply_template
```

## 1. Budget every rail

For each rail list every load with its **peak** and **average** current, and the minimum and
maximum input voltage. Radios set the peak: ESP32 Wi-Fi transmit bursts reach several hundred
milliamps, so budget at least 500 mA on its 3.3 V rail. Add 30% margin to the peak.

## 2. Choose the regulator

| Situation | Choice |
|---|---|
| (Vin − Vout) × Iload stays under the package's limit at the hottest ambient | LDO: quiet, cheap, few parts |
| Dissipation too high, or battery life matters | Buck (step-down) |
| Vout above Vin (e.g. 5 V from one Li-ion cell) | Boost |
| Vin crosses Vout (single cell to 3.3 V at the end of discharge) | Buck-boost, or an LDO with low dropout accepting the last few percent of capacity |

Always run `calc_regulator_thermal` with the datasheet θJA for the copper area you will give
the part. For bucks, start from the datasheet's recommended inductor and capacitors and check
them with `calc_buck`; never guess values.

## 3. Protect the input

- External DC input: reverse-polarity protection (P-MOSFET ideal diode preferred to a series
  Schottky when current is high), a fuse or polyfuse, a TVS at the connector.
- USB: polyfuse, ESD array, CC resistors (templates provide a verified USB-C port).
- Batteries: never without protection (over-charge, over-discharge, over-current, short) —
  either a protected cell or a protection IC on the board.

## 4. Batteries

- Charge current ≤ the cell's rated charge rate (often 0.5–1C); program it from the charger's
  datasheet table.
- Use a temperature sensor input (NTC) on products that charge unattended.
- A load-sharing power path lets the board run while charging and stops the system rail from
  feeding the cell directly.
- Measure battery voltage through a high-value divider with a capacitor at the ADC pin
  (`calc_divider`; check the ADC's input impedance limit).

Templates exist for a TP4056 charger, DW01A protection and a USB/battery power path — apply
them, then adapt values to the cell.

## 5. Decoupling

Follow each IC's datasheet first. Otherwise: one 100 nF per supply pin placed at the pin, plus
bulk (10–22 µF) per IC or rail branch. Ceramic capacitors lose much of their capacitance under
DC bias — choose the voltage rating and package so the effective value still meets the need.
Check rail droop with `simulate_circuit`: model the regulator output as a source plus its
output resistance, the trace as resistance, each capacitor with its ESR, and the load as a
current pulse (see the kicad-simulation skill).

## 6. Layout rules for power

- Buck: the input capacitor–switch–ground loop is the hot loop: make it as small as possible,
  on one layer, with the input capacitor next to the IC. Keep the switch node small. Route the
  feedback trace away from the inductor and switch node, from the output capacitor's terminal.
- Trace widths from `calc_trace_width` for each rail's current; vias from `calc_via`.
- Thermal pads: solder to copper with a via array; give LDOs and chargers copper area.
- Return currents: continuous ground plane; no slots under power loops.

## Done when

Every rail has a budget, a regulator with a thermal check that passes with margin, input
protection, and decoupling per datasheet — and the evidence (calculator outputs, simulation
results) is reported to the user.
