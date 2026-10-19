//! MCP tool annotations (`title`, `readOnlyHint`, `destructiveHint`,
//! `openWorldHint`) for every tool, as the Claude directory requires. One
//! table decides each tool's class; a test fails when a tool is unclassified.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolAnnotations {
    pub read_only_hint: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub destructive_hint: Option<bool>,
    pub open_world_hint: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Class {
    ReadOnly,
    Additive,
    Destructive,
}

const READ_ONLY_PREFIXES: &[&str] = &[
    "get_",
    "list_",
    "find_",
    "search_",
    "calc_",
    "analyze_",
    "check_",
    "audit_",
    "validate_",
    "query_",
    "estimate_",
    "score_",
    "plan_",
    "resolve_",
    "compare_",
    "render_",
    "run_",
    "diff_",
    "simulate_",
    "batch_get_",
    "suggest_",
];

const ADDITIVE_PREFIXES: &[&str] = &[
    "add_",
    "create_",
    "place_component",
    "import_",
    "export_",
    "generate_",
    "snapshot_",
    "duplicate_",
    "register_",
    "connect_",
    "batch_add_",
    "batch_connect_",
    "route_",
    "copy_",
    "save_",
    "open_",
    "launch_",
    "download_",
    "load_toolset",
    "unload_toolset",
    "set_visual_baseline",
    "set_active_layer",
    "mutate_editor_selection",
];

const DESTRUCTIVE_PREFIXES: &[&str] = &[
    "delete_",
    "remove_",
    "edit_",
    "set_",
    "modify_",
    "move_",
    "rotate_",
    "flip_",
    "align_",
    "replace_",
    "rename_",
    "update_",
    "repair_",
    "refill_",
    "fix_",
    "annotate_",
    "renumber_",
    "reset_",
    "split_",
    "bulk_",
    "batch_delete",
    "batch_edit_",
    "batch_rotate_",
    "batch_place_",
    "apply_",
    "restore_",
    "auto_place_",
    "refine_",
    "place_decoupling_caps",
    "group_components",
    "assign_",
    "enrich_",
];

const EXACT: &[(&str, Class)] = &[
    ("trace_from_point", Class::ReadOnly),
    ("read_datasheet", Class::Additive),
    ("server_stats", Class::ReadOnly),
    ("list_toolboxes", Class::ReadOnly),
    ("load_project_config", Class::ReadOnly),
    ("load_user_config", Class::ReadOnly),
    ("reload_server", Class::Additive),
    ("place_component_array", Class::Additive),
    ("apply_template", Class::Additive),
    ("run_ngspice", Class::ReadOnly),
];

const OPEN_WORLD: &[&str] = &[
    "search_jlcpcb_parts",
    "get_jlcpcb_part",
    "suggest_jlcpcb_alternatives",
    "get_datasheet_url",
    "enrich_datasheets",
    "download_jlcpcb_database",
    "import_lcsc_part",
    "read_datasheet",
    "check_bom_health",
];

pub fn class_of(name: &str) -> Option<Class> {
    if let Some((_, c)) = EXACT.iter().find(|(n, _)| *n == name) {
        return Some(*c);
    }
    let longest = |list: &[&str]| {
        list.iter()
            .filter(|p| name.starts_with(**p))
            .map(|p| p.len())
            .max()
    };
    let candidates = [
        (longest(DESTRUCTIVE_PREFIXES), Class::Destructive),
        (longest(ADDITIVE_PREFIXES), Class::Additive),
        (longest(READ_ONLY_PREFIXES), Class::ReadOnly),
    ];
    candidates
        .into_iter()
        .filter_map(|(len, c)| len.map(|l| (l, c)))
        .max_by_key(|(l, _)| *l)
        .map(|(_, c)| c)
}

pub fn annotations_for(name: &str) -> ToolAnnotations {
    let class = class_of(name).unwrap_or(Class::Destructive);
    ToolAnnotations {
        read_only_hint: class == Class::ReadOnly,
        destructive_hint: (class != Class::ReadOnly).then_some(class == Class::Destructive),
        open_world_hint: OPEN_WORLD.contains(&name),
    }
}

pub fn title_for(name: &str) -> String {
    name.split('_')
        .filter(|w| !w.is_empty())
        .map(|w| match w {
            "pcb" | "drc" | "erc" | "bom" | "svg" | "pdf" | "dxf" | "odb" | "jlcpcb" | "lcsc"
            | "ui" | "dsn" | "ses" | "bga" | "png" | "mcp" | "ipc" | "3d" | "2d" | "spice" => {
                w.to_ascii_uppercase()
            }
            "ipc2581" => "IPC-2581".into(),
            "gencad" => "GenCAD".into(),
            "ngspice" => "ngspice".into(),
            "kicad" => "KiCad".into(),
            "calc" => "Calculate".into(),
            "sch" => "Schematic".into(),
            _ => {
                let mut c = w.chars();
                c.next()
                    .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_and_meta_tool_is_classified() {
        let mut missing = Vec::new();
        let mut names: Vec<String> = Vec::new();
        for ts in crate::router::registry::ALL_TOOLSETS {
            for t in crate::router::registry::tools_for(ts.name).unwrap() {
                names.push(t.name.to_string());
            }
        }
        for t in crate::router::meta_tools::meta_tool_descriptions_for(true) {
            names.push(t.name.clone());
        }
        for n in &names {
            if class_of(n).is_none() {
                missing.push(n.clone());
            }
        }
        assert!(missing.is_empty(), "unclassified tools: {missing:?}");
        assert!(names.len() >= 260);
    }

    #[test]
    fn classes_match_what_the_tools_do() {
        for n in [
            "get_board_info",
            "run_drc",
            "run_design_checks",
            "diff_design",
            "calc_impedance",
            "trace_from_point",
            "render_board_3d",
            "list_snapshots",
        ] {
            assert_eq!(class_of(n), Some(Class::ReadOnly), "{n}");
        }
        for n in [
            "add_wire",
            "export_gerber",
            "generate_firmware_kit",
            "snapshot_design",
            "place_component",
            "route_trace",
            "import_lcsc_part",
            "apply_template",
        ] {
            assert_eq!(class_of(n), Some(Class::Additive), "{n}");
        }
        for n in [
            "delete_component",
            "edit_schematic_component",
            "set_board_size",
            "move_component",
            "restore_snapshot",
            "apply_specctra_ses",
            "batch_delete_schematic_wire",
            "refill_zones",
            "remove_check_rule",
            "set_design_rules",
            "place_decoupling_caps",
            "enrich_datasheets",
        ] {
            assert_eq!(class_of(n), Some(Class::Destructive), "{n}");
        }
        let a = annotations_for("get_board_info");
        assert!(a.read_only_hint && a.destructive_hint.is_none() && !a.open_world_hint);
        let d = annotations_for("delete_trace");
        assert!(!d.read_only_hint && d.destructive_hint == Some(true));
        assert!(annotations_for("search_jlcpcb_parts").open_world_hint);
        let v = serde_json::to_value(annotations_for("add_via")).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"readOnlyHint": false, "destructiveHint": false, "openWorldHint": false})
        );
    }

    #[test]
    fn titles_read_like_names() {
        assert_eq!(title_for("run_drc"), "Run DRC");
        assert_eq!(title_for("export_ipc2581"), "Export IPC-2581");
        assert_eq!(title_for("search_jlcpcb_parts"), "Search JLCPCB Parts");
        assert_eq!(title_for("calc_trace_width"), "Calculate Trace Width");
        assert_eq!(title_for("render_board_3d"), "Render Board 3D");
    }
}
