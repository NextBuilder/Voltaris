//! `smart_layout` toolset — one-call layout engines that work on the board open
//! in KiCad: `route_board` (grid A* autorouter), `smart_place` (placement
//! planner), `polish_board` (labels, pours, stitching, name block) and
//! `audit_layout` (engineering and beauty review). Every mutating tool plans
//! first; applying needs the plan's revision, so the board cannot change
//! between the plan the user saw and the edit KiCad receives.

pub(crate) mod audit;
pub(crate) mod model;
pub(crate) mod placer;
pub(crate) mod polish;
pub(crate) mod refine;
pub(crate) mod router;

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::live_board::{self, LiveBoard};
use crate::tools::{get_path, invalid_arg, opt_str, ToolContext, ToolDef};
use model::{round3, Board};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use voltaris_ipc::builders;

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "route_board",
            "Route every unrouted connection on the board in one call: a grid A* router that \
             orders nets by priority (pours, power, fast signals, signals, ground), uses every \
             copper layer you allow with vias, keeps clearance from foreign copper and the edge, \
             prefers 45°/90° paths with few bends and a preferred layer, straightens each path, \
             and retries the nets that failed first on the next pass. Differential pairs (D+/D-, \
             USB_DP/DM, _P/_N, or 'pairs') are routed first as coupled pairs at pair_gap and \
             length-tuned with 45° bumps until the skew is within pair_skew. Fine-pitch parts \
             (pins closer than 0.8 mm) get an automatic pitch-aligned grid and a neck-down width at \
             their pads, so no manual grid is needed. Power nets get power_width, signals \
             signal_width, any net its net_widths entry. Nets fully covered by pours on every \
             routing layer, and nets already connected, are skipped. Defaults to a dry run that \
             reports completion, vias, length, pairs (coupling, skew) and a plan_revision; apply \
             with dry_run false and that revision. Applies live in KiCad as one undo step.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string", "description": "Path to the .kicad_pcb open in KiCad" },
                    "dry_run": { "type": "boolean", "default": true },
                    "expected_plan_revision": { "type": "string", "description": "Required to apply: plan_revision from the dry run" },
                    "nets": { "type": "array", "items": { "type": "string" }, "description": "Route only these nets" },
                    "layers": { "type": "array", "items": { "type": "string" }, "description": "Copper layers to route on (default F.Cu, B.Cu)" },
                    "grid": { "type": "number", "description": "Routing grid in mm; omit to pick one from the finest pin pitch (0.2 mm on ordinary boards)" },
                    "clearance": { "type": "number", "default": 0.2 },
                    "pairs": { "type": "array", "items": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 2 }, "description": "Differential pairs as [positive, negative] net names, in addition to the detected ones" },
                    "auto_pairs": { "type": "boolean", "default": true, "description": "Detect pairs from net names (D+/D-, DP/DM, _P/_N)" },
                    "pair_width": { "type": "number", "description": "Track width for pair nets (default signal_width; take it from calc_impedance)" },
                    "pair_gap": { "type": "number", "description": "Edge-to-edge gap inside a pair (default and minimum: clearance)" },
                    "pair_skew": { "type": "number", "default": 0.1, "description": "Largest allowed length difference inside a pair in mm" },
                    "fine_pitch": { "type": "boolean", "default": true, "description": "Neck tracks down at fine-pitch pads and pick a fine grid automatically" },
                    "signal_width": { "type": "number", "default": 0.25 },
                    "power_width": { "type": "number", "default": 0.5 },
                    "net_widths": { "type": "object", "additionalProperties": { "type": "number" }, "description": "Track width per net name" },
                    "via_size": { "type": "number", "default": 0.8 },
                    "via_drill": { "type": "number", "default": 0.4 },
                    "edge_clearance": { "type": "number", "default": 0.5 },
                    "prefer_layer": { "type": "string", "default": "F.Cu", "description": "Layer to favour; 'none' for no preference" },
                    "passes": { "type": "integer", "default": 3, "minimum": 1, "maximum": 8 }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_route(args, ctx).await }
        ),
        tool!(
            "smart_place",
            "Plan and apply a professional placement: connectors flush on a board edge, the main \
             ICs in the middle, every decoupling capacitor beside the power pin it serves and \
             turned along the IC's side, every other part next to the parts it connects to, \
             snapped to a grid with no courtyard overlaps and aligned into rows; then an optimizer \
             moves, turns and swaps identical parts to shorten the ratsnest and cut its crossings, \
             keeping every decoupling capacitor beside its pin. mode 'improve' (default) keeps a \
             person's layout and its connectors and moves parts at most max_shift mm; 'replace' \
             re-places everything and anneals it (connectors slide along their edge); 'respect' \
             only places what is missing, off the board or overlapping. Locked parts, mounting \
             holes and 'keep' references never move. Reports every move with its reason, the \
             connection length and crossings before and after. Defaults to a dry run; apply with \
             dry_run false and the plan_revision.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string" },
                    "dry_run": { "type": "boolean", "default": true },
                    "expected_plan_revision": { "type": "string" },
                    "grid": { "type": "number", "default": 0.25 },
                    "gap": { "type": "number", "default": 0.4, "description": "Minimum courtyard gap in mm" },
                    "edge_margin": { "type": "number", "default": 0.5 },
                    "keep": { "type": "array", "items": { "type": "string" }, "description": "References to leave where they are" },
                    "only": { "type": "array", "items": { "type": "string" }, "description": "Place only these references" },
                    "rotate": { "type": "boolean", "default": true },
                    "mode": { "type": "string", "enum": ["improve", "replace", "respect"], "default": "improve", "description": "improve: refine the current layout; replace: full re-placement; respect: place only what is missing" },
                    "max_shift": { "type": "number", "default": 5.0, "description": "improve mode: furthest a part may move, in mm" }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_place(args, ctx).await }
        ),
        tool!(
            "polish_board",
            "Give the board a finished, maker-brand look in one call: silkscreen labels beside \
             every named connector pin (top, and mirrored on the bottom for through-hole pins), a \
             product name and version block, an optional bottom-side brand line, the ground pour \
             mirrored onto the other outer layer of a 2-layer board, and a regular grid of ground \
             stitching vias clear of parts, pads, tracks and text. Never places silkscreen on \
             pads. Defaults to a dry run; apply with dry_run false and the plan_revision.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string" },
                    "dry_run": { "type": "boolean", "default": true },
                    "expected_plan_revision": { "type": "string" },
                    "title": { "type": "string", "description": "Product name (default: the project name)" },
                    "version": { "type": "string", "description": "e.g. v1.0" },
                    "back_text": { "type": "string", "description": "Brand or URL line for the bottom side" },
                    "labels": { "type": "boolean", "default": true },
                    "both_sides": { "type": "boolean", "default": true },
                    "label_size": { "type": "number", "default": 0.8 },
                    "stitch": { "type": "boolean", "default": true },
                    "stitch_pitch": { "type": "number", "default": 5.0 },
                    "via_size": { "type": "number", "default": 0.6 },
                    "via_drill": { "type": "number", "default": 0.3 },
                    "mirror_pours": { "type": "boolean", "default": true }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_polish(args, ctx).await }
        ),
        tool!(
            "audit_layout",
            "Review the layout the way a senior engineer and a product designer would, and score \
             it: engineering (unrouted connections, fast signals changing layer without a ground \
             via, fast traces near the edge, decoupling capacitors too far from their pins, thin \
             power copper, acute corners, vias in pads, missing ground plane) and beauty \
             (off-angle tracks, too many track widths, unlabelled connectors, no name/version, \
             sharp corners, missing mounting holes, sparse stitching, parts off the 90° grid). \
             Every finding names the tool that fixes it. Read-only; complements run_drc.",
            json!({
                "type": "object",
                "properties": {
                    "board": { "type": "string" },
                    "min_power_width": { "type": "number", "default": 0.4 },
                    "decoupling_mm": { "type": "number", "default": 3.0 },
                    "edge_mm": { "type": "number", "default": 1.0 },
                    "return_via_mm": { "type": "number", "default": 1.0 }
                },
                "required": ["board"]
            }),
            |args, ctx| async move { handle_audit(args, ctx).await }
        ),
    ]
}

