use std::collections::HashSet;
use std::sync::Mutex;

use serde_json::Value;

pub const INSTRUCTIONS: &str = "Voltaris works stage by stage, and every stage follows the same loop:
1. Before the first tool call of a stage, load that stage's skill and follow it.
2. When the stage is complete, hand it to its review agent, which has a fresh context.
3. Fix every finding the professional way (the skill's method, not a workaround), then review again. Move on only when the review is clean.

Stage → skill → review agent:
- Any KiCad task → voltaris (operating rules) → —
- A new board, end to end → kicad-pro-workflow → each stage's agent below
- Requirements and parts → kicad-part-sourcing, kicad-power-design, kicad-mcu-design or kicad-esp32 → —
- Schematic → kicad-schematic → kicad-design-review-agent (schematic scope)
- Placement and routing → kicad-pro-layout, plus kicad-multilayer, kicad-high-speed-rf or kicad-power-electronics when they apply → kicad-pro-review-agent
- Silkscreen and product look → kicad-pro-silkscreen, kicad-board-craft → kicad-pro-review-agent
- Manufacturing files → kicad-manufacture → kicad-manufacture-agent
- Simulation → kicad-simulation → —

A whole schematic can be delegated to kicad-schematic-build-agent and a whole layout to kicad-layout-agent. In Claude the names carry the plugin prefix (voltaris:kicad-schematic, voltaris:kicad-pro-review-agent). A client without subagents runs the agent's checklist itself as a separate pass. Tool results add a short [voltaris] line when a stage starts or ends; act on it.";

struct Stage {
    name: &'static str,
    skill: &'static str,
}

const SCHEMATIC: Stage = Stage {
    name: "schematic",
    skill: "kicad-schematic",
};
const LAYOUT: Stage = Stage {
    name: "placement and routing",
    skill: "kicad-pro-layout",
};
const SILKSCREEN: Stage = Stage {
    name: "silkscreen",
    skill: "kicad-pro-silkscreen",
};
const PARTS: Stage = Stage {
    name: "part selection",
    skill: "kicad-part-sourcing",
};
const LIBRARY: Stage = Stage {
    name: "library",
    skill: "kicad-library",
};
const POWER: Stage = Stage {
    name: "power design",
    skill: "kicad-power-design",
};
const HIGH_SPEED: Stage = Stage {
    name: "high-speed and RF",
    skill: "kicad-high-speed-rf",
};
const SIMULATION: Stage = Stage {
    name: "simulation",
    skill: "kicad-simulation",
};
const MANUFACTURE: Stage = Stage {
    name: "manufacturing",
    skill: "kicad-manufacture",
};
const REVIEW: Stage = Stage {
    name: "review",
    skill: "kicad-review",
};
const WORKFLOW: Stage = Stage {
    name: "new design",
    skill: "kicad-pro-workflow",
};

fn stage(tool: &str, toolset: Option<&str>) -> Option<&'static Stage> {
    let by_tool = match tool {
        "create_project" => Some(&WORKFLOW),
        "polish_board" | "add_board_text" | "import_svg_logo" => Some(&SILKSCREEN),
        "calc_impedance" | "route_differential_pair" | "analyze_net_lengths" => Some(&HIGH_SPEED),
        "calc_buck" | "calc_regulator_thermal" | "calc_trace_width" | "audit_power_rails" => {
            Some(&POWER)
        }
        "search_jlcpcb_parts"
        | "get_jlcpcb_part"
        | "suggest_jlcpcb_alternatives"
        | "import_lcsc_part"
        | "check_bom_health" => Some(&PARTS),
        "smart_place"
        | "route_board"
        | "audit_layout"
        | "score_placement"
        | "set_design_rules"
        | "set_predefined_sizes"
        | "set_layer_constraints" => Some(&LAYOUT),
        "export_manufacturing_package"
        | "export_gerber"
        | "export_position_file"
        | "export_bom"
        | "validate_for_manufacturing"
        | "audit_manufacturing" => Some(&MANUFACTURE),
        _ => None,
    };
    by_tool.or(match toolset? {
        "sch_components" | "sch_wiring" | "sch_bus" | "sch_batch" | "sch_hierarchy" => {
            Some(&SCHEMATIC)
        }
        "pcb_board" | "pcb_components" | "pcb_routing" | "placement" | "smart_layout" => {
            Some(&LAYOUT)
        }
        "library" => Some(&LIBRARY),
        "lcsc_import" => Some(&PARTS),
        "signal_integrity" => Some(&HIGH_SPEED),
        "simulation" => Some(&SIMULATION),
        "manufacturing" => Some(&MANUFACTURE),
        "design_review" | "design_checks" | "verification" => Some(&REVIEW),
        _ => None,
    })
}

