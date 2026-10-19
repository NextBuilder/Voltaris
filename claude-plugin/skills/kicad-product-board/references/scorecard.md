# Product scorecard

"Better than a professional" has to be measured. Run every line, record the evidence, and
report the scorecard with the final design. A line that could not run is INCOMPLETE, never a
pass.

| # | Check | Tool | Target | Studied boards |
|---|---|---|---|---|
| 1 | Design rule check | `run_drc` (live sync, zones refilled) | 0 errors, 0 unconnected items | 8 of 15 left unconnected items |
| 2 | Electrical rule check | `run_erc` | 0 errors | — |
| 3 | Engineering grade | `audit_layout` | 90 or more | median 73, best 100 |
| 4 | Beauty grade | `audit_layout` | 90 or more | median 85, best 97 |
| 5 | Return vias on fast nets | `audit_layout` return-via check | none missing | 10 of 15 missed some |
| 6 | Decoupling distance | `audit_layout` decoupling check | every capacitor within 3 mm | 7 of 15 had some further |
| 7 | Power track widths | `calc_trace_width` per rail, `audit_layout` | every rail sized for its current | 10 of 15 had long thin rails |
| 8 | Differential pairs | `route_board` pairs report, `analyze_net_lengths` | coupled, skew within target | — |
| 9 | Silkscreen | `audit_layout` silkscreen checks, `render_board_3d` top and bottom | no text on copper, none under 0.8 mm, every connector pin labelled | 11 of 15 had unlabelled pins |
| 10 | Placement | `score_placement`, `smart_place` dry run | no hard failures; improve mode finds under 3 % left to gain | — |
| 11 | Your rules | `run_design_checks` | pass | — |
| 12 | Independent review | kicad-pro-review-agent (Gate 6) | no blocking findings | — |
| 13 | Manufacturing | `validate_for_manufacturing` | no warnings left open | — |
| 14 | BOM | `check_bom_health`, `estimate_cost` | every part in stock; cost within the brief | — |
| 15 | Release package | the release-package checklist | complete | — |

Report it as a table with the measured value beside each target, the overall verdict
(SHIP / FIX FIRST), and the three changes that would raise the lowest scores.