async fn snapshot(
    ctx: &ToolContext,
    board: &std::path::Path,
) -> anyhow::Result<Result<(String, &'static str), CallToolResult>> {
    let live = crate::tools::with_board_ipc_classified(ctx, board, move |client, document| {
        client.save_document_to_string_in(document)
    })
    .await?;
    let saved = || -> anyhow::Result<Result<(String, &'static str), CallToolResult>> {
        if !board.exists() {
            return Ok(Err(CallToolResult::error_kind(
                ToolErrorKind::FileNotFound {
                    path: board.display().to_string(),
                },
                format!("Board file not found: {}", board.display()),
            )));
        }
        Ok(Ok((
            voltaris_sexp::writer::read_consistent(board)?,
            "saved_file",
        )))
    };
    Ok(match live {
        Ok(content) => Ok((content, "ipc")),
        Err(voltaris_ipc::IpcFailure::Unreachable(_)) => saved()?,
        Err(voltaris_ipc::IpcFailure::Target { error, .. }) if error.proves_not_open() => saved()?,
        Err(voltaris_ipc::IpcFailure::Target { error, .. }) => {
            Err(crate::tools::ipc_target_error_result(&error))
        }
        Err(voltaris_ipc::IpcFailure::Rejected(message)) => Err(CallToolResult::error(message)),
        Err(voltaris_ipc::IpcFailure::Recovered(message)) => {
            Err(crate::tools::ipc_recovered_result(&message))
        }
        Err(voltaris_ipc::IpcFailure::Uncertain(message)) => {
            Err(crate::tools::ipc_uncertain_result(&message))
        }
    })
}

