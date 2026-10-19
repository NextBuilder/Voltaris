//! `enclosure` toolset — generate a parametric CadQuery enclosure from the
//! saved board: cavity from the Edge.Cuts outline, standoffs on the mounting
//! holes, and wall cut-outs for connectors and switches at the board edge.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, opt_f64, opt_str, ToolDef};
use serde_json::{json, Value};
use std::collections::HashMap;
use voltaris_sexp::board::{
    board_outline_bbox, board_outline_shape, footprint_courtyards, footprints, OutlineShape, Side,
};
use voltaris_sexp::parser::{parse_sexp, SexpNode};

#[derive(Debug, Clone)]
pub struct Hole {
    pub reference: String,
    pub x: f64,
    pub y: f64,
    pub drill: f64,
}

#[derive(Debug, Clone)]
pub struct EdgePart {
    pub reference: String,
    pub footprint: String,
    pub wall: &'static str,
    pub along_min: f64,
    pub along_max: f64,
    pub height: f64,
    pub bottom_side: bool,
}

/// Typical body heights above the PCB (mm) by footprint-name fragment.
/// Override per reference with `component_heights` — these are defaults, not data.
const HEIGHTS: &[(&str, f64)] = &[
    ("USB_C", 3.4),
    ("USB_Micro", 3.0),
    ("USB_A", 7.5),
    ("BarrelJack", 11.0),
    ("DSUB", 12.6),
    ("RJ45", 13.5),
    ("JST_PH_S", 4.0),
    ("JST_PH_B", 6.2),
    ("JST_XH", 7.0),
    ("JST_SH", 3.0),
    ("PinHeader", 8.6),
    ("PinSocket", 8.6),
    ("TerminalBlock", 10.0),
    ("SW_Push", 3.6),
    ("SW_SPST", 3.6),
    ("TS-1187A", 3.6),
    ("SW_Slide", 4.0),
    ("SD", 2.0),
    ("LED", 1.0),
];

fn default_height(footprint: &str) -> f64 {
    HEIGHTS
        .iter()
        .find(|(k, _)| {
            footprint
                .to_ascii_lowercase()
                .contains(&k.to_ascii_lowercase())
        })
        .map(|(_, h)| *h)
        .unwrap_or(6.0)
}

fn is_edge_candidate(reference: &str, footprint: &str) -> bool {
    let f = footprint.to_ascii_lowercase();
    reference.starts_with('J')
        || reference.starts_with("SW")
        || reference.starts_with("CN")
        || [
            "usb",
            "connector",
            "jack",
            "barrel",
            "sw_",
            "switch",
            "jst",
            "terminal",
            "header",
            "socket",
        ]
        .iter()
        .any(|k| f.contains(k))
}

fn reference_of(fp: &SexpNode) -> Option<String> {
    for p in fp.find_all("property") {
        if p.get(1).and_then(|n| n.as_str()) == Some("Reference") {
            return p.get(2).and_then(|n| n.as_str()).map(str::to_string);
        }
    }
    for t in fp.find_all("fp_text") {
        if t.get(1).and_then(|n| n.as_str()) == Some("reference") {
            return t.get(2).and_then(|n| n.as_str()).map(str::to_string);
        }
    }
    None
}

pub struct BoardModel {
    pub bbox: (f64, f64, f64, f64),
    pub rectangular: bool,
    pub thickness: f64,
    pub holes: Vec<Hole>,
    pub edge_parts: Vec<EdgePart>,
}

