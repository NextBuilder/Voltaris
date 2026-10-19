---
name: kicad-esp32
description: |
  Designing boards around Espressif ESP32-family modules in KiCad: choosing ESP32 / -S3 / -C3 /
  -C6, power, boot and strapping pins, native USB versus a USB-UART bridge, pin assignment,
  antenna placement and firmware bring-up. Triggers on: "ESP32", "ESP32-S3", "ESP32-C3",
  "ESP32-C6", "WROOM", "MINI-1", "ESP module", "Wi-Fi board", "IoT board", "strapping pins".
argument-hint: "[what the ESP32 board must do]"
---

# ESP32 Hardware Design

```
load_toolset('templates')        # search_templates apply_template
load_toolset('firmware')         # generate_firmware_kit
load_toolset('engineering')      # calc_regulator_thermal calc_passive
load_toolset('visual_review')    # render_board_3d
```

## Choose the chip

| | ESP32 | ESP32-S3 | ESP32-C3 | ESP32-C6 |
|---|---|---|---|---|
| CPU | 2× Xtensa LX6 | 2× Xtensa LX7, vector instructions | 1× RISC-V | RISC-V (HP + LP core) |
| Radio | Wi-Fi 4, Bluetooth Classic + BLE | Wi-Fi 4, BLE 5 | Wi-Fi 4, BLE 5 | Wi-Fi 6, BLE 5, 802.15.4 (Thread/Zigbee) |
| USB | none — needs a USB-UART bridge | USB OTG on GPIO19/20 | USB Serial/JTAG on GPIO18/19 | USB Serial/JTAG on GPIO12/13 |
| Strapping pins | GPIO0, 2, 5, 12, 15 | GPIO0, 3, 45, 46 | GPIO2, 8, 9 | GPIO8, 9, 15 (+ MTMS, MTDI) |
| Good for | legacy designs, Bluetooth Classic audio | displays, cameras, AI, USB devices | low-cost sensors and controllers | Matter/Thread, Wi-Fi 6, low power |

Prefer certified modules (WROOM, MINI) over bare chips: the RF, flash and crystal are done and
the module carries radio certification. The template library has verified cores for
ESP32-S3-WROOM-1, ESP32-C3-WROOM-02 and ESP32-C6-MINI-1, plus the USB-C port, CH340C
auto-program, LDO and battery circuits.

## Non-negotiables

1. **Power**: a 3.3 V supply good for ≥ 500 mA peaks, bulk capacitance (10–22 µF) and 100 nF at
   the module's 3V3 pin. Brown-outs during Wi-Fi transmit are the most common ESP32 board bug.
2. **EN**: 10 kΩ pull-up and 1 µF to ground (RC delay), plus a RESET button. Never floating.
3. **Boot**: a BOOT button on the boot strapping pin (GPIO0, or GPIO9 on C3/C6) with a pull-up.
   Nothing else may drive a strapping pin away from its boot level at reset.
4. **Programming path**: native USB (S3/C3/C6) through a USB-C port with ESD, or a USB-UART
   bridge with the two-transistor auto-program circuit (classic ESP32).
5. **Antenna**: module at the board edge, antenna over the edge or over a copper-free keep-out
   on every layer.

## Pin assignment

After the schematic exists, run `generate_firmware_kit`. Its audit flags: strapping pins used
by peripherals, input-only pins asked to drive, analog signals on ADC2 (unusable while Wi-Fi
runs on the classic ESP32 and unreliable on others — use ADC1), USB pins used as GPIO, and
flash/octal-PSRAM pins. Fix every warning in the schematic, or record why it is safe.

Prefer: I2C and SPI on any free GPIOs (the GPIO matrix routes them); analog inputs on ADC1;
LEDs and buzzers off strapping pins; one GPIO per button with the internal pull-up.

## Bring-up

`generate_firmware_kit` with an output directory writes the pin header and an Arduino
bring-up sketch: flash it first on every new board and follow the bring-up plan from the
kicad-pro-workflow skill.