fn revision(content: &str, tool: &str, args: &Value) -> String {
    let mut canonical = args.clone();
    if let Some(o) = canonical.as_object_mut() {
        o.remove("dry_run");
        o.remove("expected_plan_revision");
    }
    let mut h = Sha256::new();
    h.update(tool.as_bytes());
    h.update(
        serde_json::to_string(&canonical)
            .unwrap_or_default()
            .as_bytes(),
    );
    h.update(content.as_bytes());
    let digest = h.finalize();
    digest.iter().take(10).map(|b| format!("{b:02x}")).collect()
}

fn check_revision(args: &Value, actual: &str, tool: &str) -> Option<CallToolResult> {
    match opt_str(args, "expected_plan_revision") {
        None => Some(invalid_arg(
            "expected_plan_revision",
            &format!("applying {tool} needs the plan_revision from its dry run"),
        )),
        Some(expected) if expected != actual => Some(CallToolResult::error_kind(
            ToolErrorKind::PlanBlocked {
                operation: tool.to_string(),
                reasons: vec!["the board or the arguments changed since the dry run".into()],
            },
            format!(
                "{tool}: the board changed since the dry run (plan {expected}, now {actual}); run the dry run again"
            ),
        )),
        Some(_) => None,
    }
}

async fn apply_live<T, F>(
    ctx: &ToolContext,
    board: &std::path::Path,
    tool: &str,
    f: F,
) -> anyhow::Result<Result<T, CallToolResult>>
where
    T: Send + 'static,
    F: FnOnce(
            &voltaris_ipc::client::KiCadIpcClient,
            voltaris_ipc::gen::kiapi::common::types::DocumentSpecifier,
        ) -> anyhow::Result<T>
        + Send
        + 'static,
{
    Ok(match live_board::observe(ctx, board, f).await? {
        LiveBoard::Answered(value) => Ok(value),
        LiveBoard::Recovered(m) => Err(crate::tools::ipc_recovered_result(&m)),
        LiveBoard::Uncertain(m) => Err(crate::tools::ipc_uncertain_result(&m)),
        LiveBoard::Rejected(m) | LiveBoard::Unserved { message: m, .. } => Err(CallToolResult::error(
            format!("KiCad rejected {tool}: {m}. Nothing was changed."),
        )),
        LiveBoard::NotOpen { message, .. } => Err(CallToolResult::error(format!(
            "{tool} applies live: open this board in KiCad's PCB editor first ({message})."
        ))),
        LiveBoard::Unresolved(e) => Err(crate::tools::ipc_target_error_result(&e)),
        LiveBoard::NeverReached(m) => Err(CallToolResult::error(format!(
            "{tool} applies live and KiCad is unreachable ({m}); start KiCad with the API enabled and open this board."
        ))),
        LiveBoard::LostAfterObservation { situation, .. } => {
            Err(CallToolResult::error(format!("{tool}: {situation}. Nothing was changed.")))
        }
    })
}

fn f64_arg(args: &Value, key: &str, default: f64) -> f64 {
    args[key]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .unwrap_or(default)
}

fn str_list(args: &Value, key: &str) -> Option<Vec<String>> {
    args[key].as_array().map(|a| {
        a.iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect()
    })
}