pub fn read_board(
    tree: &SexpNode,
    edge_margin: f64,
    heights: &HashMap<String, f64>,
) -> Result<BoardModel, String> {
    let bbox = board_outline_bbox(tree).ok_or("board has no readable Edge.Cuts outline")?;
    let rectangular = matches!(board_outline_shape(tree), OutlineShape::Rectangular { .. });
    let thickness = tree
        .find("general")
        .and_then(|g| g.find_f64("thickness"))
        .unwrap_or(1.6);
    let courtyards: HashMap<String, _> = footprint_courtyards(tree)
        .items
        .into_iter()
        .filter_map(|c| c.reference.clone().map(|r| (r, c)))
        .collect();
    let (x0, y0, x1, y1) = bbox;
    let mut holes = Vec::new();
    let mut edge_parts = Vec::new();
    for fp in footprints(tree) {
        let name = fp.get(1).and_then(|n| n.as_str()).unwrap_or("").to_string();
        let reference = reference_of(fp).unwrap_or_default();
        let at = fp.find("at");
        let (fx, fy) = (
            at.and_then(|a| a.get_f64(1)).unwrap_or(0.0),
            at.and_then(|a| a.get_f64(2)).unwrap_or(0.0),
        );
        let fp_name = name.rsplit(':').next().unwrap_or("");
        if name.starts_with("MountingHole:") || fp_name.starts_with("MountingHole") {
            let drill = fp
                .find_all("pad")
                .iter()
                .filter_map(|p| p.find("drill").and_then(|d| d.get_f64(1)))
                .fold(0.0_f64, f64::max);
            if drill > 0.0 {
                holes.push(Hole {
                    reference: reference.clone(),
                    x: fx,
                    y: fy,
                    drill,
                });
            }
            continue;
        }
        if !is_edge_candidate(&reference, &name) {
            continue;
        }
        let Some(c) = courtyards.get(&reference) else {
            continue;
        };
        let (bx0, by0, bx1, by1) = c.bbox;
        let gaps = [
            ("left", bx0 - x0),
            ("right", x1 - bx1),
            ("top", by0 - y0),
            ("bottom", y1 - by1),
        ];
        let (wall, gap) =
            gaps.iter()
                .copied()
                .fold(("", f64::INFINITY), |a, b| if b.1 < a.1 { b } else { a });
        if gap > edge_margin {
            continue;
        }
        let (along_min, along_max) = if wall == "left" || wall == "right" {
            (by0, by1)
        } else {
            (bx0, bx1)
        };
        let height = heights
            .get(&reference)
            .copied()
            .unwrap_or_else(|| default_height(&name));
        edge_parts.push(EdgePart {
            reference,
            footprint: name,
            wall: match wall {
                "left" => "left",
                "right" => "right",
                "top" => "top",
                _ => "bottom",
            },
            along_min,
            along_max,
            height,
            bottom_side: c.layer_side == Side::Back,
        });
    }
    Ok(BoardModel {
        bbox,
        rectangular,
        thickness,
        holes,
        edge_parts,
    })
}

pub struct Params {
    pub wall: f64,
    pub floor: f64,
    pub clearance: f64,
    pub standoff: f64,
    pub top_space: f64,
    pub lid: f64,
    pub cut_margin: f64,
}

fn pilot_for(drill: f64) -> (f64, &'static str) {
    if drill >= 4.0 {
        (3.3, "M4")
    } else if drill >= 3.0 {
        (2.5, "M3")
    } else if drill >= 2.5 {
        (2.0, "M2.5")
    } else {
        (1.6, "M2")
    }
}

