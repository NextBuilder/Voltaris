# Maker brand silkscreen styles

What the boards people recognise have in common, and what each brand adds. Use these as a style
to agree with the user at Gate 0 — never copy a brand's name, logo or trademark onto a board
that is not theirs.

## Shared by all of them

- Every pin of every header labelled, short and in capitals (5V, 3V3, GND, SDA, SCL, TX, RX).
- One label size, one designator size, text upright and aligned to the header rows.
- Product name and version on the top, brand/URL on the top or bottom.
- Connectors on the edges with their function printed beside them.
- Generous empty space: they leave copper-free, text-free margins rather than fill every gap.

## Arduino

- Header groups captioned along the board edge: POWER, ANALOG IN, DIGITAL (PWM~).
- "~" before the number of every PWM-capable pin.
- Pin numbers and functions both printed where room allows (e.g. "A4/SDA").
- Board name prominent; status LEDs labelled (ON, L, TX, RX).

## Adafruit (Feather and breakouts)

- Labels on both sides; the bottom repeats the pin names so the board reads in a breadboard.
- Board name and logo on the top, often with the product number.
- STEMMA QT connectors on opposite edges with the I2C labels beside them.
- Solder jumpers labelled with what cutting or bridging them does.

## SparkFun

- Labels on both sides; jumpers and pads named by function.
- Qwiic connectors with the bus named; board name and version on the board.
- Large, readable labels on hand-solder pads and test points.

## Seeed Studio (Grove, XIAO)

- Grove connectors with the interface printed beside each one (I2C, UART, D0/D1, A0/A1).
- XIAO-style modules: pin labels beside castellated pads on the top and full pin tables on the
  bottom.
- Product name and version on the module; minimal designators on very small boards.

## Waveshare

- Interface tables: header pins labelled with their signal and often grouped into boxes
  (power, control, data).
- Board name and version on the top; onboard switches and jumpers labelled with their options.
- Status LEDs and buttons named (PWR, RST, BOOT).

## DFRobot (Gravity)

- Colour-coded Gravity connectors whose pin order matches the cable: label every pin.
- Module name and product code (SKU) printed on the board.
- Power and signal pins marked at the connector with short, upright labels.

## Turning a style into settings

| Choice | Where it goes |
|---|---|
| Labels on both sides | `polish_board` both_sides (default on) |
| Designators shown or hidden | `polish_board` references: tidy (default) or hide |
| Designator height | `polish_board` ref_size (1.0 mm default, 0.8 mm minimum) |
| Name, version, brand line | `polish_board` title, version, back_text |
| Logo | `import_svg_logo`, in clear space, never across copper |
| Header group captions, "~" PWM marks | `add_board_text`, at the function-level size |