macro_rules! load {
    ($ctx:expr, $args:expr) => {{
        let path = get_path($args, "board")?;
        match snapshot($ctx, &path).await? {
            Ok((content, source)) => (path, content, source),
            Err(result) => return Ok(result),
        }
    }};
}

async fn handle_route(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let (path, content, source) = load!(ctx, args);
    let board = Board::from_text(&content)?;
    let mut opts = router::RouteOptions::default();
    if let Some(l) = str_list(args, "layers") {
        opts.layers = l;
    }
    opts.only_nets = str_list(args, "nets");
    opts.grid = args["grid"].as_f64().filter(|g| g.is_finite() && *g > 0.0);
    opts.auto_pairs = args["auto_pairs"].as_bool().unwrap_or(true);
    opts.fine_pitch = args["fine_pitch"].as_bool().unwrap_or(true);
    opts.pair_width = args["pair_width"]
        .as_f64()
        .filter(|w| w.is_finite() && *w > 0.0);
    opts.pair_gap = args["pair_gap"]
        .as_f64()
        .filter(|g| g.is_finite() && *g > 0.0);
    opts.pair_skew = f64_arg(args, "pair_skew", opts.pair_skew);
    if let Some(pairs) = args["pairs"].as_array() {
        for pair in pairs {
            match pair
                .as_array()
                .map(|p| p.iter().filter_map(Value::as_str).collect::<Vec<_>>())
            {
                Some(p) if p.len() == 2 => opts.pairs.push((p[0].to_string(), p[1].to_string())),
                _ => {
                    return Ok(invalid_arg(
                        "pairs",
                        "each pair is [positive, negative] net names",
                    ))
                }
            }
        }
    }
    opts.clearance = f64_arg(args, "clearance", opts.clearance);
    opts.signal_width = f64_arg(args, "signal_width", opts.signal_width);
    opts.power_width = f64_arg(args, "power_width", opts.power_width);
    opts.via_size = f64_arg(args, "via_size", opts.via_size);
    opts.via_drill = f64_arg(args, "via_drill", opts.via_drill);
    opts.edge_clearance = f64_arg(args, "edge_clearance", opts.edge_clearance);
    opts.passes = args["passes"].as_u64().unwrap_or(3).clamp(1, 8) as usize;
    opts.prefer_layer = match opt_str(args, "prefer_layer") {
        Some("none") => None,
        Some(l) => Some(l.to_string()),
        None => opts.prefer_layer,
    };
    if let Some(map) = args["net_widths"].as_object() {
        opts.net_widths = map
            .iter()
            .filter_map(|(k, v)| v.as_f64().map(|w| (k.clone(), w)))
            .collect();
    }
    if opts.via_drill >= opts.via_size {
        return Ok(invalid_arg("via_drill", "must be smaller than via_size"));
    }
    let started = std::time::Instant::now();
    let result = match tokio::task::spawn_blocking({
        let opts = opts.clone();
        move || router::route(&board, &opts)
    })
    .await?
    {
        Ok(r) => r,
        Err(e) => return Ok(CallToolResult::error(e.to_string())),
    };
    let elapsed = started.elapsed().as_millis();
    let rev = revision(&content, "route_board", args);
    let (routed, total) = result.routed_connections();
    let nets: Vec<Value> = result
        .nets
        .iter()
        .map(|n| {
            json!({
                "net": n.net,
                "status": n.status,
                "connections": format!("{}/{}", n.connections_routed, n.connections_total),
                "segments": n.segments.len(),
                "vias": n.vias.len(),
                "length_mm": round3(n.length()),
                "detail": n.detail,
            })
        })
        .collect();
    let summary = json!({
        "connections_routed": routed,
        "connections_total": total,
        "completion_pct": if total == 0 { 100.0 } else { round3(routed as f64 * 100.0 / total as f64) },
        "nets_routed": result.nets.iter().filter(|n| n.status == "routed").count(),
        "nets_partial": result.nets.iter().filter(|n| n.status == "partial").count(),
        "nets_failed": result.nets.iter().filter(|n| n.status == "failed").count(),
        "vias": result.vias(),
        "track_length_mm": round3(result.length()),
        "pass_used": result.pass_used,
        "grid_mm": result.grid,
        "router_ms": elapsed,
    });
    let pairs: Vec<Value> = result
        .pairs
        .iter()
        .map(|p| {
            json!({
                "positive": p.positive,
                "negative": p.negative,
                "status": p.status,
                "width_mm": p.width,
                "gap_mm": p.gap,
                "length_mm": [round3(p.length_positive), round3(p.length_negative)],
                "skew_mm": round3(p.skew),
                "coupled_pct": round3(p.coupled_pct),
                "tuned_mm": round3(p.tuned_mm),
            })
        })
        .collect();
    let fine: Vec<Value> = result
        .fine_pitch
        .iter()
        .map(|f| json!({ "reference": f.reference, "pitch_mm": f.pitch, "pad_width_mm": f.pad_width, "neck_width_mm": f.neck }))
        .collect();
    let dry = args["dry_run"].as_bool().unwrap_or(true);
    if dry {
        return Ok(CallToolResult::json(&json!({
            "dry_run": true,
            "source": source,
            "plan_revision": rev,
            "summary": summary,
            "pairs": pairs,
            "fine_pitch": fine,
            "nets": nets,
            "warnings": result.warnings,
            "next": "Apply with dry_run false and this plan_revision, then refill_zones and run_drc."
        })));
    }
    if let Some(refusal) = check_revision(args, &rev, "route_board") {
        return Ok(refusal);
    }
    type NetCopper = (String, Vec<router::RoutedSegment>, Vec<(f64, f64)>);
    let copper: Vec<NetCopper> = result
        .nets
        .iter()
        .filter(|n| !n.segments.is_empty() || !n.vias.is_empty())
        .map(|n| (n.net.clone(), n.segments.clone(), n.vias.clone()))
        .collect();
    let (via_drill, via_size) = (opts.via_drill, opts.via_size);
    let applied = apply_live(ctx, &path, "route_board", move |c, doc| {
        c.run_commit("Voltaris route_board", |c| {
            let mut items = Vec::new();
            for (net, segs, vias) in &copper {
                let code = c.resolve_net_code_in(doc.clone(), net)?;
                for s in segs {
                    let t = builders::build_track(
                        net, code, &s.layer, s.width, s.a.0, s.a.1, s.b.0, s.b.1,
                    );
                    items.push(builders::pack_any(&t, "kiapi.board.types.Track"));
                }
                for v in vias {
                    let via = builders::build_via(net, code, v.0, v.1, via_drill, via_size);
                    items.push(builders::pack_any(&via, "kiapi.board.types.Via"));
                }
            }
            let n = items.len();
            if n > 0 {
                c.create_items_in(doc.clone(), items)?;
            }
            Ok(n)
        })
    })
    .await?;
    match applied {
        Ok(n) => Ok(CallToolResult::json(&json!({
            "dry_run": false,
            "applied_items": n,
            "plan_revision": rev,
            "summary": summary,
            "pairs": pairs,
            "fine_pitch": fine,
            "nets": nets,
            "warnings": result.warnings,
            "undo": "One KiCad undo step removes everything route_board added.",
            "next": "refill_zones, then run_drc with sync_live_board and audit_layout."
        }))),
        Err(r) => Ok(r),
    }
}

