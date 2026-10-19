//! Static registry mapping toolset names → ToolDef slices.
//!
//! Each toolset module exposes a `tools()` function returning its Vec<ToolDef>.
//! This registry wires them together by name.

use super::ToolsetMeta;
use crate::tools::ToolDef;
use std::collections::HashMap;
use std::sync::OnceLock;

/// Toolsets auto-loaded when the server starts.
///
/// Kept minimal so that baseline `tools/list` context stays small (20 tools
/// including meta-tools ≈ 2K tokens). The LLM expands its toolbelt on demand
/// via `load_toolset(...)`.
///
/// Starter choices:
/// - `project` — needed to open / create / save any project
/// - `config` — user preferences, design rules; call `load_user_config` at session start
pub static STARTER_KIT: &[&str] = &["project", "config"];

pub static ALL_TOOLSETS: &[ToolsetMeta] = &[
    ToolsetMeta {
        name: "project",
        description: "Create, open, save, rename, snapshot KiCad projects, and launch the live schematic viewer",
        category: "project",
        tool_count: 7,
    },
    ToolsetMeta {
        name: "editor_navigation",
        description: "Observe and semantically navigate exact KiCad editor, document, sheet, selection, and cross-probe context",
        category: "project",
        tool_count: 5,
    },
    ToolsetMeta {
        name: "sch_components",
        description: "Add, edit, move, rotate, and delete schematic symbols",
        category: "schematic",
        tool_count: 18,
    },
    ToolsetMeta {
        name: "sch_wiring",
        description: "Wires, net labels, power symbols, junctions, no-connects, pin-to-pin connections",
        category: "schematic",
        tool_count: 20,
    },
    ToolsetMeta {
        name: "sch_bus",
        description: "Buses, bus entries, and fanning a group of pins out onto a bus",
        category: "schematic",
        tool_count: 4,
    },
    ToolsetMeta {
        name: "sch_analysis",
        description: "Net connectivity, pin queries, trace paths, overlap/orphan detection",
        category: "schematic",
        tool_count: 15,
    },
    ToolsetMeta {
        name: "sch_batch",
        description: "Bulk add, edit, delete, and move schematic elements in one call",
        category: "schematic",
        tool_count: 12,
    },
    ToolsetMeta {
        name: "sch_export",
        description: "Export schematic to SVG/PDF/PNG/netlist, run ERC, and synchronize a live PCB",
        category: "schematic",
        tool_count: 10,
    },
    ToolsetMeta {
        name: "sch_hierarchy",
        description: "Sheet files and hierarchical sheets: create a schematic file and set its page size, add/edit/move/delete/duplicate a sheet, hierarchy and page-numbering queries, import/add/edit/delete sheet pins, pin/label sync validation",
        category: "schematic",
        tool_count: 14,
    },
    ToolsetMeta {
        name: "pcb_board",
        description: "Board outline, layers, zones, mounting holes, board text, SVG logo import",
        category: "pcb",
        tool_count: 13,
    },
    ToolsetMeta {
        name: "pcb_components",
        description: "Place, refresh, move, rotate, flip, align, duplicate and repair PCB footprints; inspect pads; inspect and edit a placed footprint's graphics",
        category: "pcb",
        tool_count: 20,
    },
    ToolsetMeta {
        name: "pcb_routing",
        description: "Traces, vias, copper pours, net classes, differential pairs, and strict Specctra SES import",
        category: "pcb",
        tool_count: 15,
    },
    ToolsetMeta {
        name: "placement",
        description: "Placement quality and automation: score with named deductions, plan decoupling rows and BGA fanouts, first placement, force-directed refinement",
        category: "pcb",
        tool_count: 5,
    },
    ToolsetMeta {
        name: "pcb_export",
        description: "Gerber, PDF, SVG, 3D model, BOM, Specctra DSN, pick-and-place, DRC, DXF/GenCAD/IPC-2581/ODB++",
        category: "pcb",
        tool_count: 14,
    },
    ToolsetMeta {
        name: "library",
        description: "Search, register, and author symbol and footprint libraries — create symbols and footprints, edit pads, graphics, metadata and 3D models",
        category: "library",
        tool_count: 17,
    },
    ToolsetMeta {
        name: "integration",
        description: "JLCPCB parts database, local Freerouting MCP routing, datasheet URLs",
        category: "integration",
        tool_count: 9,
    },
    ToolsetMeta {
        name: "lcsc_import",
        description: "Import any LCSC/JLCPCB part (symbol, footprint, 3D model) into a project library via easyeda2kicad and register it",
        category: "library",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "verification",
        description: "DRC, design rules, layer constraints, clearance checks, KiCad UI control (ERC is in sch_export)",
        category: "verification",
        tool_count: 10,
    },
    ToolsetMeta {
        name: "config",
        description: "User preferences, project rules, design rules, fab constraints — call load_user_config at session start",
        category: "config",
        tool_count: 7,
    },
    ToolsetMeta {
        name: "design_review",
        description: "AI-powered design audits: decoupling, connections, power rails, DFM, BOM health",
        category: "review",
        tool_count: 6,
    },
    ToolsetMeta {
        name: "templates",
        description: "Reference circuit library verified against KiCad 10: ESP32-S3/C3/C6 cores, USB-C device port, CH340C auto-program, Li-ion charge/protect/power-path, AP2112K LDO, WS2812B, SHT30, plus USB-C sink, LDO, buck, STM32, I2C, LED — with footprints and exact wiring plans",
        category: "templates",
        tool_count: 4,
    },
    ToolsetMeta {
        name: "engineering",
        description: "Engineering calculators: IPC-2221 trace width, impedance (microstrip, stripline, CPWG, diff pairs) with JLCPCB stackup presets, vias, LDO thermals, E-series dividers, LED/RC/crystal/I2C, buck sizing",
        category: "engineering",
        tool_count: 8,
    },
    ToolsetMeta {
        name: "enclosure",
        description: "Generate a 3D-printable base + lid enclosure (CadQuery script → STEP/STL) from the board outline, mounting holes and edge connectors",
        category: "mechanical",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "firmware",
        description: "Generate board_pins.h and an Arduino bring-up sketch from the schematic, and audit ESP32 pin choices (strapping, ADC2, input-only, USB, PSRAM/flash pins)",
        category: "firmware",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "signal_integrity",
        description: "Routed length, via count and delay per net; differential-pair and length-group skew checks for USB, Ethernet, clocks and buses",
        category: "pcb",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "simulation",
        description: "Simulate circuits: built-in linear SPICE solver (.op/.ac/.tran for R, C, L, sources, E/G) and an ngspice batch runner for nonlinear models",
        category: "simulation",
        tool_count: 2,
    },
    ToolsetMeta {
        name: "design_checks",
        description: "Turn plain-English design rules into stored automatic checks and run them, plus a built-in professional rule pack (pull-ups, ESD, decoupling, footprints, LCSC numbers)",
        category: "review",
        tool_count: 4,
    },
    ToolsetMeta {
        name: "design_history",
        description: "Snapshot the project, diff any two states (parts, connections, placement, routed lengths) and restore a snapshot",
        category: "project",
        tool_count: 4,
    },
    ToolsetMeta {
        name: "datasheet",
        description: "Read PDF datasheets by path, URL, MPN or LCSC number: page-cited pin tables, absolute maximum ratings, operating conditions, application circuits and layout guidance",
        category: "integration",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "block_diagram",
        description: "Draw the system block diagram from the netlist as Mermaid: blocks, buses (USB, I2C, SPI, UART, SWD, I2S, CAN, analog, GPIO) and the power tree",
        category: "review",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "visual_review",
        description: "Render the board in 3D from several angles (top, bottom, iso, sides) with a review checklist for what DRC cannot see",
        category: "review",
        tool_count: 1,
    },
    ToolsetMeta {
        name: "manufacturing",
        description: "Design-to-fab pipeline: export Gerber+BOM+positions package, validate for fab house, estimate cost",
        category: "manufacturing",
        tool_count: 3,
    },
];

