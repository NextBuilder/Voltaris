# Standards and sources: where a rule comes from

A rule is only as good as its source. Use this page to decide which source wins, which standard
covers a question, and which Voltaris tool turns it into a check.

## Which source wins

| Tier | Sources | Use for |
|---|---|---|
| 1 | The part's datasheet, its errata, the maker's reference design and evaluation-board layout, application notes, official standards | Every value around a part, every layout rule for a part, every safety and compliance decision |
| 2 | Textbooks and peer-reviewed papers (below) | The physics behind a rule; checking a Tier-3 claim |
| 3 | Articles, videos, forums, other boards | Ideas and patterns — never the only authority for a safety or rating decision |

When sources disagree, the higher tier wins; within a tier, the part's own datasheet wins over a
generic rule. Record the datasheet page behind each decision in `.voltaris/project.json`
(`read_datasheet` finds it).

## Standards map

Revisions change and documents get superseded. Name a standard in fab notes only after checking
its current revision with IPC or IEC; quote the class or clause, not just the number.

They answer different questions, and passing one stage proves nothing about the next: design
standards (IPC-2221/2222, 2152, 7351) say how to draw the board, performance standards
(IPC-6012) what the made board must achieve, process standards (J-STD-001) how it is soldered,
acceptance standards (IPC-A-600 bare board, IPC-A-610 assembled) how it is inspected, and IEC
test standards how the finished product is tested.

| Question | Standard | Voltaris check |
|---|---|---|
| General design rules, conductor spacing by voltage (bare board) | IPC-2221 (generic), IPC-2222 (rigid) | `set_design_rules`, `check_clearance` |
| Trace and plane current vs temperature rise | IPC-2152 | `calc_trace_width` |
| Flex and rigid-flex | IPC-2223 | — |
| HDI, microvias, via-in-pad stacks | IPC-2226 | kicad-multilayer |
| RF and microwave boards | IPC-2252 design guide (confirm the current RF sectional standard with IPC) | `calc_impedance` |
| Land patterns | IPC-7351 (being replaced by IPC-7352) | `get_footprint_info`, `create_footprint` |
| Base materials, high-speed laminates | IPC-4101, IPC-4103 | `list_stackups` |
| Board performance: Class 2 (products) or Class 3 (high reliability) | IPC-6011 / 6012 (rigid), 6013 (flex), 6018 (RF) | fab notes |
| Via filling and capping (type VII for via-in-pad) | IPC-4761 | `audit_layout` via-in-pad check |
| Bare-board and assembly acceptance; soldering | IPC-A-600, IPC-A-610, J-STD-001 | — |
| Intelligent manufacturing data | IPC-2581, ODB++ | `export_ipc2581`, `export_odb` |
| Creepage and clearance for mains or high voltage | IEC 60664-1 (up to 1000 V AC / 1500 V DC) with the product standard (IEC 62368-1 for ICT and AV) — not the IPC-2221 table. Inputs: working voltage, insulation function (basic or reinforced), overvoltage category, pollution degree, material group, altitude | kicad-power-electronics |
| ESD, burst, surge, conducted and radiated immunity | IEC 61000-4-2 / -4 / -5 / -6 / -3 | ESD at the connector, `run_design_review` |
| Emissions and immunity of multimedia equipment | CISPR 32, CISPR 35 | kicad-design-masters sections 5–7 |
| Cold, heat, vibration, shock, humidity | IEC 60068-2-1, -2-2, -2-6, -2-27, -2-14, -2-30, -2-78 — chosen from the real operating and storage environment | Gate 0 brief |
| Lithium batteries: portable, industrial, transport | IEC 62133-2, IEC 62619, UN 38.3 | `audit_power_rails`, Gate 6 |
| Military and space | MIL-PRF-31032, MIL-STD-883 / 202, NASA workmanship, ECSS-Q-ST-70 | only when a contract invokes them |
| ESD control during handling | ANSI/ESD S20.20 | — |

A standard applies when the product's market, category or contract invokes it — not because it
exists. Ask at Gate 0 where the product will be sold and what it plugs into.

## Textbooks by discipline