async fn handle_place(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let (path, content, source) = load!(ctx, args);
    let board = Board::from_text(&content)?;
    let opts = placer::PlaceOptions {
        grid: f64_arg(args, "grid", 0.25).max(0.05),
        gap: f64_arg(args, "gap", 0.4),
        edge_margin: f64_arg(args, "edge_margin", 0.5),
        keep: str_list(args, "keep")
            .unwrap_or_default()
            .into_iter()
            .collect(),
        only: str_list(args, "only").map(|v| v.into_iter().collect::<BTreeSet<_>>()),
        rotate: args["rotate"].as_bool().unwrap_or(true),
        mode: match opt_str(args, "mode").unwrap_or("improve") {
            "improve" => placer::PlaceMode::Improve,
            "replace" => placer::PlaceMode::Replace,
            "respect" => placer::PlaceMode::Respect,
            _ => return Ok(invalid_arg("mode", "use improve, replace or respect")),
        },
        max_shift: f64_arg(args, "max_shift", 5.0),
        iterations: 0,
    };
    let plan = match tokio::task::spawn_blocking(move || placer::plan(&board, &opts)).await? {
        Ok(p) => p,
        Err(e) => return Ok(CallToolResult::error(e.to_string())),
    };
    let rev = revision(&content, "smart_place", args);
    let reduction = if plan.airwire_before > 0.0 {
        round3((1.0 - plan.airwire_after / plan.airwire_before) * 100.0)
    } else {
        0.0
    };
    let moves: Vec<Value> = plan
        .moves
        .iter()
        .map(|m| {
            json!({
                "reference": m.reference,
                "role": m.role,
                "from": { "x": round3(m.from.0), "y": round3(m.from.1), "rotation": m.from.2 },
                "to": { "x": m.to.0, "y": m.to.1, "rotation": m.to.2 },
                "reason": m.reason,
            })
        })
        .collect();
    let body = json!({
        "source": source,
        "plan_revision": rev,
        "moves": moves,
        "kept": plan.kept.iter().map(|(r, why)| json!({"reference": r, "why": why})).collect::<Vec<_>>(),
        "unplaced": plan.unplaced.iter().map(|(r, why)| json!({"reference": r, "why": why})).collect::<Vec<_>>(),
        "connection_length_mm": { "before": round3(plan.airwire_before), "after": round3(plan.airwire_after), "reduction_pct": reduction },
        "ratsnest_crossings": { "before": plan.crossings_before, "after": plan.crossings_after },
        "optimizer": plan.refine.map(|r| json!({ "iterations": r.iterations, "accepted_moves": r.accepted, "cost_before": round3(r.cost_before), "cost_after": round3(r.cost_after) })),
    });
    if args["dry_run"].as_bool().unwrap_or(true) {
        let mut b = body;
        b["dry_run"] = json!(true);
        b["next"] = json!(
            "Apply with dry_run false and this plan_revision, then score_placement and render_board_3d."
        );
        return Ok(CallToolResult::json(&b));
    }
    if let Some(refusal) = check_revision(args, &rev, "smart_place") {
        return Ok(refusal);
    }
    if plan.moves.is_empty() {
        return Ok(CallToolResult::json(
            &json!({ "dry_run": false, "applied": 0, "plan_revision": rev }),
        ));
    }
    let placements: Vec<Value> = plan
        .moves
        .iter()
        .map(|m| json!({ "reference": m.reference, "x": m.to.0, "y": m.to.1, "rotation": m.to.2 }))
        .collect();
    let applied = crate::tools::pcb_components::handle_set_component_placements(
        &json!({ "board": path.display().to_string(), "placements": placements }),
        ctx,
    )
    .await?;
    if applied.is_error {
        return Ok(applied);
    }
    let mut b = body;
    b["dry_run"] = json!(false);
    b["applied"] = json!(plan.moves.len());
    b["undo"] = json!("One KiCad undo step reverses the whole placement.");
    b["next"] = json!("score_placement, then route_board.");
    Ok(CallToolResult::json(&b))
}

