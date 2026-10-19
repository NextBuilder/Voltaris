//! `visual_review` toolset — render the board in 3D from several angles so the
//! model can inspect it the way a human reviewer would, with a checklist of
//! what DRC cannot see.

use crate::mcp::protocol::{CallToolResult, ToolContent};
use crate::tool;
use crate::tools::cli::{render_pcb_view, render_view};
use crate::tools::{get_path, invalid_arg, opt_f64, opt_str, opt_str_list, ToolContext, ToolDef};
use serde_json::{json, Value};

pub const REVIEW_CHECKLIST: &str = "Visual review — inspect each image for what DRC cannot catch:\n\
1. Connectors, buttons and switches face outward and sit flush with the board edge they serve.\n\
2. RF module antennas overhang the edge or sit over a copper-free keep-out on every layer.\n\
3. Decoupling capacitors sit beside the pins they serve, not across the board.\n\
4. Hot parts (regulators, chargers, drivers) have copper around them and are not packed next to sensors or the battery.\n\
5. Silkscreen: reference designators readable, not on pads or under parts; pin-1, polarity and LED/diode marks visible.\n\
6. Tall parts do not block connectors, buttons, displays or each other; mounting holes have clearance for screw heads.\n\
7. Every footprint shows its 3D body — a missing body usually means a wrong or unassigned footprint/model.\n\
8. Board outline, cut-outs and mounting holes match the enclosure plan.\n\
Report each finding with the reference designator and the fix, then fix and re-render.";

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "render_board_3d",
        "Render the saved board in 3D with kicad-cli from one or more views (top, bottom, iso, \
         iso_bottom, front, back, left, right) and return the images together with a visual-review \
         checklist covering what DRC cannot detect: connector orientation, antenna keep-outs, \
         decoupling proximity, thermal placement, silkscreen legibility, missing 3D bodies. \
         Use it after placement and again after routing. Images enter the model's context, so \
         keep sizes modest and ask for 'high' quality only for final checks.",
        json!({
            "type": "object",
            "properties": {
                "board": { "type": "string", "description": "Path to the saved .kicad_pcb (save the board first — kicad-cli reads the file)" },
                "views": { "type": "array", "items": { "type": "string", "enum": ["top", "bottom", "iso", "iso_bottom", "front", "back", "left", "right"] }, "default": ["iso", "top", "bottom"], "maxItems": 6 },
                "width": { "type": "integer", "minimum": 200, "maximum": 2400, "default": 900 },
                "height": { "type": "integer", "minimum": 200, "maximum": 2400, "default": 600 },
                "quality": { "type": "string", "enum": ["basic", "high"], "default": "basic", "description": "'high' raytraces: much slower" },
                "zoom": { "type": "number", "exclusiveMinimum": 0, "default": 1.0 },
                "floor": { "type": "boolean", "default": false, "description": "Add floor, shadows and post-processing" }
            },
            "required": ["board"]
        }),
        |args, ctx| async move { handle_render(args, ctx).await }
    )]
}

async fn handle_render(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    use base64::Engine;
    let board = get_path(args, "board")?;
    if !board.is_file() {
        return Ok(invalid_arg(
            "board",
            &format!("{} does not exist", board.display()),
        ));
    }
    let views = match opt_str_list(args, "views") {
        Ok(v) => v.unwrap_or_else(|| vec!["iso".into(), "top".into(), "bottom".into()]),
        Err(e) => return Ok(e),
    };
    let width = args["width"].as_u64().unwrap_or(900).clamp(200, 2400) as u32;
    let height = args["height"].as_u64().unwrap_or(600).clamp(200, 2400) as u32;
    let quality = opt_str(args, "quality").unwrap_or("basic");
    let zoom = opt_f64(args, "zoom").unwrap_or(1.0);
    let floor = args["floor"].as_bool().unwrap_or(false);
    let mut content = Vec::new();
    let mut rendered = Vec::new();
    let mut failed = Vec::new();
    let tmpdir = std::env::temp_dir().join(format!("kcp-render-{}", std::process::id()));
    std::fs::create_dir_all(&tmpdir)?;
    for name in &views {
        let Some(view) = render_view(name) else {
            return Ok(invalid_arg("views", &format!("unknown view '{name}'")));
        };
        let out = tmpdir.join(format!("{name}.png"));
        match render_pcb_view(
            &ctx.config.kicad_cli,
            &board,
            &out,
            &view,
            width,
            height,
            quality,
            zoom,
            floor,
        )
        .await
        {
            Ok(()) => {
                let bytes = std::fs::read(&out)?;
                let _ = std::fs::remove_file(&out);
                content.push(ToolContent::Text {
                    text: format!("View: {name}"),
                });
                content.push(ToolContent::Image {
                    data: base64::engine::general_purpose::STANDARD.encode(&bytes),
                    mime_type: "image/png".into(),
                });
                rendered.push(name.clone());
            }
            Err(e) => failed.push(json!({ "view": name, "error": e.to_string() })),
        }
    }
    let _ = std::fs::remove_dir(&tmpdir);
    if rendered.is_empty() {
        return Ok(CallToolResult::error(format!(
            "No view rendered. kicad-cli errors: {}",
            serde_json::to_string(&failed).unwrap_or_default()
        )));
    }
    content.push(ToolContent::Text {
        text: serde_json::to_string(&json!({
            "rendered": rendered,
            "failed": failed,
            "complete": failed.is_empty(),
            "checklist": REVIEW_CHECKLIST,
        }))
        .unwrap_or_default(),
    });
    Ok(CallToolResult {
        content,
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use crate::tools::cli::{render_view, render_view_args};

    #[test]
    fn iso_view_uses_verified_kicad_cli_flags() {
        let v = render_view("iso").unwrap();
        let a = render_view_args(&v, "b.kicad_pcb", "o.png", 900, 600, "basic", 1.0, false);
        let joined = a.join(" ");
        assert!(joined.starts_with("pcb render --output o.png --width 900 --height 600 --side top --quality basic --background opaque --zoom 1"));
        assert!(joined.contains("--rotate -45,0,45 --perspective"));
        assert_eq!(a.last().unwrap(), "b.kicad_pcb");
        assert!(render_view("sideways").is_none());
        let b = render_view_args(
            &render_view("bottom").unwrap(),
            "b",
            "o",
            300,
            200,
            "high",
            0.8,
            true,
        );
        assert!(b.contains(&"--floor".to_string()) && !b.contains(&"--rotate".to_string()));
    }
}
