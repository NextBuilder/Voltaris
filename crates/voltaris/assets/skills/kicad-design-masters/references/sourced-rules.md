# Sourced rules: what the makers' own notes say

Every rule here was read from the cited Tier-1 document (November 2026), not recalled. The
code in brackets is the document number, so anyone can check it. A part's own datasheet still
wins over all of these.

| Code | Document |
|---|---|
| ADI MT-031 | Grounding Data Converters and Solving the Mystery of "AGND" and "DGND" |
| ADI MT-101 | Decoupling Techniques |
| ADI AN-1368 | Ferrite Bead Demystified |
| TI SNVA021 | AN-1149 Layout Guidelines for Switching Power Supplies |
| TI SLYT614 | Five steps to a great PCB layout for a step-down converter |
| TI SNVA489 | AN-2162 Simple Success With Conducted EMI From DC-DC Converters |
| TI SLUA271 | QFN and SON PCB Attachment |
| TI SPRAAR7 | High-Speed Interface Layout Guidelines |
| TI SLVA680 | ESD Protection Layout Guide |
| JLCPCB, PCBWay | Published rigid-PCB capability pages |

## Switching converters

| Rule | Source |
|---|---|
| Place the input capacitor first, as close to the IC as the fab rules allow, routed to PVIN and PGND with short wide copper before anything else | SLYT614 step 1 |
| Then the inductor, close to the IC, rotated so the switch node copper stays as small as possible; no vias in the switch node | SLYT614 step 2 |
| Output capacitor last on power ground, closing the loop back to PGND; no vias inside the power loop | SLYT614 step 3 |
| Feedback divider, soft-start and small decoupling at the IC with short direct traces; keep the FB node tiny; route FB away from the inductor and switch node, on the far side of a ground plane if possible | SLYT614 step 4, SNVA021 |
| Output-voltage sense from the output capacitor with its own dedicated trace and two vias, isolated from other copper | SLYT614 step 3 |
| Quiet ground for small-signal parts, power ground for the loop, joined at one point — the exposed pad; thermal vias directly under the pad | SLYT614 step 5 |
| Closed-core (shielded) inductors; never stick cores | SNVA021 |
| Both switch-state current loops circulate in the same direction | SNVA021 |
| High-current traces: an absolute floor of 0.38 mm per ampere (15 mil/A); size real widths with `calc_trace_width` (IPC-2152) | SNVA021 |
| Vias between layers for current: TI's two notes disagree — about one via per 200 mA (SNVA021) and roughly one via per ampere (SLYT614). Use `calc_via` for the drill and copper you have and add margin | SNVA021, SLYT614 |

## Conducted EMI input filter

| Rule | Source |
|---|---|
| LC low-pass with the inductor in the supply lead; filter resonance at or below a tenth of the switching frequency | SNVA489 |
| Filter inductor typically 1–10 µH for low and medium power: the largest that meets current and size | SNVA489 |
| Damp it: an electrolytic Cd ≥ 4 × the converter's input capacitance with a series resistance, rated ≥ 25 % above the maximum input voltage | SNVA489 |
| Conducted EMI comes from the input current, not the layout: the filter fixes it; layout fixes radiated EMI | SNVA489 |

## Decoupling and ferrite beads

| Rule | Source |
|---|---|
| A bulk electrolytic (10–100 µF) where each supply enters the board, within about 50 mm of the chip | MT-101, MT-031 |
| A 0.01–0.1 µF ceramic at every power pin, its own via to the ground plane and the IC ground pin on a second via | MT-031, MT-101 |
| X7R for values up to a few µF; C0G only to about 0.01 µF; general-purpose aluminium electrolytics are not decoupling parts | MT-101 |
| A capacitor's self-resonance (1 / 2π√(ESL·C)) must sit above the frequency it filters; parallel a small and a large value to widen the band | MT-101 |
| A ferrite bead loses most of its impedance under DC current: run it near 20 % of its rated current, not at its rating; datasheet curves are at zero bias | AN-1368 |
| A bead with a ceramic capacitor peaks 10–15 dB at its resonance, worst at light load; damp it with a capacitor ≥ 16 × the decoupling capacitance plus a series resistor (about 1–2 Ω), and leave a resistor footprint to tune | AN-1368 |
| Never put an undamped bead filter on a rail feeding a PLL, VCO or precise ADC near the converter's switching frequency | AN-1368 |

## Grounding