pub fn render_cadquery(m: &BoardModel, p: &Params, name: &str) -> String {
    let (x0, y0, x1, y1) = m.bbox;
    let (bw, bh) = (x1 - x0, y1 - y0);
    let mut s = format!(
        "# {name} enclosure — generated by Voltaris from the saved .kicad_pcb\n\
         # Run:  pip install cadquery  &&  python {name}_enclosure.py\n\
         # Writes {name}_base.step/.stl and {name}_lid.step/.stl next to this script.\n\
         # Coordinates: board viewed from the top, origin at the base's outer corner, mm.\n\n\
         import cadquery as cq\n\n\
         BOARD_W, BOARD_H, PCB_T = {bw:.3}, {bh:.3}, {t:.3}\n\
         WALL, FLOOR, CLEAR = {w}, {f}, {c}\n\
         STANDOFF, TOP_SPACE, LID_T = {so}, {ts}, {lid}\n\
         CUT_MARGIN, LIP_W, LIP_H, FIT = {cm}, 1.2, 2.0, 0.2\n\n\
         IN_W, IN_H = BOARD_W + 2 * CLEAR, BOARD_H + 2 * CLEAR\n\
         OUT_W, OUT_H = IN_W + 2 * WALL, IN_H + 2 * WALL\n\
         PCB_Z = FLOOR + STANDOFF\n\
         BASE_H = PCB_Z + PCB_T + TOP_SPACE\n\n\
         def bx(x):\n    return WALL + CLEAR + (x - {x0:.4})\n\n\
         def by(y):\n    return WALL + CLEAR + ({y1:.4} - y)\n\n\
         base = cq.Workplane(\"XY\").box(OUT_W, OUT_H, BASE_H, centered=False)\n\
         base = base.cut(cq.Workplane(\"XY\").box(IN_W, IN_H, BASE_H, centered=False).translate((WALL, WALL, FLOOR)))\n",
        t = m.thickness, w = p.wall, f = p.floor, c = p.clearance, so = p.standoff,
        ts = p.top_space, lid = p.lid, cm = p.cut_margin,
        x0 = x0, y1 = y1,
    );
    if !m.holes.is_empty() {
        s.push_str(
            "\n# Standoffs on the board's mounting holes (pilot holes for self-tapping screws)\n",
        );
        s.push_str("HOLES = [\n");
        for h in &m.holes {
            let (pilot, screw) = pilot_for(h.drill);
            s.push_str(&format!(
                "    ({:.4}, {:.4}, {:.2}, {pilot}),  # {} drill {:.2} → {screw}\n",
                h.x,
                h.y,
                (h.drill + 3.0).max(5.0),
                h.reference,
                h.drill
            ));
        }
        s.push_str(
            "]\nfor x, y, od, pilot in HOLES:\n\
             \x20   post = cq.Workplane(\"XY\").circle(od / 2).circle(pilot / 2).extrude(STANDOFF).translate((bx(x), by(y), FLOOR))\n\
             \x20   base = base.union(post)\n",
        );
    }
    if !m.edge_parts.is_empty() {
        s.push_str("\n# Wall cut-outs for edge connectors and switches: (ref, wall, along_min, along_max, height, bottom_side)\n");
        s.push_str("CUTS = [\n");
        for e in &m.edge_parts {
            s.push_str(&format!(
                "    (\"{}\", \"{}\", {:.4}, {:.4}, {:.2}, {}),  # {}\n",
                e.reference,
                e.wall,
                e.along_min,
                e.along_max,
                e.height,
                if e.bottom_side { "True" } else { "False" },
                e.footprint
            ));
        }
        s.push_str(
            "]\nfor ref, wall, a0, a1, height, bottom in CUTS:\n\
             \x20   z0 = (PCB_Z - height if bottom else PCB_Z + PCB_T) - CUT_MARGIN\n\
             \x20   zh = height + 2 * CUT_MARGIN\n\
             \x20   if wall in (\"left\", \"right\"):\n\
             \x20       y_lo, y_hi = sorted((by(a0), by(a1)))\n\
             \x20       x = 0 if wall == \"left\" else OUT_W - WALL\n\
             \x20       cut = cq.Workplane(\"XY\").box(WALL, (y_hi - y_lo) + 2 * CUT_MARGIN, zh, centered=False).translate((x, y_lo - CUT_MARGIN, z0))\n\
             \x20   else:\n\
             \x20       x_lo, x_hi = bx(a0), bx(a1)\n\
             \x20       y = OUT_H - WALL if wall == \"top\" else 0\n\
             \x20       cut = cq.Workplane(\"XY\").box((x_hi - x_lo) + 2 * CUT_MARGIN, WALL, zh, centered=False).translate((x_lo - CUT_MARGIN, y, z0))\n\
             \x20   base = base.cut(cut)\n",
        );
    }
    s.push_str(
        "\n# Lid: plate with a locating lip that drops inside the walls\n\
         lid = cq.Workplane(\"XY\").box(OUT_W, OUT_H, LID_T, centered=False)\n\
         lip_outer = cq.Workplane(\"XY\").box(IN_W - 2 * FIT, IN_H - 2 * FIT, LIP_H, centered=False).translate((WALL + FIT, WALL + FIT, -LIP_H))\n\
         lip_inner = cq.Workplane(\"XY\").box(IN_W - 2 * FIT - 2 * LIP_W, IN_H - 2 * FIT - 2 * LIP_W, LIP_H, centered=False).translate((WALL + FIT + LIP_W, WALL + FIT + LIP_W, -LIP_H))\n\
         lid = lid.union(lip_outer.cut(lip_inner))\n\n",
    );
    s.push_str(&format!(
        "for part, label in ((base, \"base\"), (lid, \"lid\")):\n\
         \x20   cq.exporters.export(part, f\"{name}_{{label}}.step\")\n\
         \x20   cq.exporters.export(part, f\"{name}_{{label}}.stl\")\n\
         print(f\"Base {{OUT_W:.1f}} x {{OUT_H:.1f}} x {{BASE_H:.1f}} mm, lid {{LID_T}} mm — exported STEP and STL\")\n"
    ));
    s
}

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "generate_enclosure",
        "Generate a parametric two-part 3D-printable enclosure (base + lid) as a CadQuery Python \
         script from the saved .kicad_pcb: the cavity follows the Edge.Cuts bounding box, a standoff \
         with a screw pilot hole sits under every MountingHole footprint, and connectors/switches \
         whose courtyards reach the board edge get wall cut-outs at the right height. Component \
         heights default to typical values per footprint family — pass component_heights for exact \
         parts. Running the script exports STEP (for CAD) and STL (for printing). Writes the script \
         when output_dir is given.",
        json!({
            "type": "object",
            "properties": {
                "board": { "type": "string", "description": "Path to the saved .kicad_pcb" },
                "output_dir": { "type": "string", "description": "Where to write <name>_enclosure.py" },
                "wall_mm": { "type": "number", "exclusiveMinimum": 0, "default": 2.0 },
                "floor_mm": { "type": "number", "exclusiveMinimum": 0, "default": 2.0 },
                "clearance_mm": { "type": "number", "minimum": 0, "default": 0.5, "description": "Gap between board edge and inner wall" },
                "standoff_mm": { "type": "number", "exclusiveMinimum": 0, "default": 5.0, "description": "Floor-to-PCB height (room for THT leads and bottom-side parts)" },
                "top_space_mm": { "type": "number", "exclusiveMinimum": 0, "default": 12.0, "description": "PCB-top-to-lid height: tallest component plus margin" },
                "lid_mm": { "type": "number", "exclusiveMinimum": 0, "default": 2.0 },
                "cut_margin_mm": { "type": "number", "minimum": 0, "default": 0.6, "description": "Extra opening around each connector" },
                "edge_margin_mm": { "type": "number", "minimum": 0, "default": 1.5, "description": "A connector counts as an edge part when its courtyard is within this distance of the outline" },
                "component_heights": { "type": "object", "additionalProperties": { "type": "number", "exclusiveMinimum": 0 }, "description": "Exact body heights by reference, e.g. {\"J1\": 3.26}" }
            },
            "required": ["board"]
        }),
        |args, _ctx| async move { handle_enclosure(args) }
    )]
}