fn applied(args: &Value) -> bool {
    !args["dry_run"].as_bool().unwrap_or(true)
}

fn milestone(tool: &str, args: &Value) -> Option<&'static str> {
    match tool {
        "run_erc" => Some(
            "When ERC is clean and every block is drawn, the schematic stage is complete: run the kicad-design-review-agent on it (schematic scope: ERC, power rails, datasheet values, presentation), fix every finding, and review again before any layout work.",
        ),
        "update_pcb_from_schematic" => Some(
            "The board now follows the schematic. If the schematic changed since its last clean review by the kicad-design-review-agent, run that review first; then load kicad-pro-layout before placing.",
        ),
        "smart_place" if applied(args) => Some(
            "Placement applied: run score_placement and audit_layout, fix every overlap and lane finding, then route.",
        ),
        "route_board" if applied(args) => Some(
            "Routing applied: run audit_layout and run_drc. When copper and silkscreen are finished, run the kicad-pro-review-agent (Gate 6), fix every finding, and review again.",
        ),
        "polish_board" if applied(args) => Some(
            "Silkscreen applied: when the layout is finished, run the kicad-pro-review-agent (Gate 6), fix every finding, and review again.",
        ),
        "run_drc" => Some(
            "When DRC is clean and the layout is finished, run the kicad-pro-review-agent (Gate 6) before any manufacturing files; fix every finding and review again.",
        ),
        "export_manufacturing_package" => Some(
            "Package exported: run the kicad-manufacture-agent to check it against the board before ordering; fix every finding and export again.",
        ),
        _ => None,
    }
}

#[derive(Default)]
pub struct Guide {
    started: Mutex<HashSet<&'static str>>,
}

impl Guide {
    pub fn note(&self, tool: &str, toolset: Option<&str>, args: &Value) -> Option<String> {
        let mut lines = Vec::new();
        if let Some(stage) = stage(tool, toolset) {
            let fresh = self
                .started
                .lock()
                .map(|mut s| s.insert(stage.skill))
                .unwrap_or(false);
            if fresh {
                lines.push(format!(
                    "[voltaris] {} stage: load the {} skill now if it is not loaded, and follow it for every step of this stage.",
                    stage.name, stage.skill
                ));
            }
        }
        if let Some(next) = milestone(tool, args) {
            lines.push(format!(
                "[voltaris] {next} (A review agent running this check itself ignores this line.)"
            ));
        }
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

pub fn instructions(notice: Option<String>) -> String {
    match notice {
        Some(n) => format!("{n}\n\n{INSTRUCTIONS}"),
        None => INSTRUCTIONS.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn announces_a_stage_skill_once_per_session() {
        let guide = Guide::default();
        let first = guide.note("add_wire", Some("sch_wiring"), &json!({}));
        assert!(first.unwrap().contains("kicad-schematic"));
        assert!(guide
            .note("connect_pins", Some("sch_wiring"), &json!({}))
            .is_none());
    }

    #[test]
    fn ends_a_stage_with_its_review_agent() {
        let guide = Guide::default();
        assert!(guide
            .note("route_board", Some("smart_layout"), &json!({}))
            .is_some_and(|n| !n.contains("Gate 6")));
        let applied = guide
            .note(
                "route_board",
                Some("smart_layout"),
                &json!({"dry_run": false}),
            )
            .unwrap();
        assert!(applied.contains("kicad-pro-review-agent"));
        let fab = guide
            .note(
                "export_manufacturing_package",
                Some("pcb_export"),
                &json!({}),
            )
            .unwrap();
        assert!(fab.contains("kicad-manufacture-agent"));
    }

    #[test]
    fn instructions_keep_a_drift_notice_first() {
        let text = instructions(Some("run voltaris init".into()));
        assert!(text.starts_with("run voltaris init"));
        assert!(text.contains("kicad-pro-review-agent"));
    }
}