| Discipline | Read first | Then |
|---|---|---|
| PCB design and fabrication | Coombs and Holden, *Printed Circuits Handbook* | Peter Wilson, *The Circuit Designer's Companion*; Lee Ritchey, *Right the First Time* |
| Signal integrity | Eric Bogatin, *Signal and Power Integrity — Simplified* | Johnson and Graham, *High-Speed Digital Design*; Hall, Hall and McCall, *High-Speed Digital System Design* |
| EMC | Henry Ott, *Electromagnetic Compatibility Engineering* | Tim Williams, *EMC for Product Designers*; Bruce Archambeault, *PCB Design for Real-World EMI Control*; Clayton Paul, *Introduction to EMC* |
| RF and antennas | Christopher Bowick, *RF Circuit Design* | David Pozar, *Microwave Engineering*; Constantine Balanis, *Antenna Theory*; Edwards and Steer, *Foundations for Microstrip Circuit Design* |
| Power conversion | Erickson and Maksimović, *Fundamentals of Power Electronics* | Pressman, *Switching Power Supply Design*; Sandler, *Power Integrity*; Smith and Bogatin, *Principles of Power Integrity for PDN Design* |
| Thermal | Incropera, *Fundamentals of Heat and Mass Transfer* | the part maker's package thermal notes |

## Manufacturer libraries — what each is best for

| Maker | Go there for |
|---|---|
| Texas Instruments | Converter layout guides and the hot-loop picture in each datasheet, thermal pads, WEBENCH, EMI board design |
| Analog Devices | Data-converter grounding (MT-031), decoupling (MT-101), precision analog, RFI rectification, LTspice models |
| Infineon | MOSFET and GaN/SiC switching, gate drive, power-device cooling |
| STMicroelectronics, NXP, Microchip, Nordic, Silicon Labs, Espressif | MCU hardware design guides: supplies, clocks, boot, USB, RF reference layouts |
| onsemi, MPS, Renesas | Power stages and their evaluation-board layouts |
| Würth Elektronik | Inductors, ferrites and EMC application notes; magnetics data for Isat and losses |
| Samtec, Keysight, Intel | Connector launches, via optimisation, channel simulation, FPGA and DDR layout |
| Rogers, Isola | Dk and Df versus frequency, laminate choice for RF and fast links |
| Eurocircuits, NCAB | Fab capability, stackups, via filling, surface finishes, panelisation |

For every IC on a board: datasheet, errata, hardware design guide, reference schematic, the
evaluation board's layout files, then app notes — in that order.

## Tools, in the order a project needs them

1. KiCad with Voltaris, plus quick calculators: `calc_trace_width`, `calc_impedance`,
   `calc_via`, `calc_buck`, `calc_regulator_thermal` (Saturn PCB Toolkit for a second opinion).
2. Circuit simulation: `simulate_circuit`, `run_ngspice`; LTspice with the maker's models.
3. Field solvers and SI/PI/thermal tools (HFSS, SIwave, HyperLynx, Sigrity, Icepak) — only when a
   requirement such as a multi-gigabit link, a radiating antenna or a hot power stage justifies it.
4. Mechanical: `generate_enclosure`, `export_3d`, then a CAD tool.

## The documents a finished project has

| Document | Voltaris source |
|---|---|
| Requirements and power budget | Gate 0 brief, `.voltaris/project.json` |
| Architecture and block diagram | `generate_block_diagram` |
| Part selection with datasheet pages | Gate 1–2 notes, `read_datasheet` |
| Footprint validation (pads, pin numbers, paste against the package drawing) | kicad-library skill, `get_footprint_info` |
| Schematic | `export_schematic_pdf` |
| Stackup specification | `get_board_stackup` |
| Layout evidence | `render_board_3d`, `audit_layout`, `score_placement` |
| Simulation results | `simulate_circuit` reports |
| ERC and DRC reports | `run_erc`, `run_drc` |
| Fabrication and assembly package, with the IPC-6012 class in the fab notes | `export_manufacturing_package` |
| Bare-board and assembly inspection records (IPC-A-600, IPC-A-610) | bring-up guide |
| Bring-up and validation plan | `generate_firmware_kit`, the Gate 7 bring-up guide |
| Changes after testing | `diff_design` between snapshots, release notes |

## Principles

1. Know the electrical reason for every important routing decision.
2. A clean DRC is a starting point, not a finished board.
3. Thermal, EMC, return path and manufacturing are design constraints from Gate 0, not review
   items.
4. Calculate first, simulate where it pays, then measure the real board.
5. Never copy a trace width, clearance, via size or thermal-via count without checking it
   against this board's current, voltage and copper.
6. For a single part, its datasheet and recommended layout are the primary reference.
7. Safety and compliance come from the standard the product falls under.
8. Write down assumptions, calculations, results and changes.
9. Manufacturing and assembly are part of the design.
10. Success is measured performance and reliability — and a product board also has to look
    finished, because that is part of what is sold.