async fn handle_polish(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let (path, content, source) = load!(ctx, args);
    let board = Board::from_text(&content)?;
    let project = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("board")
        .to_string();
    let opts = polish::PolishOptions {
        labels: args["labels"].as_bool().unwrap_or(true),
        both_sides: args["both_sides"].as_bool().unwrap_or(true),
        label_size: f64_arg(args, "label_size", 0.8).max(0.8),
        stitch: args["stitch"].as_bool().unwrap_or(true),
        stitch_pitch: f64_arg(args, "stitch_pitch", 5.0),
        via_size: f64_arg(args, "via_size", 0.6),
        via_drill: f64_arg(args, "via_drill", 0.3),
        mirror_pours: args["mirror_pours"].as_bool().unwrap_or(true),
        title: opt_str(args, "title").map(str::to_string),
        version: opt_str(args, "version").map(str::to_string),
        back_text: opt_str(args, "back_text").map(str::to_string),
        ..Default::default()
    };
    let plan = match polish::plan(&board, &project, &opts) {
        Ok(p) => p,
        Err(e) => return Ok(CallToolResult::error(e.to_string())),
    };
    let rev = revision(&content, "polish_board", args);
    let body = json!({
        "source": source,
        "plan_revision": rev,
        "texts": plan.texts.iter().map(|t| json!({
            "layer": t.layer, "text": t.text, "x": round3(t.at.0), "y": round3(t.at.1),
            "size": t.size, "rotation": t.rotation, "purpose": t.purpose
        })).collect::<Vec<_>>(),
        "zones": plan.zones.iter().map(|z| json!({"net": z.net, "layer": z.layer, "points": z.points.len()})).collect::<Vec<_>>(),
        "stitching_vias": plan.vias.len(),
        "notes": plan.notes,
    });
    if args["dry_run"].as_bool().unwrap_or(true) {
        let mut b = body;
        b["dry_run"] = json!(true);
        b["next"] = json!(
            "Apply with dry_run false and this plan_revision, then run_drc and render_board_3d."
        );
        return Ok(CallToolResult::json(&b));
    }
    if let Some(refusal) = check_revision(args, &rev, "polish_board") {
        return Ok(refusal);
    }
    let texts = plan.texts.clone();
    let vias = plan.vias.clone();
    let zones = plan.zones.clone();
    let (via_drill, via_size) = (opts.via_drill, opts.via_size);
    let applied = apply_live(ctx, &path, "polish_board", move |c, doc| {
        let n = c.run_commit("Voltaris polish_board", |c| {
            let mut items = Vec::new();
            for t in &texts {
                let bt = builders::board_text(
                    &t.layer, &t.text, t.at.0, t.at.1, t.size, t.rotation, t.mirror,
                );
                items.push(builders::pack_any(&bt, "kiapi.board.types.BoardText"));
            }
            let mut codes: BTreeMap<String, i32> = BTreeMap::new();
            for (net, at) in &vias {
                let code = match codes.get(net) {
                    Some(c) => *c,
                    None => {
                        let code = c.resolve_net_code_in(doc.clone(), net)?;
                        codes.insert(net.clone(), code);
                        code
                    }
                };
                let via = builders::build_via(net, code, at.0, at.1, via_drill, via_size);
                items.push(builders::pack_any(&via, "kiapi.board.types.Via"));
            }
            for z in &zones {
                let code = c.resolve_net_code_in(doc.clone(), &z.net)?;
                let spec = builders::ZoneSpec {
                    layer: &z.layer,
                    net_name: &z.net,
                    points: &z.points,
                    clearance_mm: 0.3,
                    min_thickness_mm: 0.25,
                    name: "",
                    priority: 0,
                    connection:
                        voltaris_ipc::gen::kiapi::board::types::ZoneConnectionStyle::ZcsThermal,
                };
                let zone = builders::build_zone(&spec, code);
                items.push(builders::pack_any(&zone, "kiapi.board.types.Zone"));
            }
            let n = items.len();
            if n > 0 {
                c.create_items_in(doc.clone(), items)?;
            }
            Ok(n)
        })?;
        if !zones.is_empty() || !vias.is_empty() {
            c.refill_zones()?;
        }
        Ok(n)
    })
    .await?;
    match applied {
        Ok(n) => {
            let mut b = body;
            b["dry_run"] = json!(false);
            b["applied_items"] = json!(n);
            b["undo"] = json!("One KiCad undo step removes everything polish_board added.");
            b["next"] = json!("run_drc with sync_live_board, then render_board_3d.");
            Ok(CallToolResult::json(&b))
        }
        Err(r) => Ok(r),
    }
}

