# Proven circuit blocks

The blocks maker product boards are built from, as the studied boards use them, each with the
layout rule that makes it work. Values are starting points: confirm every one against the
part's datasheet with `read_datasheet` before the schematic is final, and check stock with
`search_jlcpcb_parts`.

## 1. USB-C (USB 2.0 device or power sink)

| Item | Value / part | Why |
|---|---|---|
| CC1, CC2 | 5.1 kΩ to GND, one resistor each | Without them a C-to-C charger never turns on VBUS |
| D+ / D− | Join the A and B side pads (A6–B6, A7–B7); series resistors only if the MCU datasheet asks (RP2040: 27 Ω) | Reversible plug |
| ESD | A low-capacitance USB TVS array (USBLC6-2, SP0503, SRV05-4) | The port is the most touched part of the board |
| VBUS | Bulk 10 µF, optional PTC fuse, TVS for hot-plug spikes on anything over 1 A | |

Layout: ESD array between the connector and everything else, within a few millimetres of the
pins; D± routed as a 90 Ω coupled pair (`route_board` does this) over unbroken ground, with a
ground via beside any pair via; receptacle shell pads to ground; connector centred on its edge.

## 2. Input protection

| Threat | Block |
|---|---|
| Reversed battery or DC jack | P-MOSFET "ideal diode" (AO3401A class) — no diode drop; or a Schottky (SS34) when the drop is acceptable |
| Overcurrent / short on an output | Resettable PTC fuse (1812/1206) sized for the normal load |
| Surges on long wires (12–24 V inputs, RS-485) | TVS at the terminal: SMAJ with a standoff just above the working voltage (SMAJ24 on a 12–24 V input, SMAJ6.8CA bidirectional on RS-485 lines) |
| Wrong adapter | Print the input range at the jack ("INPUT 12V-24V") and choose a regulator rated above the worst adapter |

Layout: protection parts in the order the current meets them — connector, fuse, TVS, reverse
protection, then the regulator — all within a short, wide path.

## 3. Linear regulators (LDO)

| Part class | Current | Use |
|---|---|---|
| AP2127K-3.3 / ME6211 | 300–500 mA | Small MCU boards (Glyph Mini) |
| SPX3819 / MIC5219 | 500 mA | ESP32 boards with radio bursts |
| LD1117 / AMS1117 | 800 mA–1 A | Higher current, but ~1 V+ dropout: not from a single Li-ion cell |

Rules: input and output capacitors as the datasheet states, at the pins; size the heat with
`calc_regulator_thermal` — (Vin − Vout) × I above roughly half a watt in SOT-23 means a buck
instead. Radio modules (ESP32, nRF) need a 3V3 rail that rides through current bursts: 10–22 µF
at the module's supply pin.

## 4. Switching regulators (buck and boost)

Rules (every switcher): the input capacitor across VIN and GND pins with the shortest loop on
the top layer; the inductor beside the SW pin; the SW node as small as possible (it radiates);
the feedback divider at the FB pin, its trace away from the inductor; a cluster of ground vias
under the IC and the input capacitor ground; output capacitors at the inductor output. Use
`calc_buck` for the inductor and capacitors. Label the block with its output ("FIXED 5V BUCK 2A").

## 5. Li-ion / LiPo charging and power path

| Need | Block |
|---|---|
| Simple 1-cell charge, ≤1 A | TP4056 (linear; RPROG 1.2 kΩ ≈ 1 A); thermal pad to the ground pour with vias |
| Small cell, ≤500 mA | MCP73831 class |
| Run the load while charging, solar or wide input | Power-path charger (BQ24074 class) |
| USB/battery automatic switchover | P-MOSFET + Schottky load sharing: USB powers the load and charges; the battery takes over when USB is gone |
| Battery voltage reading | 100 kΩ/100 kΩ divider to an ADC pin, behind a solder jumper or MOSFET so it does not drain the cell |

