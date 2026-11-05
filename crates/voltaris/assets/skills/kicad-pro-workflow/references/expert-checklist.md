# Expert review checklist

Run every item. Mark each PASS, FAIL (with reference designator and fix) or N/A with a reason.

## Stackup and architecture
- Layer count justified by routing, return paths and power; stackup is one the fab builds (list_stackups)
- Copper weight fits the highest current; board thickness fits the enclosure and connectors
- Controlled-impedance nets listed with their target and the fab's tolerance

## Power
- Input protection: reverse polarity (where a user can plug in wrong), over-current (fuse/polyfuse), ESD/TVS on external power
- Every regulator: input and output capacitors per datasheet, dissipation checked with calc_regulator_thermal, dropout margin at minimum input
- Bulk capacitance near loads with current bursts (radios, motors, LED strips)
- Battery: charge current ≤ 1C of the cell, protection present (or cell is protected), power path cannot back-feed USB
- Power-up: enable/reset RC or supervisor so the MCU starts only on a stable rail
- Inductor Isat ≥ the converter IC's peak switch-current limit; rated current ≥ worst average
- Fuse breaking capacity ≥ prospective short-circuit current of the source (battery packs)
- Power budget per source covers the sum of outputs, or firmware limits are documented
- No two fitted 0 Ω option links joining two sources; links sized for their current
- Every protection/charger IC resistor matches its datasheet table for the chosen mode

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
- Current-sense amplifiers use Kelvin traces from the shunt pads, not taps on the pour
- No courtyard overlaps, and no DRC exclusions hiding one
- Every ground pad has a via to the ground plane; no isolated pour islands
- Mounting holes clear of copper and parts by the screw head radius plus margin
- Junction temperature of every part dissipating over ~0.25 W estimated against its datasheet limit
- Paste on large thermal pads windowed (about 50–70 % coverage) so the part does not float
- Switch nodes small and away from sensitive traces and the board edge
- Silkscreen: polarity, pin-1, connector caption and pin strip, board name/revision/date

## Manufacturing
- DRC clean against the fab's current rules; zones refilled
- Every part has MPN and LCSC number, footprint matches the ordered package, rotation corrections checked for assembly
- Fiducials or tooling holes if the assembler requires them; panelisation agreed
- Test points on every rail and key signal for bring-up
- Vias in SMD pads only with filled and capped vias ordered (IPC-4761 type VII) and ENIG
- BOM = schematic = pick-and-place; DNP only as KiCad's DNP flag, never as a text field

## Release
- Gerbers and drill files viewed after export, not just generated
- Netlist matches between schematic and board (update_pcb_from_schematic shows no changes)
- Mechanical fit checked in 3D against the enclosure (render_board_3d, generate_enclosure)
- A bring-up procedure for every rail and critical signal; firmware pin map from the schematic
- A snapshot tagged with the release revision; the release notes list what changed
