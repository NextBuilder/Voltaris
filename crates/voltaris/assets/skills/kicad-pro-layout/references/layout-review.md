# Layout review

Run after routing, before the independent review (Gate 6). Each item needs evidence: a tool
result, a measurement or a render view.

## Placement
- [ ] `score_placement`: no hard failures; no courtyard overlaps
- [ ] Connectors on edges, facing out; mounting-hole screw clearance kept on both sides
- [ ] Every switching regulator's hot loop matches its datasheet layout example
- [ ] Every power pin has its decoupling capacitor on the same side, closer than its via
- [ ] Crystal beside its pins; nothing routed under it
- [ ] Antenna keep-out empty on every layer

## Return paths and planes
- [ ] Every fast signal has an unbroken reference plane underneath
- [ ] No fast signal crosses a slot, split or pour boundary
- [ ] A ground via beside every fast-signal via
- [ ] Bottom-layer jumps on 2-layer boards are short and perpendicular
- [ ] Pours stitched; no islands or slivers

## Power
- [ ] Each power net's narrowest track is at least its `calc_trace_width` result
- [ ] Enough vias for the current at each layer change (`calc_via`)
- [ ] High-current pads connect solidly, not through thermal spokes

## Signals
- [ ] Differential pairs: `analyze_net_lengths` verdict pass; impedance from `calc_impedance`
- [ ] No stubs on fast nets; series terminations at the source
- [ ] No acute angles; widths change only at pads; buses parallel and evenly spaced

## Checks
- [ ] `run_drc` (live sync, refilled zones): 0 errors, 0 unconnected; every warning fixed or
      explained
- [ ] `run_design_checks`: pass
- [ ] `render_board_3d` top, bottom and iso views inspected
