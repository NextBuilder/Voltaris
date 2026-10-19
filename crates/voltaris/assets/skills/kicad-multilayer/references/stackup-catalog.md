# Stackup catalog: measured multilayer boards

Measured from the published KiCad files of each project. "Planes" lists the nets poured on
each layer, outer to outer; "sig" marks a routing layer whose pours are local. Via and track
columns give the most-used sizes. Bottom parts is the share of footprints on the back.

| Board (team) | Layers, thickness | Planes | Main vias (pad / drill) | Main tracks | Bottom parts |
|---|---|---|---|---|---|
| HackRF One (Great Scott Gadgets) | 4, 1.6 mm, ENIG | sig / GND / VCC, VAA / sig | 1.07 / 0.64, 0.79 / 0.41, 0.69 / 0.33 | 0.2, 0.51 | 0 of 437 |
| Cynthion (Great Scott Gadgets) | 6, 1.56 mm | sig / GND / power+GND / power / GND / sig | 0.46 / 0.2, 0.41 / 0.15 | 0.127, 0.51 | 253 of 348 |
| Glasgow revD (Glasgow Embedded) | 6, 1.58 mm, ENIG | sig / GND / −15V / 1V1, 1V8, 3V3 / GND / sig+GND | 0.3 / 0.2 | 0.09, 0.2 | 626 of 782 |
| OrangeCrab r0.2 (Greg Davill) | 6 | sig / GND+power / GND+power / sig+power / GND+power / GND+power | 0.3 / 0.15, 0.28 / 0.15 | 0.121, 0.15 | — |
| ULX3S (Radiona) | 4, 1.6 mm | GND / power (1V1, 2V5, 3V3) / GND / GND | 0.42 / 0.2 | 0.127, 0.19, 0.3 | 168 of 235 |
| Kria K26 baseboard (Antmicro) | 8, 1.28 mm, ENIG | sig / GND / power / GND / power / GND / power / sig | 0.45 / 0.1, 0.6 / 0.25 | 0.109, 0.182 | 234 of 660 |
| Jetson Orin baseboard (Antmicro) | 8, 1.38 mm, ENIG | sig / GND+power / GND / — / GND / 5V / GND / sig | 0.45 / 0.1 | 0.125–0.16 | 383 of 681 |
| LPDDR4 test board (Antmicro) | 8, 1.6 mm | sig / GND / GND / GND / DDR power / 3V3, 5V / power / sig | 0.5 / 0.2 | 0.1–0.2 | 152 of 497 |
| HDMI–MIPI bridge (Antmicro) | 4, 0.91 mm | GND / GND / power splits / GND | 0.6 / 0.2, 0.4 / 0.15 | 0.1, 0.4 | 26 of 121 |
| USB-C power adapter (Antmicro) | 4, 1.23 mm | sig / 5V, GND / 12V, 3V3, VBUS / sig | 0.55 / 0.25, 0.45 / 0.2 | 0.2 | 46 of 124 |
| Environment sensor (Antmicro) | 4, 1.6 mm | sig / GND / 3V3, VBUS / GND | 0.6 / 0.25 | 0.15, 0.25 | 4 of 109 |
| Signal-integrity test board (Antmicro) | 4, 1.6 mm | sig / GND / GND / sig | 0.55 / 0.2 | 0.185 (50 Ω) | 0 of 28 |
| Bitaxe Ultra and Gamma (Bitaxe) | 4, 1.6 mm | power pours / GND / 3V3 / power pours | 0.8 / 0.4, 0.5 / 0.3 | 0.254, 0.2, 1.5 | 17–20 of ~140 |
| Castor & Pollux (Winterbloom) | 4, 1.6 mm, ENIG | sig / GND / GND / sig | 0.69 / 0.33 | 0.2, 0.4 | 183 of 235 |
| CatSniffer (Electronic Cats) | 4, 1.6 mm | sig / GND / 3V3, radio rails / GND | 0.6 / 0.3 | 0.2, 0.25 | 5 of 100 |
| BMS C1 (Libre Solar) | 4, 1.58 mm | battery and pack nets poured on all four layers | 0.8 / 0.4, 0.7 / 0.3 | 0.25, 0.5 | 0 of 210 |
| xESC 2 (Clemens Elflein) | 4 | motor pours / GND / 3V3, VM / motor pours | 0.45 / 0.2, 0.6 / 0.3 | 0.127, 0.5 | 66 of 114 |
| ESP32-PoE (OLIMEX) | 4 | GND and power pours on every layer | 0.7 / 0.4, 0.9 / 0.6 | 0.15, 0.51, 1.02 | — |
| RP2040-PICO30 (OLIMEX) | 4 | sig / GND / 3V3 / sig | 0.7 / 0.4, 1.0 / 0.6 | 0.2, 1.78 | — |
| HadesFCS (pms67) | 4 | sig / GND / 3V3, 5V / GND | 0.8 / 0.4 | 0.25, 0.5 | — |

## What the catalog shows

- Ground is the first inner layer on 20 of the 21 boards; ULX3S is the deliberate exception
  (power on layer 2, ground on layers 3 and 4 and the top pour).
- Four-layer boards that carry RF or precision analog give **both** inner layers to ground, so
  each outer layer has its own plane (Antmicro's SI board, Castor & Pollux).
- The denser the board, the smaller the via and the more parts on the bottom: 0.6 / 0.3 vias and
  single-side assembly at 4 layers, 0.3 / 0.15 vias and most passives on the bottom at 6.
- Power-electronics boards (BMS C1, Bitaxe, xESC) pour the high-current nets on several layers
  in parallel and join them with via fields — see the kicad-power-electronics skill.
- ENIG is the finish written into HackRF, Glasgow, Castor & Pollux and the Antmicro SoM
  baseboards: flat pads for fine pitch and BGA.
