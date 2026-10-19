---
name: kicad-pro-workflow
description: |
  Voltaris's gated, end-to-end professional PCB workflow: requirements → research and
  part selection → datasheet rules → schematic → board setup → placement → routing → independent
  review → manufacturing files, enclosure and firmware bring-up. Triggers on: "design a board",
  "design a PCB for", "make me a PCB", "from scratch", "full design", "professional PCB",
  "Flux-style", "end to end", "ready to manufacture", "production ready".
---

# Voltaris — Professional Design Workflow

Follow the voltaris skill's operating rules throughout. This skill adds the engineering
discipline: a board advances only through **gates**, and each gate has checks that must
return evidence. A gate whose check could not run is **INCOMPLETE** — say so, never pass it.

Specialist skills carry the depth for each area — load them when their topic comes up:
kicad-power-design (power tree, batteries), kicad-esp32 (ESP32 boards), kicad-mcu-design
(RP2040/RP2350, STM32, nRF52 boards), kicad-high-speed-rf (impedance, USB, RF, length matching),
kicad-part-sourcing (stock, alternates, importing parts), kicad-simulation (checking
circuits numerically), kicad-pro-layout (floor plan, placement and routing method) and
kicad-board-craft (form factor, connectors, silkscreen and the finished look).

Two habits run through every gate:
- **Snapshot before every large change** (applying templates, re-placing, autorouting,
  restoring) with `snapshot_design`, and show the user `diff_design` afterwards so every
  change is visible and reversible (`restore_snapshot`).
- **Rules become checks.** Every requirement the user states ("all I2C lines need pull-ups",
  "power traces at least 0.5 mm", "every part needs an LCSC number") and every datasheet rule
  that can be checked mechanically is stored with `add_check_rule`, and `run_design_checks`
  runs at Gates 2, 5 and 6.
  ```
  load_toolset(['design_history', 'design_checks'])
  ```

Ask the user only where the answer changes the design, and always offer suggested options.
Show a short plan and get approval after Gate 1; after that, work through the gates and
report at each one.

## Gate 0 — Requirements brief

Pin down, with suggested options for each:
- Function, inputs/outputs, interfaces (USB, I2C sensors, displays, motors, radios)
- Power source and budget (USB 5 V, Li-ion cell, adapter), runtime target
- Board size or enclosure limit, connector positions, layer count (2 or 4)
- Fab and assembly house (JLCPCB by default), quantity, cost target
- Environment: indoor/outdoor, temperature, enclosure

Write the brief back to the user before Gate 1.

## Gate 1 — Architecture, part selection, research

1. Draw the block diagram in words: power tree, MCU, each interface. Once the schematic
   exists, `generate_block_diagram` draws the real one from the netlist — compare it with the
   brief and check `rails_without_source` is empty.
2. Power budget: sum peak and average currents per rail, then size each regulator.
   ```
   load_toolset('engineering')      # calc_regulator_thermal calc_buck calc_trace_width calc_passive
   ```
   Every linear regulator gets `calc_regulator_thermal` with the datasheet θJA. Above the
   thermal limit, switch to a buck (`calc_buck`).
3. Parts: prefer parts that are in stock at the assembly house.
   ```
   load_toolset('integration')      # search_jlcpcb_parts get_jlcpcb_part get_datasheet_url
   ```
   Record MPN, LCSC number and stock for every active part. Look up current stock — never
   quote it from memory. A part KiCad's libraries lack is imported by LCSC number with
   `import_lcsc_part` and accepted against its datasheet (kicad-part-sourcing skill).
4. **Datasheet rules.** For every IC, read the datasheet with `read_datasheet` (by MPN, LCSC
   number, URL or a PDF the user saved): its pin description, absolute maximum ratings,
   recommended operating conditions, typical application and layout guidelines come back with
   page numbers. Store each requirement as a design rule with its source page, and each
   mechanically checkable one as a check rule too — see
   [datasheet rules](references/datasheet-rules.md).
   ```
   load_toolset(['datasheet', 'config', 'design_checks'])   # read_datasheet add_design_rule add_check_rule
   ```
5. **Simulate the risky parts** — filters, rail droop under radio bursts, regulator output
   filters, analog front ends — with `simulate_circuit` (kicad-simulation skill).
   ```
   load_toolset('simulation')       # simulate_circuit run_ngspice
   ```

**Gate 1 exit:** brief, block diagram, power budget, part list with MPN/LCSC/stock, and the
stored datasheet rules. Get the user's approval.

## Gate 2 — Schematic

```
load_toolset('templates')        # search_templates get_template apply_template
load_toolset('sch_batch')        # batch_connect_to_net validate_component_connections
load_toolset('sch_wiring')       # add_no_connect add_power_symbol
load_toolset('sch_analysis')     # find_shorted_nets find_single_pin_nets
load_toolset('sch_export')       # run_erc update_pcb_from_schematic
load_toolset('firmware')         # generate_firmware_kit
```

1. Start from verified templates (`search_templates`). After `apply_template`, run every entry
   of its wiring plan in order — each is a complete `batch_connect_to_net` call — and put
   `add_no_connect` on each pin it lists for no-connect flags. Map template nets onto project
   nets with `net_mappings`.