static TOOL_CATALOGUE: OnceLock<HashMap<&'static str, Vec<ToolDef>>> = OnceLock::new();

fn tool_catalogue() -> &'static HashMap<&'static str, Vec<ToolDef>> {
    TOOL_CATALOGUE.get_or_init(|| {
        ALL_TOOLSETS
            .iter()
            .map(|toolset| {
                let tools = build_tools_for(toolset.name)
                    .unwrap_or_else(|| panic!("no tool builder for '{}'", toolset.name));
                (toolset.name, tools)
            })
            .collect()
    })
}

/// Return cached ToolDefs for a given toolset name, or None if unknown.
/// Clones share each definition's compiled input validator through `Arc`.
pub fn tools_for(name: &str) -> Option<Vec<ToolDef>> {
    tool_catalogue().get(name).cloned()
}

fn build_tools_for(name: &str) -> Option<Vec<ToolDef>> {
    use crate::tools::*;
    match name {
        "project" => Some(project::tools()),
        "editor_navigation" => Some(editor_navigation::tools()),
        "sch_components" => Some(sch_components::tools()),
        "sch_wiring" => Some(sch_wiring::tools()),
        "sch_bus" => Some(sch_bus::tools()),
        "sch_analysis" => Some(sch_analysis::tools()),
        "sch_batch" => Some(sch_batch::tools()),
        "sch_export" => Some(sch_export::tools()),
        "sch_hierarchy" => Some(sch_hierarchy::tools()),
        "pcb_board" => Some(pcb_board::tools()),
        "pcb_components" => Some(pcb_components::tools()),
        "pcb_routing" => Some(pcb_routing::tools()),
        "placement" => Some(placement::tools()),
        "pcb_export" => Some(pcb_export::tools()),
        "library" => Some(library::tools()),
        "integration" => Some(integration::tools()),
        "lcsc_import" => Some(lcsc_import::tools()),
        "verification" => Some(verification::tools()),
        "config" => Some(config::tools()),
        "design_review" => Some(design_review::tools()),
        "templates" => Some(templates::tools()),
        "engineering" => Some(engineering::tools()),
        "enclosure" => Some(enclosure::tools()),
        "firmware" => Some(firmware::tools()),
        "signal_integrity" => Some(signal_integrity::tools()),
        "simulation" => Some(simulation::tools()),
        "design_checks" => Some(design_checks::tools()),
        "design_history" => Some(design_history::tools()),
        "datasheet" => Some(datasheet::tools()),
        "block_diagram" => Some(block_diagram::tools()),
        "visual_review" => Some(visual_review::tools()),
        "manufacturing" => Some(manufacturing::tools()),
        _ => None,
    }
}
