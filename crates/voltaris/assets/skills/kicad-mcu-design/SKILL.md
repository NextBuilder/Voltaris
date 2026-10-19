---
name: kicad-mcu-design
description: |
  Designing boards around Raspberry Pi RP2040 / RP2350, ST STM32 and Nordic nRF52 parts in
  KiCad: choosing the part, the minimum circuit each one needs (supplies, decoupling, clocks,
  boot, flash, debug, USB), pin assignment and firmware bring-up. Triggers on: "RP2040",
  "RP2350", "Pico", "STM32", "STM32G0", "STM32F4", "Blue Pill", "nRF52", "nRF52840",
  "nRF52832", "BLE board", "MDBT50Q", "SWD".
argument-hint: "[what the board must do]"
---

# RP2040 / RP2350, STM32 and nRF52 Hardware Design

```
load_toolset('datasheet')        # read_datasheet — the reference circuit is the authority
load_toolset('firmware')         # generate_firmware_kit — pin audit and bring-up code
load_toolset('engineering')      # calc_passive (crystal load caps), calc_regulator_thermal
load_toolset('design_checks')    # store each datasheet requirement as a check
```

Every number below is a starting point. Before the schematic is final, open the part's own
hardware design guide or reference circuit with `read_datasheet`, cite the page, and follow it
where it differs.

## Choose the part

| Need | Good fit |
|---|---|
| Cheap, fast, lots of GPIO, USB device, PIO for odd protocols | RP2040 |
| As RP2040 plus security, more RAM, faster cores, more ADC (RP2350B) | RP2350A (30 GPIO) / RP2350B (48 GPIO); RP2354 has 2 MB flash inside |
| Low power, precise analog, CAN, many timers, long-term supply | STM32 (G0/C0 for cost, G4 for analog/motor, L4/U5 for low power, F4/H7 for speed) |
| Bluetooth LE / Thread / Zigbee on a battery | nRF52840 (USB, 1 MB flash) or nRF52832 — on a certified module |

## RP2040 minimum circuit

| Block | What to fit |
|---|---|
| Supplies | IOVDD, USB_VDD, ADC_AVDD at 3.3 V; 100 nF on every IOVDD and DVDD pin |
| Core | Internal LDO: VREG_VIN (3.3 V, 1 µF) → VREG_VOUT (1 µF) → both DVDD pins (1.1 V) |
| Flash | External QSPI NOR flash (e.g. W25Q16JV, up to 16 MB) on the QSPI pins, close to the chip |
| BOOTSEL | Button from QSPI_SS to GND through 1 kΩ — held at reset it starts the USB bootloader |
| Clock | 12 MHz crystal (the USB bootloader needs exactly 12 MHz) with load caps from `calc_passive`, and 1 kΩ in series on XOUT as in Raspberry Pi's minimal design |
| USB | 27 Ω series resistors on USB_DP and USB_DM, placed next to the chip; route as a 90 Ω pair |
| Reset / test | RUN to a reset button (pulled high); TESTEN to GND |
| Debug | SWCLK, SWDIO, GND to a header or test pads |
| ADC | Only GPIO26–29; ADC_AVDD gets its own 100 nF (filter it if you need accuracy) |

## RP2350 differences

- The core supply is a **switching regulator**: VREG_LX → 3.3 µH inductor → DVDD, with VREG_FB
  to DVDD, VREG_PGND to ground and an RC-filtered VREG_AVDD. It is layout-sensitive — copy the
  regulator layout and inductor part from Raspberry Pi's RP2350 minimal design exactly.
- RP2350A has GPIO0–29 (ADC on 26–29); RP2350B has GPIO0–47 (ADC on 40–47).
- Erratum E9: on early silicon an input relying on the internal pull-down can latch high —
  wire buttons active-low with pull-ups (`generate_firmware_kit` warns about this).

## RP2040 / RP2350 pin rules

- I2C: SDA on GPIO 0, 2, 4, 6 … and SCL on 1, 3, 5, 7 …; GPIO 4k/4k+1 is I2C0 and 4k+2/4k+3 is
  I2C1 — SDA and SCL must be on the **same** controller. `generate_firmware_kit` checks this.
