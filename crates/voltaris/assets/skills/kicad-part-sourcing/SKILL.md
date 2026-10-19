---
name: kicad-part-sourcing
description: |
  Selecting parts that can actually be bought and assembled: JLCPCB/LCSC stock and part numbers,
  alternates, lifecycle and supply risk, and importing parts KiCad's libraries do not have.
  Triggers on: "find a part", "LCSC", "JLCPCB part", "C-number", "in stock", "basic part",
  "extended part", "alternative part", "BOM cost", "import part", "missing footprint",
  "easyeda", "not in KiCad library".
argument-hint: "[function or part number]"
---

# Part Sourcing

```
load_toolset('integration')      # search_jlcpcb_parts get_jlcpcb_part suggest_jlcpcb_alternatives get_datasheet_url
load_toolset('lcsc_import')      # import_lcsc_part
load_toolset('design_review')    # check_bom_health
load_toolset('library')          # search_symbols search_footprints
```

## Selection order

1. **Function and specs first**, from the datasheet: voltage, current, package, temperature.
2. **Availability**: look up live stock and price with `search_jlcpcb_parts` and
   `get_jlcpcb_part`. Never quote stock, price or Basic/Extended status from memory — they
   change daily; quote the lookup result with its date.
3. **Assembly cost**: JLCPCB charges extra per unique Extended part, so prefer Basic parts for
   passives and common ICs. Check the current fee on JLCPCB rather than assuming one.
4. **Second source**: for every active part, find an alternate with `suggest_jlcpcb_alternatives`
   (same footprint and pinout) and note it in the BOM.
5. **Lifecycle**: avoid parts marked not-recommended, obsolete or with very low stock.

## Getting the part into KiCad

1. Search KiCad's own libraries (`search_symbols`, `search_footprints`) — prefer them.
2. Otherwise import by LCSC number with `import_lcsc_part`: symbol, footprint and 3D model land
   in the project's own library and are registered automatically.
3. **Accept every imported or custom part** against the datasheet before use: pin numbers,
   names and electrical types, footprint pad numbering and dimensions, pin 1 and orientation,
   3D model alignment — the kicad-library skill's physical pin-map acceptance contract.
   EasyEDA library data is community-made and sometimes wrong.
4. Put the LCSC number and MPN on every symbol so the BOM is assembly-ready.

## Before ordering

Run `check_bom_health`: every part needs an MPN and LCSC number, stock above the order
quantity, and no lifecycle warnings. Report anything single-sourced.
