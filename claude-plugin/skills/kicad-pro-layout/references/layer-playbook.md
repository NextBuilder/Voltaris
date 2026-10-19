# Layer playbook

## 2-layer boards (most maker boards)

- **Bottom = ground.** Keep the bottom layer as one solid ground pour with as few cuts as
  possible. Route on top; use the bottom only for short jumps.
- When a trace must cross on the bottom, keep it short and run it **perpendicular** to the
  signals above, so it cuts the plane as a thin slot rather than a long wall. Never run two
  bottom traces side by side to make a long slot.
- Top ground pour around the routing, stitched to the bottom every 5–10 mm and next to every
  ground pad of an IC.
- USB 2.0 Full Speed and 2.4 GHz RF work on 2 layers if the bottom is solid ground under them;
  the 1.6 mm dielectric makes 90 Ω/50 Ω traces wide, so use coplanar structures
  (`calc_impedance`) or a 0.8 mm / 1.0 mm board when impedance matters.
- Power: wide top traces or a top power pour, decoupling to the bottom ground with a via at
  each capacitor's ground pad.

## 4-layer boards

Standard stackup, outer to outer:

| Layer | Use |
|---|---|
| L1 (top) | Components, fast signals, short routes |
| L2 | **Solid ground**, never split |
| L3 | Power pours (split by rail if needed) and slow signals |
| L4 (bottom) | Remaining signals, ground pour |

- Fast signals on L1 reference L2 — the tightest, cleanest return path on the board.
- A signal moving from L1 to L4 changes reference plane; add a ground via beside the signal
  via (and a decoupling capacitor near the transition if L3 is the reference on that side).
- Order the board with the fab's matching impedance stackup (`list_stackups`) and solve the
  widths for that stackup.

## When to choose 4 layers

Choose 4 layers when any of these apply: USB High Speed or faster interfaces, Ethernet, a
dense MCU (QFN-56 and up, BGA), several switching regulators, radio plus sensitive analog on
one board, or a board that must pass EMC testing first time. The cost difference at JLCPCB-class
fabs is small compared with a respin.