- UART0 defaults to GPIO0 (TX) / GPIO1 (RX), the usual debug console.
- GPIO25 drives the LED on the Pico boards; on your own board any GPIO works.

## STM32 minimum circuit

| Block | What to fit |
|---|---|
| Supplies | 100 nF on every VDD pin plus one 4.7–10 µF bulk; VDDA 1 µF + 10 nF (add a ferrite for analog work); VBAT to VDD with 100 nF when no backup battery |
| Core | F4/F7/H7 parts have VCAP pins: low-ESR ceramic (2.2 µF on F4) exactly as the datasheet states — never leave them open |
| Reset | NRST: 100 nF to GND (the internal pull-up does the rest), plus a button if wanted |
| Boot | BOOT0 to GND through 10 kΩ where the pin exists (G0/C0 use an option byte and PA14-BOOT0) |
| Clock | HSE crystal on PH0/PH1 (PF0/PF1 on F0/G0/C0/L0) with load caps from `calc_passive`; check oscillator margin per ST AN2867. LSE 32.768 kHz on PC14/PC15 only for RTC |
| USB FS | PA11 = DM, PA12 = DP. Most families have the 1.5 kΩ DP pull-up built in (F1 does not). USB needs an accurate 48 MHz: a crystal, or HSI48 with CRS on parts that have it |
| Debug | SWDIO PA13, SWCLK PA14, NRST, GND, VDD to a 10-pin 1.27 mm Cortex debug header or Tag-Connect; SWO on PB3 optional |

STM32 pin rules (all checked by `generate_firmware_kit`): keep PA13/PA14 for SWD; PA15, PB3,
PB4 are JTAG after reset; PC13–PC15 are weak backup-domain pins (no LEDs); PB2 is BOOT1 on F1/F4.
Pick the exact part and alternate functions in STM32CubeMX before routing — the ADC channel,
timer and peripheral on each pin depend on the part number.

## nRF52 minimum circuit

**Use a certified module** (Raytac MDBT42Q / MDBT50Q, Fanstel, Ebyte E73 and similar) unless you
can tune and certify RF. A module includes the crystal, DC/DC inductors, matching network and
antenna; you supply power, decoupling, SWD and the antenna keep-out from its datasheet.

Chip-down designs copy Nordic's reference circuitry from the product specification exactly:
32 MHz crystal (XC1/XC2) with its load caps, DC/DC inductors on DCC (and DCCH on nRF52840 for
VDDH), every DEC pin capacitor, the antenna matching network and a 50 Ω feed (`calc_impedance`).

| Topic | Rule |
|---|---|
| DC/DC | Firmware may only enable the DC/DC regulator when its inductors are fitted |
| Supply | nRF52840 can run from a Li-ion cell on VDDH (high-voltage mode) and from USB VBUS |
| Low-frequency clock | 32.768 kHz crystal on XL1/XL2 is optional (the calibrated internal RC meets BLE timing) — without it those pins are free GPIO |
| NFC pins | P0.09/P0.10 are NFC by default — GPIO only after UICR.NFCPINS is set |
| Reset | P0.21 (nRF52832) / P0.18 (nRF52840) is pin reset when UICR.PSELRESET is set |
| Analog | SAADC inputs only on AIN0–AIN7 pins |
| Slow pins | The product specification marks some pins "low frequency I/O only" near the radio — keep clocks and fast buses off them |
| Debug | SWDIO, SWDCLK, reset, VDD, GND to a header or Tag-Connect |

## Bring-up

1. `generate_firmware_kit` with the framework the user will write in:
   RP: `arduino` (arduino-pico) or `pico-sdk`; STM32: `arduino` (STM32duino) or `stm32-hal`;
   nRF52: `nrfx` (header only — Arduino nRF52 cores number pins per board).
2. Every warning is fixed in the schematic or accepted by the user in writing.
3. First power-up follows the kicad-pro-workflow bring-up plan: current-limited supply, rails
   measured, then SWD connection, then the generated sketch.
