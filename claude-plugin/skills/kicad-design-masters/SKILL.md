---
name: kicad-design-masters
description: |
  The working rules of the people the industry learns PCB design from — Rick Hartley, Eric
  Bogatin, Howard Johnson, Henry Ott, Keith Armstrong, Analog Devices' grounding notes — and
  the open-hardware teams whose boards set the standard (Great Scott Gadgets, Antmicro, Glasgow,
  Libre Solar, OLIMEX, Phil's Lab), turned into numbers and Voltaris checks. Triggers on:
  "best practice", "like an expert", "world class", "rules of thumb", "signal integrity",
  "return path", "why does my board radiate", "EMC", "crosstalk", "rise time", "mixed signal",
  "analog ground", "split ground", "learn PCB design", "who to learn from".
---

# Rules From the Masters

Each section states a rule as its teachers give it (paraphrased), the number that makes it
usable, and the Voltaris tool that checks it. The [world corpus](references/world-corpus.md)
lists the open boards these rules were checked against and what each one does best.

```
load_toolset('engineering')      # calc_impedance calc_trace_width calc_via list_stackups
load_toolset('signal_integrity') # analyze_net_lengths
load_toolset('smart_layout')     # audit_layout route_board smart_place
load_toolset('design_checks')    # run_design_checks add_check_rule
```

## 1. Rise time decides, not clock frequency (Howard Johnson, Eric Bogatin)

- A signal's useful bandwidth is about 0.35 / rise time: a 1 ns edge reaches 350 MHz even on
  a 1 MHz clock. A fast MCU pin is a fast signal whatever the firmware does with it.
- Signals travel about 15 cm per nanosecond in FR-4 (6 inches per ns).
- A trace behaves as a simple wire while its delay is under about a sixth of the rise time;
  longer than that, treat it as a transmission line: controlled impedance, series termination
  at the source, no stubs. For a 1 ns edge that is roughly 2.5 cm.
- Practical consequence: give clocks, SPI/QSPI, SDIO, USB and any GPIO driving a long cable the
  care of a fast net. Voltaris's fast-net checks cover them by name.

## 2. The return path is half the circuit (Rick Hartley, Henry Ott)

- Current flows in loops. At high frequency the return flows in the plane directly under the
  signal, because that path has the least inductance; the energy itself travels in the
  dielectric between the trace and that plane.
- So the plane next to a fast trace must be **unbroken under its whole length**: no slots, no
  splits, no rows of vias with merged anti-pads cutting across it.
- When a signal changes layer, give its return a way to change plane next to the signal via
  (`audit_layout` return-via check, which knows when two layers share one plane).
- Tight coupling wins: the closer a trace is to its plane, the smaller the loop, the lower the
  crosstalk and radiation. This is why the best stackups put thin dielectric between each
  signal layer and its ground (see kicad-multilayer).
- Do not split the ground plane to "separate" analog and digital. Partition the placement
  instead and keep one plane (section 6).

## 3. Impedance in your head (Eric Bogatin)

- 50 Ω microstrip on FR-4: the trace is about twice as wide as its height above the plane.
- 50 Ω stripline: the two planes are about twice the trace width apart.
- Use these to sanity-check `calc_impedance` and the fab's stackup tool, never instead of them.
- A 90 Ω or 100 Ω differential pair stays a pair: same layer, same length (`route_board`
  `pair_skew`, `analyze_net_lengths`), constant gap, and a plane under both traces.

## 4. Crosstalk and spacing

- Keep a fast trace at least three trace widths (edge to edge) from its neighbours, and more
  from anything that leaves the board.
- Coupling falls quickly with spacing and rises with parallel length — so route aggressors and
  victims apart, and cross them at right angles on adjacent layers instead of running them in
  parallel.
- Keep clocks and fast edges a few millimetres in from the board edge: the field fringes and
  radiates there (`audit_layout` edge check).

## 5. Power delivery (Bogatin, Analog Devices)

- A decoupling capacitor is only as good as its mounting loop: short wide connections, vias at
  the pad edges, two vias on larger capacitors, and the capacitor on the same side as the pin
  or directly under it.
- Place the smallest value nearest the pin; bulk capacitors can sit further away.
- Closely spaced power and ground planes form a capacitor that works above the frequency where
  discrete capacitors stop helping — another reason to pair them in the stackup.
- `audit_layout` decoupling check: under 3 mm is the target, 3–5 mm is advice, beyond 5 mm is a
  warning.

## 6. Mixed signal and grounding (Analog Devices, Henry Ott)

- One ground plane. Place analog parts over one region and digital over another, and route
  each kind only over its own region, so their return currents never mix.
- A data converter's AGND and DGND pins both go to that one plane, right at the part; the
  converter sits on the boundary between the regions.
- If a split is truly required (isolation, safety), every signal crosses only at the single
  bridge between the halves.
- Keep switching regulators and their hot loops away from the analog region and its inputs.

## 7. EMC: what leaves the board (Keith Armstrong, Henry Ott, Kenneth Wyatt)

- Cables are the antennas. Filter and protect every signal at the connector where it leaves
  the board, and keep fast circuitry away from connectors.
- Stitch ground pours to the planes densely around the board edge and around RF — a spacing
  well under a twentieth of the shortest wavelength you care about; a few millimetres on
  typical boards (`audit_layout` stitching check).
- Unconnected or poorly stitched copper islands on signal layers can resonate: connect every
  pour to ground with vias, or remove it.
- The "20-H rule" (pulling power planes back from the edge) is widely disputed among these
  experts; stitching and a ground ring at the edge matter more.

## 8. Learn from boards that shipped

The open boards in the [world corpus](references/world-corpus.md) are the best free training
material there is: open a project in KiCad, look at its stackup, net classes and how the hard
parts (USB, RF, BGA, power stage) are placed and routed, and copy the pattern. Then run
`audit_layout` on your own board — the same engine graded the corpus.

## Where to keep learning

- **Books:** Howard Johnson and Martin Graham, *High-Speed Digital Design*; Eric Bogatin,
  *Signal and Power Integrity — Simplified*; Henry Ott, *Electromagnetic Compatibility
  Engineering*; Lee Ritchey, *Right the First Time*.
- **Talks and channels:** Rick Hartley's talks on return paths and stackups (AltiumLive,
  Robert Feranec's channel); Eric Bogatin's Signal Integrity Academy and webinars; Robert
  Feranec / FEDEVEL Academy (long expert interviews and board-layout courses); Phil's Lab
  (Phil Salmony, complete KiCad hardware designs — his HadesFCS board is in the corpus);
  Altium Academy (Zach Peterson); EEVblog (Dave Jones); Kenneth Wyatt on EMC troubleshooting.
- **Application notes:** Analog Devices MT-031 (grounding data converters) and MT-101
  (decoupling); Texas Instruments' layout guides for each switching regulator you use.
