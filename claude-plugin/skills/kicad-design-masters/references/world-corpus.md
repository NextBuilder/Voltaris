# World corpus: 35 open boards, measured and audited

Each board's published KiCad file was measured (stackup, net classes, vias, tracks, sides)
and graded by Voltaris's `audit_layout` — the same engine that grades your board. Grades are
out of 100. Beauty grades reward product-style silkscreen; dense engineering boards that print
every designator at 0.4–0.7 mm grade low there by design, not by accident.

| Board | Team | Layers | Eng. | Beauty | What to copy |
|---|---|---|---|---|---|
| OrangeCrab r0.2 | Greg Davill | 6 | 91 | 86 | ECP5 BGA and DDR3 on a Feather-size board: 0.3 / 0.15 vias, 0.12 mm tracks, power planes beside ground |
| Minino | Electronic Cats | 2 | 90 | 23 | Clean 2-layer routing with ground on both sides |
| MPPT 2420 HC | Libre Solar | 4 | 89 | 96 | Solar charger power stage, shunt sense pair, labelled terminals |
| Bus Pirate 5 rev 2b | Dangerous Prototypes | 4 | 89 | 61 | Many I/O buffers routed tidily on 4 layers |
| BLDC 4 | VESC (Benjamin Vedder) | 4 | 88 | 83 | The motor controller many ESCs grew from: 5 mm supply tracks |
| ESP32-C3-DevKit-Lipo | OLIMEX | 2 | 88 | 60 | LiPo charging and USB on 2 layers, power widths per class |
| ESP32-S3-DevKit-LiPo | OLIMEX | 2 | 88 | 58 | Same pattern for the S3 with native USB |
| HDMI–MIPI bridge | Antmicro | 4 (0.9 mm) | 88 | 30 | 100 Ω HDMI and CSI pairs at 0.1 / 0.1 on a thin board, impedance-named classes |
| Keyboard FeatherWing | Solder Party | 2 | 87 | 92 | Product silkscreen, ground poured on both layers |
| Sol | Winterbloom | 2 | 86 | 93 | Eurorack module craft: art-quality silkscreen |
| MPPT 1210 HUS | Libre Solar | 2 | 84 | 96 | Compact buck power stage |
| BMS C1 | Libre Solar | 4 | 81 | 63 | Pack and battery nets poured on four layers, ~80 vias per net, HV net class |
| Castor & Pollux | Winterbloom | 4 | 81 | 42 | Two inner ground planes for clean analog, 828 curved tracks |
| Signal-integrity test board | Antmicro | 4 | 80 | 66 | 50 Ω reference structures, both inner layers ground |
| ESP32-PoE | OLIMEX | 4 | 78 | 99 | Ethernet with PoE isolation, polished silkscreen |
| Expansion Card (microcontroller) | Framework | 2 | 77 | 97 | A tiny product board with nothing wasted |
| Big Honking Button | Winterbloom | 2 | 76 | 96 | Simple module, beautiful finish |
| BMS 8S50 IC | Libre Solar | 2 | 76 | 96 | 50 A BMS on 2 layers |
| Tigard | Tigard | 4 | 75 | 80 | Hardware-hacking multi-protocol tool, dense 0.15 mm routing |
| Cynthion | Great Scott Gadgets | 6 | 74 | 79 | FPGA, USB HS PHYs and power switching on 6 layers; 253 of 348 parts on the bottom |
| xESC 2 | Clemens Elflein | 4 | 73 | 74 | Small BLDC drive: motor supply on three layers |
| LPDDR4 test board | Antmicro | 8 | 73 | 20 | LPDDR4 routing with 50 Ω DQ and 80 Ω DQS classes |
| Solo | SoloKeys | 4 | 72 | 95 | Security key in a USB-A plug outline |
| USB-C power adapter | Antmicro | 4 (1.2 mm) | 72 | 50 | USB PD power path on a thin board |
| HadesFCS | Phil's Lab (Phil Salmony) | 4 | 70 | 93 | A complete flight-controller board from a teaching channel, ground on layer 2 |
| HackRF One | Great Scott Gadgets | 4 | 70 | 12 | The reference open SDR: RF chain, ground on layer 2, VCC and VAA planes |
| Environment sensor | Antmicro | 4 | 69 | 33 | 90 Ω USB class at 0.182 / 0.196 |
| CatSniffer | Electronic Cats | 4 | 67 | 17 | Multi-radio board with radio rails on their own plane |
| Glasgow revD | Glasgow Embedded | 6 | 64 | 64 | 782 parts, 626 on the bottom, 1500 vias of 0.3 / 0.2, hundreds of filled vias in pads |
| Bitaxe Gamma | Bitaxe | 4 | 64 | 54 | High-current ASIC core supply poured on outer layers |
| RP2040-PICO30 | OLIMEX | 4 | 63 | 96 | RP2040 on a narrow board, 1.78 mm power tracks |
| Bitaxe Ultra | Bitaxe | 4 | 56 | 58 | Earlier Bitaxe, same power pattern |
| ULX3S | Radiona | 4 | 55 | 52 | ECP5 FPGA dev board, power on layer 2 and ground on layers 3–4 |
| Kria K26 baseboard | Antmicro | 8 | 52 | 17 | SoM carrier: ground on every other layer, 0.45 / 0.1 microvias, 14 000 segments at 0.109 mm |
| Jetson Orin baseboard | Antmicro | 8 | 49 | 11 | 85 Ω PCIe/USB SS and 100 Ω HDMI/MDI classes, 1720 arcs |

Medians: engineering 76, beauty 64. Fourteen boards reach 80+ on engineering, eleven reach
90+ on beauty. Twenty-six of the 35 have four or more layers.

## What even these boards miss

How many of the 35 boards each check flagged at least once:

| Check | Boards | Lesson |
|---|---|---|
| Return via on a fast layer change | 28 | The most common miss everywhere — add the ground via beside the signal via |
| Long thin power track | 23 | Size rails with `calc_trace_width` or pour them |
| Connector pins without labels | 20 | Label every pin on the silkscreen |
| Decoupling more than 3 mm from its pin | 20 | Move the capacitor, not the via |
| Designators on vias or pads | 18 | Run `polish_board` to re-place them |
| Acute corners | 18 | 45° or arcs only |
| Via in an unplugged pad | 17 | Order filled vias, or move them |
| No version mark | 17 | Print a revision on every board |
| Unrouted connections | 1 | Shipped boards are complete — and so should yours be |

## Sources

Every board is public on GitHub under its team's account: greatscottgadgets (hackrf,
cynthion-hardware), GlasgowEmbedded/glasgow, antmicro (jetson-orin-baseboard,
kria-k26-devboard, lpddr4-test-board, hdmi-mipi-bridge, usb-c-power-adapter,
environment-sensor, signal-integrity-test-board), LibreSolar (bms-c1, bms-8s50-ic,
mppt-2420-hc, mppt-1210-hus), vedderb/bldc-hardware, ClemensElflein/xESC, skot/bitaxe,
bitaxeorg/bitaxeGamma, OLIMEX (ESP32-POE, ESP32-C3-DevKit-Lipo, ESP32-S3-DevKit-LiPo,
RP2040-PICO30), orangecrab-fpga/orangecrab-hardware, emard/ulx3s, ElectronicCats (CatSniffer,
Minino), wntrblm (Castor_and_Pollux, Big_Honking_Button, Sol), arturo182 (Keyboard FeatherWing hardware),
FrameworkComputer/ExpansionCards, DangerousPrototypes/BusPirate5-hardware, solokeys/solo-hw,
tigard-tools/tigard and pms67/HadesFCS.
