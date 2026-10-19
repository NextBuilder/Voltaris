# First-article bring-up plan

Write this for the specific board, naming its rails and parts. Do every step on every board of
the first batch; stop and investigate at the first deviation.

1. **Visual** — inspect under magnification: solder bridges, missing or rotated parts, polarity
   of diodes, LEDs, electrolytics and the battery connector.
2. **Resistance to ground, unpowered** — measure every rail to GND. A near-zero reading is a
   short; find it before power.
3. **Current-limited first power** — bench supply at the nominal input with the current limit
   just above the expected idle current. Note the current; compare with the power budget.
4. **Rails** — measure every rail's voltage and ripple against its target.
5. **Thermal** — after 5 minutes, touch-test or IR-check regulators, chargers and drivers;
   compare with calc_regulator_thermal.
6. **Programming** — flash the generated bring-up sketch (from `generate_firmware_kit`)
   over native USB or the auto-program UART.
7. **Peripherals** — run the bring-up sketch: LEDs, pixels, I2C scan finds every expected
   address, buttons, ADC readings plausible.
8. **Radio** — Wi-Fi/BLE scan and RSSI at a fixed distance versus a known-good dev board.
9. **Battery path** (if fitted) — charge current, charge termination LED, switchover with USB
   unplugged under load, protection cut-off.
10. **Record** — measurements per board serial; every deviation becomes a fix in the next revision.