| Rule | Source |
|---|---|
| At least one full layer of ground; on a 2-layer board keep at least 75 % of the ground layer intact and remove islands and thin necks | MT-031 |
| A 4-layer board puts power and ground planes next to each other | MT-031 |
| A single board with one converter: one ground plane, AGND and DGND joined at the converter; split planes only for multi-board or multi-converter systems, joined at one star point | MT-031 |
| Converter with low digital current: AGND and DGND both to the analog ground with the shortest leads; a buffer register beside it with series resistors (about 500 Ω for a 10 pF load) | MT-031 |
| Sampling clock from a low-jitter oscillator on the analog side, never a microcontroller clock pin | MT-031 |
| Ground pins between signal pins on connectors; 30–40 % of board-to-board connector pins as ground | MT-031 |
| Do not autoroute a mixed-signal board | MT-031 |

## Thermal pads (QFN, SON)

| Rule | Source |
|---|---|
| Board pad the size of the exposed pad, at least 0.2 mm from the inner ends of the lead pads | SLUA271 |
| Thermal vias 0.3 mm drill on about a 1.0 mm grid; plugged vias preferred, top-side tenting if plugging is not available | SLUA271 |
| Paste on the thermal pad 50–70 % of its area in several windows; a full-size window floats the part | SLUA271 |
| Non-solder-mask-defined pads; no vias or copper beside pin 1 of a part with an exposed pin-1 feature | SLUA271 |
| Solder the centre pad even when no vias are needed | SLUA271 |

## High-speed interfaces

| Rule | Source |
|---|---|
| A continuous ground reference under every high-speed pair; never cross a split; at least 2.3 mm (90 mil) from the plane edge | SPRAAR7 |
| On a reference-layer change, ground stitching vias within 5 mm (200 mil) of the signal vias, symmetric, the closer the better | SPRAAR7 |
| 5 × trace width between pairs; 0.76 mm (30 mil) from other signals; 1.27 mm (50 mil) from clocks | SPRAAR7 |
| USB 2.0: 90 Ω ±10 % differential, at most 1.27 mm (50 mil) skew inside the pair, at most 4 vias per line, no test points or stubs | SPRAAR7 |
| USB 3, PCIe, SATA, HDMI: 0.127 mm (5 mil) skew inside the pair; vias 2 (USB 3) or 0; via stubs under 0.38 mm (15 mil) or back-drilled | SPRAAR7 |
| Bends above 135°; constant width after the escape; AC capacitors 0402 or smaller, placed symmetrically | SPRAAR7 |
| ESD parts, common-mode filter and AC capacitors as one tight group at the connector | SPRAAR7 |

`calc_impedance` sets the widths and `analyze_net_lengths` checks the skew.

## ESD protection

| Rule | Source |
|---|---|
| TVS as close to the connector as the rules allow, and far closer to it than to the protected IC | SLVA680 |
| The signal goes connector → TVS pad → IC; no stub to the TVS, no via before the TVS | SLVA680 |
| TVS ground straight to a plane with a via right beside its ground pin; every 0.25 nH there adds about 10 V at 8 kV | SLVA680 |
| 45° corners at most, or curves; no unprotected traces between the connector and the TVS | SLVA680 |

## Fabrication limits for both JLCPCB and PCBWay

Design to the stricter of the two, with margin, so one set of files builds at either fab.

| Item | JLCPCB | PCBWay | Design to |
|---|---|---|---|
| Track / gap (1 oz) | 0.10 / 0.10 mm | 0.10 / 0.10 mm | 0.15 / 0.15 mm; 0.127 mm only where needed |
| Mechanical drill | 0.15 mm | 0.15 mm (extra cost under 0.2) | 0.3 mm vias; 0.2 mm only on dense boards |
| PTH annular ring | 0.25 mm recommended (2-layer) | 0.15 mm | 0.15 mm minimum, 0.25 mm on 2-layer |
| Copper to routed edge | 0.2 mm | 0.25 mm normal | 0.3 mm or more |
| Silkscreen line width / text height | 0.15 / 1.0 mm | 0.15 / 0.8 mm | 0.15 / 1.0 mm (`polish_board` default) |
| Solder mask bridge | 0.10 mm green, 0.13 mm black or white | 0.10 mm green, 0.11 mm other | 0.13 mm |
| Non-plated hole | 0.5 mm minimum | — | 0.5 mm minimum |
| Pad hole to pad hole | 0.45 mm | 0.41 mm | 0.5 mm |
| Via in pad | Filled and capped; default from 6 layers | Separate service | Only with filled and capped vias ordered and stated in the fab notes |

Set these with `set_design_rules` at Gate 3; `audit_layout` and DRC enforce them.