async fn handle_audit(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let (_path, content, source) = load!(ctx, args);
    let board = Board::from_text(&content)?;
    let opts = audit::AuditOptions {
        min_power_width: f64_arg(args, "min_power_width", 0.4),
        decoupling_mm: f64_arg(args, "decoupling_mm", 3.0),
        edge_mm: f64_arg(args, "edge_mm", 1.0),
        return_via_mm: f64_arg(args, "return_via_mm", 1.0),
    };
    let a = audit::audit(&board, &opts);
    let grade = |s: i64| match s {
        90.. => "A",
        80..=89 => "B",
        70..=79 => "C",
        60..=69 => "D",
        _ => "F",
    };
    let findings: Vec<Value> = a
        .findings
        .iter()
        .map(|f| {
            json!({
                "severity": f.severity,
                "area": f.area,
                "check": f.check,
                "message": f.message,
                "at": f.at.map(|(x, y)| json!({"x": round3(x), "y": round3(y)})),
                "fix_with": f.fix,
            })
        })
        .collect();
    Ok(CallToolResult::json(&json!({
        "source": source,
        "engineering_score": a.engineering,
        "engineering_grade": grade(a.engineering),
        "beauty_score": a.beauty,
        "beauty_grade": grade(a.beauty),
        "findings": findings,
        "metrics": a.metrics,
        "note": "Scores are heuristics on top of KiCad's DRC, not a replacement for it; run_drc stays the authority on clearances and connectivity."
    })))
}