fn handle_enclosure(args: &Value) -> anyhow::Result<CallToolResult> {
    let path = get_path(args, "board")?;
    let text = std::fs::read_to_string(&path)?;
    let tree = parse_sexp(&text).map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))?;
    let heights: HashMap<String, f64> = args["component_heights"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_f64().map(|h| (k.clone(), h)))
                .collect()
        })
        .unwrap_or_default();
    let model = match read_board(
        &tree,
        opt_f64(args, "edge_margin_mm").unwrap_or(1.5),
        &heights,
    ) {
        Ok(m) => m,
        Err(e) => return Ok(CallToolResult::error(format!("{}: {e}", path.display()))),
    };
    let params = Params {
        wall: opt_f64(args, "wall_mm").unwrap_or(2.0),
        floor: opt_f64(args, "floor_mm").unwrap_or(2.0),
        clearance: opt_f64(args, "clearance_mm").unwrap_or(0.5),
        standoff: opt_f64(args, "standoff_mm").unwrap_or(5.0),
        top_space: opt_f64(args, "top_space_mm").unwrap_or(12.0),
        lid: opt_f64(args, "lid_mm").unwrap_or(2.0),
        cut_margin: opt_f64(args, "cut_margin_mm").unwrap_or(0.6),
    };
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "board".into());
    let script = render_cadquery(&model, &params, &name);
    let mut written = None;
    if let Some(dir) = opt_str(args, "output_dir") {
        let target = std::path::Path::new(dir).join(format!("{name}_enclosure.py"));
        std::fs::create_dir_all(dir)?;
        std::fs::write(&target, script.as_bytes())?;
        written = Some(target.display().to_string());
    }
    let (x0, y0, x1, y1) = model.bbox;
    let mut warnings = Vec::new();
    if !model.rectangular {
        warnings.push("Outline is not an axis-aligned rectangle: the cavity uses its bounding box, so add internal ribs or edit the script for a form-fitting shell".to_string());
    }
    if model.holes.is_empty() {
        warnings.push("No MountingHole footprints: the board will sit loose — add mounting holes or snap-fit ribs".to_string());
    }
    if model
        .edge_parts
        .iter()
        .any(|e| !heights.contains_key(&e.reference))
    {
        warnings.push("Some cut-out heights are typical values for the footprint family; confirm against the part datasheets (component_heights)".to_string());
    }
    let outer = (
        x1 - x0 + 2.0 * (params.clearance + params.wall),
        y1 - y0 + 2.0 * (params.clearance + params.wall),
    );
    Ok(CallToolResult::json(&json!({
        "board_mm": [((x1 - x0) * 1000.0).round() / 1000.0, ((y1 - y0) * 1000.0).round() / 1000.0, model.thickness],
        "enclosure_outer_mm": [(outer.0 * 100.0).round() / 100.0, (outer.1 * 100.0).round() / 100.0,
            ((params.floor + params.standoff + model.thickness + params.top_space) * 100.0).round() / 100.0],
        "standoffs": model.holes.iter().map(|h| json!({ "reference": h.reference, "x": h.x, "y": h.y, "drill": h.drill, "screw": pilot_for(h.drill).1 })).collect::<Vec<_>>(),
        "cutouts": model.edge_parts.iter().map(|e| json!({ "reference": e.reference, "wall": e.wall, "height_mm": e.height, "height_source": if heights.contains_key(&e.reference) { "caller" } else { "typical" }, "footprint": e.footprint })).collect::<Vec<_>>(),
        "warnings": warnings,
        "script_path": written,
        "script": script,
        "next_steps": "pip install cadquery, run the script, and open the STEP files in your CAD tool to check fit before printing."
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew")
      (general (thickness 1.6))
      (gr_rect (start 100 50) (end 150 80) (layer "Edge.Cuts") (width 0.1))
      (footprint "MountingHole:MountingHole_3.2mm_M3" (layer "F.Cu") (at 104 54)
        (property "Reference" "H1" (at 0 0 0) (layer "F.SilkS"))
        (pad "" np_thru_hole circle (at 0 0) (size 3.2 3.2) (drill 3.2) (layers "*.Cu" "*.Mask")))
      (footprint "Connector_USB:USB_C_Receptacle_HRO_TYPE-C-31-M-12" (layer "F.Cu") (at 102 65 90)
        (property "Reference" "J1" (at 0 0 0) (layer "F.SilkS"))
        (fp_rect (start -4.5 -2) (end 4.5 5) (layer "F.CrtYd") (width 0.05)))
      (footprint "Connector_Dsub:DSUB-9_Female_Horizontal_P2.77x2.84mm_EdgePinOffset7.70mm_Housed_MountingHolesOffset9.12mm" (layer "F.Cu") (at 125 76)
        (property "Reference" "J2" (at 0 0 0) (layer "F.SilkS"))
        (pad "0" thru_hole circle (at -12.5 0) (size 4 4) (drill 3.2) (layers "*.Cu" "*.Mask"))
        (fp_rect (start -16 -6) (end 16 4.5) (layer "F.CrtYd") (width 0.05)))
      (footprint "Resistor_SMD:R_0402_1005Metric" (layer "F.Cu") (at 125 65)
        (property "Reference" "R1" (at 0 0 0) (layer "F.SilkS"))
        (fp_rect (start -1 -0.5) (end 1 0.5) (layer "F.CrtYd") (width 0.05)))
    )"#;

    #[test]
    fn board_features_are_extracted() {
        let tree = parse_sexp(BOARD).unwrap();
        let m = read_board(&tree, 1.5, &HashMap::new()).unwrap();
        assert_eq!(m.bbox, (100.0, 50.0, 150.0, 80.0));
        assert_eq!(m.thickness, 1.6);
        assert_eq!(m.holes.len(), 1);
        assert_eq!(
            (m.holes[0].x, m.holes[0].y, m.holes[0].drill),
            (104.0, 54.0, 3.2)
        );
        assert_eq!(
            m.edge_parts.len(),
            2,
            "R1 is not an edge part; the D-sub's own holes are not mounting holes"
        );
        assert_eq!(m.edge_parts[0].reference, "J1");
        assert_eq!(m.edge_parts[1].wall, "bottom");
        assert_eq!(m.edge_parts[0].wall, "left");
        assert_eq!(m.edge_parts[0].height, 3.4);
        let script = render_cadquery(
            &m,
            &Params {
                wall: 2.0,
                floor: 2.0,
                clearance: 0.5,
                standoff: 5.0,
                top_space: 12.0,
                lid: 2.0,
                cut_margin: 0.6,
            },
            "demo",
        );
        assert!(
            script.contains("HOLES = [")
                && script.contains("CUTS = [")
                && script.contains("\"J1\", \"left\"")
        );
    }
}
