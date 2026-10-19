# Connector standards for maker boards

Always confirm against the exact part's datasheet before placing. Pin 1 is the pin marked on
the connector housing and in the KiCad footprint.

## Qwiic (SparkFun) / STEMMA QT (Adafruit)

| Pin | Signal | Cable colour |
|---|---|---|
| 1 | GND | Black |
| 2 | V+ (3.3 V on Qwiic; STEMMA QT boards accept 3–5 V) | Red |
| 3 | SDA | Blue |
| 4 | SCL | Yellow |

- Connector: JST SH, 1.0 mm pitch, 4 pins, right-angle SMD (BM04B-SRSS-TB or SM04B-SRSS-TB).
- KiCad footprint: `Connector_JST:JST_SH_SM04B-SRSS-TB_1x04-1MP_P1.00mm_Horizontal`.
- I2C bus is 3.3 V logic. Add 2.2–10 kΩ pull-ups with a solder jumper so the user can cut them
  when many boards share the bus.
- Put two connectors on opposite edges so boards daisy-chain; add an address-select jumper if
  the chip has address pins.

## Grove (Seeed Studio)

| Pin | Cable colour | I2C | UART | Digital | Analog |
|---|---|---|---|---|---|
| 1 | Yellow | SCL | RX | Dn | An |
| 2 | White | SDA | TX | Dn+1 | An+1 |
| 3 | Red | VCC | VCC | VCC | VCC |
| 4 | Black | GND | GND | GND | GND |

- Connector: 2.0 mm pitch, 4 pins, latching (HY2.0-4P), vertical or right-angle.
- VCC is 3.3 V or 5 V depending on the host; the module must accept both or say which it needs.
- Label the port type on the silkscreen next to the connector ("I2C", "UART", "D2").

## Gravity (DFRobot)

- PH2.0 (2.0 mm) connectors with colour-coded headers that show the pin's function.
- I2C modules use 4 pins labelled "+", "−", "C", "D" (VCC, GND, SCL, SDA).
- Digital and analog modules use 3 pins; follow the module family's documented order and label
  every pin on the silkscreen, because Gravity users rely on the labels.
- Gravity designs aim to survive a reversed plug — add reverse-polarity protection on VCC if you
  build a Gravity-compatible board.

## 0.1" headers (Arduino, Feather, Pico, breakout boards)

- 2.54 mm pitch; row spacing a multiple of 2.54 mm.
- Label every pin on both sides of the board; use the MCU's pin names (D0, A0, GP2, IO4) and
  the bus function where it is fixed (SDA, SCL, TX, RX, MOSI).
- Square pad for pin 1 and for the GND pin of power headers.

## Power inputs

- Screw terminals: label "+" and "−" (or the rail name) on the silkscreen, and the voltage range.
- Barrel jacks: centre-positive is the convention; print it with the polarity symbol.
- USB-C sink: 5.1 kΩ from each CC pin to GND, ESD protection beside the connector, shield tied
  to GND through a 1 MΩ resistor parallel with a 4.7 nF capacitor or directly, per your EMC plan.
