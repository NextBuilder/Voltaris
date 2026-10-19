---
name: kicad-simulation
description: |
  Simulating circuits before building them: filters, dividers, RC/LC timing, regulator output
  filters, rail droop under load steps, op-amp stages (built-in solver) and nonlinear circuits
  with real models (ngspice). Triggers on: "simulate", "SPICE", "ngspice", "frequency response",
  "Bode", "transient", "ripple", "droop", "filter design", "will this work", "time constant".
---

# Circuit Simulation

```
load_toolset('simulation')       # simulate_circuit run_ngspice
load_toolset('sch_export')       # generate_netlist
load_toolset('engineering')      # calc_passive calc_buck
```

## Pick the engine

- **`simulate_circuit`** — built in, nothing to install. Linear circuits: R, C, L, sources
  (DC, AC, PULSE, SIN, PWL), ideal amplifiers (E) and transconductances (G). Analyses `.op`,
  `.ac`, `.tran`. Answers most board questions: filter corner frequencies, divider loading,
  decoupling and rail droop, LC output-filter resonance, RC reset delays, ideal op-amp gain.
- **`run_ngspice`** — needs ngspice installed. Anything nonlinear: diodes, transistors,
  MOSFET switches, regulators with vendor models, saturation, clipping. Export the schematic
  with `generate_netlist` using the spice format after assigning simulation models, or write
  the netlist by hand, and pass analysis commands as control lines.

## Model what matters

Ideal parts hide the problems you are trying to find. Add, as R/L/C:
- capacitor ESR (series R) and, for fast edges, ESL (series L)
- effective capacitance after DC-bias derating (often 30–70% below nominal for small ceramics)
- regulator output resistance and trace resistance (from `calc_trace_width` with a length)
- source impedance of whatever drives an ADC or a filter

## Recipes

See [simulation recipes](references/recipes.md) for ready netlists: RC low-pass, LC output
filter, rail droop under a radio burst, divider into an ADC, ideal non-inverting amplifier.

## Reading results

- AC: the −3 dB frequency, peaking (resonance) and phase. More than ~3 dB of peaking in an LC
  filter means it needs damping.
- Transient: final value, ripple over the last part of the run, rise times, overshoot, and the
  minimum value during a load step (droop) — compare with the downstream part's minimum supply.

Report the netlist you simulated alongside the numbers, so the assumptions are visible.
