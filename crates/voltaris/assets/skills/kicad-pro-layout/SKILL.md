---
name: kicad-pro-layout
description: |
  Fast, professional placement and routing the way experienced layout engineers do it: floor
  plan, placement by function and signal flow, routing order, return paths and ground planes,
  current-sized power copper, via strategy, 2-layer and 4-layer playbooks, autorouter cleanup
  and a layout review. Triggers on: "route like a pro", "professional routing", "clean
  routing", "placement", "floor plan", "ground plane", "return path", "2-layer board",
  "4-layer board", "autoroute", "Freerouting", "noise", "EMI", "power routing", "high current",
  "layout review", "make the layout better".
argument-hint: "[board or block to lay out]"
---

# Professional Layout and Routing

Good layout is decided before the first track: **placement is 80 % of routing.** This skill
is the method; kicad-pcb has the tool mechanics, kicad-high-speed-rf has interface-specific
numbers, and kicad-board-craft has the finishing style.

```
load_toolset('pcb_components')   # set_component_placements move_component rotate_component align_components get_component_pads
load_toolset('placement')        # score_placement place_decoupling_caps
load_toolset('pcb_routing')      # route_trace route_pad_to_pad route_differential_pair add_via add_copper_pour create_netclass assign_net_to_class query_traces delete_trace
load_toolset('engineering')      # calc_trace_width calc_via calc_impedance
load_toolset('verification')     # run_drc set_design_rules set_predefined_sizes
load_toolset('visual_review')    # render_board_3d
```

## The one physical idea

Every signal is a loop: current goes out on the trace and **returns on the copper directly
beneath it** (at high frequency) or along the lowest-resistance path (at DC). Noise, EMI,
crosstalk and ground bounce all come from loops that are large or interrupted. So:

- Every fast or noisy signal needs an unbroken reference plane right under it.
- A slot, split or line of vias in that plane forces the return around it — a bigger loop.
- When a signal changes layer, its return must change layer too: put a ground via within
  about 1 mm of every signal via on a fast net.
- Small hot loops (switching regulator input, gate drives, crystals) are made small by
  placement, not by routing.

## 1. Floor plan (before placing any part)

1. Fix the mechanics: outline, mounting holes, connectors at their edges, buttons, displays,
   antenna keep-outs (kicad-board-craft for style, the module datasheet for keep-outs).
2. Draw the zones: power input → regulation → MCU/logic → interfaces → outputs, flowing in one
   direction across the board. Keep noisy zones (switchers, motor/LED drivers, relays) away
   from quiet ones (ADC inputs, sensors, crystals, RF).
3. Decide the layer plan now — see the [2-layer and 4-layer playbook](references/layer-playbook.md).
4. Snapshot (`snapshot_design`) so the floor plan can be restored.

## 2. Placement order

1. Connectors and mechanical parts (fixed by the floor plan).
2. The main IC/module, oriented so its pins face the parts they talk to.
3. **Critical loops first**, each as tight as the datasheet layout example:
   switching regulator (input cap → switch → inductor → output cap, all on one side, one
   ground point), crystal (beside the pins, load caps beside the crystal), USB (connector →
   ESD → series parts → MCU in a straight line).
4. Decoupling: one cap per power pin, on the same side, between the pin and the via to the
   plane, with the smallest value closest. Use `place_decoupling_caps` with the capacitor
   references you assign.
5. Everything else grouped by function, close to what it connects to; pull-ups beside their
   bus, LEDs at the edge.
6. Batch moves with `set_component_placements` (one undo step), then `score_placement`:
   no hard failures, no courtyard overlaps.

Ratsnest test: the airwires should look short and mostly untangled. If they cross wildly,
rotate or swap parts now — every crossing removed here saves two vias later.

## 3. Board rules before routing

- Set clearances and minimums for the fab (`set_design_rules`; JLCPCB-class default
  0.15 mm track and space, 0.3 mm via drill / 0.6 mm pad — prefer 0.2 mm and 0.4/0.8 mm where
  space allows, they are cheaper and more reliable).
- Net classes with `create_netclass` / `assign_net_to_class`: power classes sized with
  `calc_trace_width` for their real current (external layer, 10 °C rise is a sensible
  default), signal default 0.2–0.25 mm, controlled-impedance classes from `calc_impedance`.
- Put the standard widths and via sizes in the dropdowns with `set_predefined_sizes` so every
  track uses a planned width.

## 4. Routing order

1. **Power and high-current paths**, short and wide, with enough vias to carry the current
   (`calc_via` gives current per via; use several in parallel at every layer change).
2. **Critical signals**: differential pairs (`route_differential_pair`), clocks, crystal,
   RF feed, analog inputs — shortest path, over solid ground, away from the board edge.
3. **Buses** as a group, parallel, equal spacing, turning together.
4. **General signals**, by hand or with the autorouter (section 5).
5. **Ground pours** last on each layer (`add_copper_pour`), then stitching vias.

Style rules that are also good engineering:
- 45° corners (or arcs); no acute angles that trap etchant.
- Leave pads straight out of the pad's long axis before turning; neck down only at the pad.
- Keep traces 3× their width from neighbours where crosstalk matters (the "3W" rule).
- Never route between the pads of a passive or under a crystal or inductor.
- No stubs on fast nets; route through test points rather than branching to them.
- Thermal reliefs on plane connections of hand-soldered and through-hole pads; solid
  connections for high-current pads.

## 5. Autorouter, then clean up

Freerouting is fast and useful for the boring 60 %, never for the critical 40 %.

1. Route and lock power, critical and matched nets by hand first.
2. `check_freerouting`, then `export_specctra_dsn` → `route_specctra_dsn` →
   `plan_specctra_ses_import` → `apply_specctra_ses`. Locked tracks are preserved.
   ```
   load_toolset(['pcb_export', 'integration', 'pcb_routing'])   # export_specctra_dsn route_specctra_dsn apply_specctra_ses
   ```
3. Review every autorouted net: remove detours and via pairs that a single-layer path could
   avoid (`query_traces`, `delete_trace`, `route_trace`), and restore ground-plane continuity
   on the reference layer.
4. Snapshot before and `diff_design` after, so the user sees what the autorouter did.

## 6. Finish

1. Ground pours on both outer layers of a 2-layer board, `refill_zones`.
2. Stitching vias every 5–10 mm across the pour and along the edges; a fence of vias along RF
   traces and the board perimeter on noisy boards.
3. Remove copper islands and thin slivers (raise the zone's minimum width or delete them).
4. Teardrops on pads and vias.
5. `run_drc` with a live-board sync and zone refill: 0 errors, 0 unconnected items.
6. Run the [layout review](references/layout-review.md) and fix what it finds.

## Speed habits

- Batch: place with `set_component_placements`, read pads with `get_component_pads`, route
  pad-to-pad with `route_pad_to_pad`, rather than one move at a time.
- Read before you route: `get_component_pads` gives exact pad centres, so tracks start on the
  pad and DRC passes first time.
- Snapshot at every gate; a bad experiment costs one `restore_snapshot`, not an hour.
- Render (`render_board_3d`, top view) after placement and after routing; looking at the board
  catches what numbers miss.