Board marks: "+"/"−" at the battery connector (JST PH 2.0 mm is the maker standard — check
polarity, it varies between batteries), CHRG/FULL LED labels, the charge current printed beside
a current-select jumper ("0.5 A" / "1.0 A"). Never let the board accept more than 4.2 V on the
battery pin; say so on the bottom silkscreen.

## 6. Microcontrollers and modules

| Part | Essentials |
|---|---|
| RP2040 | 12 MHz crystal with its load caps; QSPI flash (W25Q) within ~10 mm and routed short; BOOTSEL button on QSPI CS through 1 kΩ; 1.1 V core regulator caps at the pins; USB 27 Ω series resistors |
| ESP32-S3 / C3 / C6 modules | Antenna over the board edge with the datasheet keep-out on every layer; EN with 10 kΩ + 1 µF; BOOT and RESET buttons labelled; native USB on S3 GPIO19/20, C3 GPIO18/19, C6 GPIO12/13 |
| nRF52840 | 32 MHz crystal with the load caps its datasheet calculates; DC/DC inductor if the internal converter is used; antenna matching per the reference layout |
| STM32 | 100 nF per VDD pin plus bulk; VDDA filtered; SWD pads (SWDIO, SWCLK, NRST, 3V3, GND) |

Every power pin gets its 100 nF within 2–3 mm; the crystal and its caps sit within 3 mm of the
MCU, with short equal traces, a ground guard and nothing routed underneath.

## 7. USB-to-UART for programming

CH340K/CH340C or CP2102N, with ESP32-style auto-reset: two transistors (a dual N-MOSFET such as
2N7002DW, or two NPNs) driving EN and BOOT from DTR/RTS. Put TX/RX LEDs on the UART lines only
if the chip has dedicated LED pins.

## 8. Switching loads: motors, LEDs, relays

Logic-level N-MOSFET on the low side (AO3400A, IRLML6344 — fully on at 2.5–3.3 V gate),
10–100 Ω gate resistor, 10–100 kΩ gate pull-down so the load stays off during boot, a flyback
diode across every inductive load (1N4148W for small motors, SS14/SS34 for larger ones), and
the load current loop — supply, load, MOSFET, ground return — kept small and wide (2 mm tracks
or pours for motor current on the drone).

## 9. Addressable LEDs (WS2812B)

5 V supply with 100 nF per LED and bulk capacitance at the power entry; a 3.3 V→5 V buffer
(74AHCT125 / SN74AHCT1G125) on the data line from a 3.3 V MCU; 300–500 Ω in series with the
data line near the first LED; the power path sized for the full-white current of every LED.

## 10. Interfaces

| Interface | Block |
|---|---|
| I2C | 2.2–10 kΩ pull-ups behind a labelled solder jumper (several boards on one bus); Qwiic/STEMMA QT or Grove connector |
| RS-485 | Transceiver (THVD14xx class), 120 Ω termination behind a jumper, bidirectional TVS on A/B, a terminal block labelled A+/B−/GND |
| LoRa / sub-GHz | Module on an edge, u.FL or SMA with a 50 Ω feed (CPWG) as short as possible, ground stitching along the feed |
| Debug | SWD or UART test pads (1 mm), labelled, on the bottom if the top is full |

## 11. Status, buttons, jumpers

- Status LEDs: 1 kΩ from 3.3 V is bright enough indoors; group them on one edge, label each
  (PWR, CHRG, STAT, TX, RX).
- Buttons: RESET and BOOT labelled; tactile switches at the edge where a finger reaches them.
- Solder jumpers (triangle pads) for every option a user might change, each labelled with the
  function and the default ("INT PULLUP", "E1", "1A JMP").

## 12. Sensors

Keep temperature, humidity and pressure sensors away from regulators, chargers and the radio;
a slot in the board around the sensor stops heat travelling through the copper. IMUs near the
mechanical centre of a moving board (the drone puts its IMU at the centre of the frame).
