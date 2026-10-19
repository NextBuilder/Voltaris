# Expert review checklist

Run every item. Mark each PASS, FAIL (with reference designator and fix) or N/A with a reason.

## Power
- Input protection: reverse polarity (where a user can plug in wrong), over-current (fuse/polyfuse), ESD/TVS on external power
- Every regulator: input and output capacitors per datasheet, dissipation checked with calc_regulator_thermal, dropout margin at minimum input
- Bulk capacitance near loads with current bursts (radios, motors, LED strips)
- Battery: charge current ≤ 1C of the cell, protection present (or cell is protected), power path cannot back-feed USB
- Power-up: enable/reset RC or supervisor so the MCU starts only on a stable rail

## MCU and logic
- Strapping/boot pins hold their boot level at reset — no external part overrides them
- Reset and boot buttons reachable in the enclosure; programming path works (native USB or UART + auto-program)
- Pull-ups on I2C (one pair per bus, value from calc_passive), open-drain lines, active-low resets and chip selects
- Logic levels match between every pair of connected parts (3.3 V vs 5 V)
- Unused pins treated per datasheet

## Interfaces and signal integrity
- USB D+/D− as a 90 Ω pair, length-matched, no stubs, ESD at the connector, CC resistors per CC pin
- RF: 50 Ω feed, antenna keep-out on every layer, ground stitching around the feed
- Crystals: load caps from calc_passive, short traces, guard ground, no signals underneath
- Every external connector: ESD protection and a defined pinout matched to the mating cable

## Layout
- Switching regulator input loop and switch node as small as possible; feedback trace away from the switch node
- Solid reference plane under every high-speed or RF trace; no splits under signals
- Trace widths ≥ calc_trace_width for the net's current; vias counted with calc_via for high-current paths
- Thermal pads have via arrays; hot parts have copper and distance from sensors and the battery
- Mounting holes clear of copper and parts by the screw head radius plus margin
- Silkscreen: polarity, pin-1, connector labels, board name/revision/date

## Manufacturing
- DRC clean against the fab's current rules; zones refilled
- Every part has MPN and LCSC number, footprint matches the ordered package, rotation corrections checked for assembly
- Fiducials or tooling holes if the assembler requires them; panelisation agreed
- Test points on every rail and key signal for bring-up