2. Add what no template covers, following the stored datasheet rules.
3. Every IC: decoupling as its datasheet states, unused pins handled as its datasheet states.
4. Checks — all must be clean or explicitly justified:
   - `run_erc`: 0 errors
   - `find_shorted_nets`: none
   - `find_single_pin_nets`: only intentional test points/headers
   - `validate_component_connections`: every unconnected pin is intentional
   - `run_design_checks`: verdict pass (or pass_with_warnings with each warning justified)
   - `generate_firmware_kit` (ESP32, RP2040/RP2350, STM32, nRF52): its warnings list is empty, or
     each warning is accepted by the user
   - `generate_block_diagram`: matches the Gate 1 architecture; no rail without a source

## Gate 3 — Board setup

```
load_toolset('pcb_board')        # set_board_size add_mounting_hole add_zone
load_toolset('pcb_routing')      # create_netclass assign_net_to_class
load_toolset('engineering')      # calc_impedance list_stackups calc_trace_width calc_via
```

1. Stackup: for impedance-critical nets choose a preset (`list_stackups`) and solve widths with
   `calc_impedance` — USB 2.0 is a 90 Ω differential pair, RF feeds are 50 Ω (CPWG on 2/4-layer
   boards). Order the board with the same stackup.
2. Net classes: power rails sized by `calc_trace_width` for their current; signal default;
   USB pair; RF. Create with `create_netclass` and `assign_net_to_class`.
3. Outline, mounting holes (M3 by default) and keep-outs (antenna, mounting-hole screw heads).
   Agree the style with the user — form factor, connector standard, mask colour — using the
   kicad-board-craft skill.
4. Push the netlist with `update_pcb_from_schematic`.

## Gate 4 — Placement

```
load_toolset('placement')        # score_placement place_decoupling_caps
load_toolset('pcb_components')   # move_component rotate_component align_components
load_toolset('visual_review')    # render_board_3d
load_toolset('smart_layout')     # smart_place audit_layout
```

Start with `smart_place` (dry run, review the moves, apply), then adjust by hand.
Follow the kicad-pro-layout skill's floor plan and placement order. In short: fixed mechanical parts (connectors at their edges, buttons, mounting
holes) → RF module with its antenna over the edge/keep-out → MCU → power stage with its input
loop tight → decoupling beside the pins it serves (`place_decoupling_caps` with the capacitor
references you chose) → everything else grouped by function, sensors away from heat.

**Gate 4 exit:** `score_placement` has no hard failures, and a `render_board_3d` iso + top
review against its checklist finds nothing open.

## Gate 5 — Routing

```
load_toolset('pcb_routing')      # route_trace route_differential_pair add_via add_copper_pour apply_specctra_ses
load_toolset('integration')      # check_freerouting route_specctra_dsn
load_toolset('pcb_export')       # refill_zones
load_toolset('verification')     # run_drc
load_toolset('smart_layout')     # route_board polish_board audit_layout
```

1. By hand first: power input loop and regulator loops (short, wide), USB pair with
   `route_differential_pair`, RF feed, crystal.
2. Ground: solid pour on the reference layer (`add_copper_pour`), stitching vias along RF and
   board edges.
3. Remaining signals with `route_board` (dry run, then apply with its plan_revision), or
   Freerouting for very dense boards (kicad-pro-layout skill, section 5). Review the result —
   never accept it unseen.
   Then `polish_board` for pin labels, the name block, the mirrored pour and stitching vias.
4. `refill_zones`, then `run_drc`: 0 errors and 0 unconnected items.
5. `analyze_net_lengths` on every differential pair and matched group: verdict pass.
   ```
   load_toolset('signal_integrity') # analyze_net_lengths
   ```
6. `run_design_checks` again — board_nets rules (minimum power trace widths) run now.
7. `audit_layout`: engineering and beauty grades A, or every remaining finding explained.

## Gate 6 — Independent review

A design must not grade itself. Delegate to the **kicad-pro-review-agent** (fresh context),
then run your own pass:

```
load_toolset('design_review')    # run_design_review
load_toolset('visual_review')    # render_board_3d
```

- `run_design_review`, `run_design_checks` and the full [expert checklist](references/expert-checklist.md)
- `diff_design` against the snapshot approved at Gate 1/2: every change since then is explained
- `render_board_3d` with views iso, top and bottom against its visual checklist, plus the
  kicad-board-craft beauty review and the kicad-pro-layout layout review
- Fix every finding, then re-run Gates 2, 5 and 6 checks.

## Gate 7 — Manufacturing, enclosure, bring-up

```
load_toolset('manufacturing')    # validate_for_manufacturing export_manufacturing_package
load_toolset('enclosure')        # generate_enclosure
load_toolset('firmware')         # generate_firmware_kit
```

1. `validate_for_manufacturing`, then `export_manufacturing_package` for the chosen fab; follow
   the kicad-manufacture skill's evidence rules for the package.
2. Optional enclosure: `generate_enclosure`, pass exact connector heights from datasheets.
3. Firmware: `generate_firmware_kit` with an output directory.
4. Write the [bring-up plan](references/bringup-plan.md) for the first boards.

## Final report

One table: each gate, its checks, the evidence (counts, verdicts), and status
PASS / FAIL / INCOMPLETE. The board is "ready to order" only when every gate is PASS. Always
recommend ordering a small first batch (5 boards) and following the bring-up plan before
volume.
