# Common form factors

Sizes are the published board outlines. Before ordering, check the official drawing for the
exact hole and header positions of the revision you are matching.

| Form factor | Board size | Mounting | Notes |
|---|---|---|---|
| Arduino Uno shield | 68.6 × 53.3 mm | 4 holes, 3.2 mm | Headers on the Uno R3 pattern; the D7–D8 header gap is 160 mil (not 100) by design |
| Arduino Nano footprint | 18 × 45 mm | none | Two 15-pin rows, 15.24 mm (0.6") apart |
| Raspberry Pi HAT | 65 × 56.5 mm | 4 × M2.5, centres 3.5 mm from edges (58 × 49 mm pattern) | 40-pin header along one long edge; HAT spec also asks for an ID EEPROM |
| Raspberry Pi Pico footprint | 51 × 21 mm | 4 holes, 2.1 mm | Two 20-pin rows, 17.78 mm (0.7") apart, castellated |
| Adafruit Feather | 50.8 × 22.86 mm (2.0 × 0.9") | 4 corner holes | 16-pin and 12-pin headers on the long edges, 20.32 mm (0.8") apart |
| Seeed XIAO | 21 × 17.8 mm | none | Two 7-pin castellated rows; USB-C on the short edge |
| Waveshare RP2040-Zero | 18 × 23.5 mm | none | Two 9-pin castellated rows plus pads on the bottom; the other "-Zero" boards share the style but not always the size |
| SparkFun / Adafruit breakout | multiples of 0.1" (e.g. 1.0 × 1.0") | 2–4 holes on the 0.1" grid | Qwiic/STEMMA QT on opposite edges |

## Designing your own family

- Choose a base size on the 2.54 mm grid and reuse it: 25.4 × 25.4 mm (1 × 1") for sensor
  breakouts, 50.8 × 25.4 mm (2 × 1") for controllers.
- Same hole size, hole inset and corner radius on every board in the family, so they stack with
  standoffs and share an enclosure.
- Same connector standard on the same edge across the family.
