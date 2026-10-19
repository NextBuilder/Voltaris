# Reference board study

Fifteen KiCad board files (fourteen designs) that maker product teams released as open
hardware, measured from their `.kicad_pcb` and `.kicad_pro` files and audited with
`audit_layout`. Numbers are what the files contain, not what the projects claim.

Sources: PCBCupid's GitHub (Glyph Mini 2040, G-Sense touch slider and I2S microphones, MSPM0
dev kit boards, LoRa sensor nodes, hackathon keychain) and CircuitDigest's GitHub (LiteWing
ESP32-S3 drone v2.5 and v2.6, nRF52840 FIDO2 security key, EdgeSense ToF scanner, robot arm
controller).

## The boards

| Board | Size (mm) | Parts | Notable |
|---|---|---|---|
| Glyph Mini 2040 (RP2040) | 18 × 23.5 | 35, both sides | Castellated pins, QFN-56 0.4 mm, QSPI flash, USB-C, knockout name, 0.6 mm pin numbers |
| G-Sense touch slider | 50.8 × 22.9 | 23 | Labelled jumpers (INT PULLUP / INT DETECT), bottom pin labels |
| G-Sense I2S mic (1 and 2 ch) | 25.4 × 22.9 | 13–15 | Teardrops, channel-select note printed on the board |
| MSPM0 dev kit (3 boards) | 12 × 12 to 50.8 × 11.4 | 9–60 | Power and Signal net classes, 0.15 mm tracks, single-letter pin labels |
| LoRa sensor nodes (2) | 54 × 63, 79 × 69 | 43–72 | Function captions, knockout section titles, solder jumpers for options, power-path charger with solar input |
| Hackathon keychain | 50.8 × 22.9 | 15 | Parts on the bottom, USB diff-pair net class, teardrops |
| LiteWing drone v2.5 / v2.6 | 100 × 100 frame | 93–95 | Arc-routed tracks, +BATT/VBUS pours on both layers, motor MOSFETs at 45°, 2 mm motor tracks |
| nRF52840 FIDO2 key | 26.5 × 11, 0.8 mm thick | 38 | Test pads for SWD, TVS on USB, touch sensor, compact two-sided layout |
| EdgeSense ToF scanner | 38.5 × 31 | 58 | STM32H5, LiPo charger, 1V8 domain with level shifter |
| Robot arm controller | 175 × 90 | 95 | ESP32, motor drivers, wide power tracks (2.5–3 mm) |

## What they agree on

| Item | Observed |
|---|---|
| Layers | 2 on every board |
| Thickness | 1.6 mm, except the USB-plug key at 0.8 mm |
| Outline | Rounded corners on 14 of 15; sizes on 2.54 mm multiples for modules |
| Ground | Ground pour on both outer layers on every board |
| Vias | 0.6 mm / 0.3 mm drill on 13 of 15 boards; 0.8 / 0.4 on the two hand-soldered LoRa nodes |
| Signal tracks | 0.2–0.3 mm (0.15 mm on the 12 mm MSPM0 boards, 0.19 mm under the RP2040) |
| Power tracks | 0.5 mm typical, 0.76–1.0 mm for 1–2 A, 1.5–3 mm or pours for battery and motors |
| Clearance (net class) | 0.15–0.2 mm |
| Edge clearance (rule) | 0.3–0.5 mm |
| Angles | Only 0/45/90° segments, or arcs; off-angle segments are rare |
| Designators | Hidden on every PCBCupid product board; shown at 0.7–0.8 mm on development boards |
| Logos | Logo footprints on almost every board, top and bottom |
| Knockout text | Product names and section titles (G-SENSE, LORA, RS485, PCBCUPID) |
| Bottom side | Pin tables, brand, URL, "MADE IN INDIA", "Designed @ / Manufactured @" lines |
| Jumpers | Triangle solder jumpers for options, each labelled with its function |
| Passive size | 0402 on machine-assembled compact boards, 0603 on mid-size, 0805 hand-solder pads on DIY boards |

## What the audit found (and what to do better)

| Finding | Boards (of 15) | Do instead |
|---|---|---|
| Fast net changes layer with no ground via within 1 mm | 10 (crystal, SPI/QSPI clock, SWCLK, USB D±) | A ground via beside every signal via on a fast net |
| Power rail on long thin track (0.15–0.3 mm for 10–70 mm) | 10 | Size every rail with `calc_trace_width`; pour high-current rails |
| Connector pins without any label | 11 | Label every pin, top or bottom (`polish_board`) |
| Decoupling capacitor more than 3 mm from its pin | 7 | Within 2–3 mm, on the same layer as the pin |
| Via inside an SMD pad | 7 (up to 58 vias) | Move the via beside the pad, or order plugged/capped vias |
| 5–8 different track widths | 6 | One width per net class |
| Unconnected items left in the file | 8 (1–8 each) | Fill zones and run DRC with unconnected items as errors |
| Designators on vias, under parts or under 0.8 mm | 6 | 0.8 mm minimum, off vias, beside the part |
| Fast trace within 1 mm of the edge | 2 | Keep clocks and USB 1 mm+ (better 2 mm) from the edge |
| No version mark | 14 | Print the version beside the product name |

Scores the shipped boards reach on `audit_layout`:

| | Lowest | Median | Highest |
|---|---|---|---|
| Engineering | 30 | 73 | 100 |
| Beauty | 37 | 85 | 97 |

A Voltaris board targets 90+ on both, which is above the median of every studied board and
equal to the best of them (see the scorecard).

## Engine checks this study drove

The audit of these boards also exposed mistakes in Voltaris's own auditor, now fixed and
covered by tests: 45° bends were counted as acute corners; pads that share a number (castellated
pins) and pads inside copper pours were not treated as connected; arc tracks were ignored; pin
labels on the bottom side or as GPIO numbers were missed; a rail named VDD_NRF counted as an RF
signal; and footprint text at 180° (which KiCad keeps upright) was flagged as upside down.
