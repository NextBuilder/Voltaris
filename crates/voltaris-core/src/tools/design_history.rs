//! `design_history` toolset — named snapshots of a KiCad project and an
//! engineering diff between any two of them (or a snapshot and the files on
//! disk): parts, connectivity, placement and routed lengths.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::design_model::{self, Design};
use crate::tools::signal_integrity::measure;
use crate::tools::{invalid_arg, opt_str, require_str, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use voltaris_sexp::parser::{parse_sexp, SexpNode};

const LIMIT: usize = 200;

pub struct Project {
    pub dir: PathBuf,
    pub name: String,
}

impl Project {
    pub fn locate(arg: &str) -> Result<Project, String> {
        let p = PathBuf::from(arg);
        if p.is_file() {
            let stem = p
                .file_stem()
                .and_then(|s| s.to_str())
                .ok_or("bad file name")?
                .to_string();
            return Ok(Project {
                dir: p.parent().map(Path::to_path_buf).unwrap_or_default(),
                name: stem,
            });
        }
        if !p.is_dir() {
            return Err("path does not exist".into());
        }
        let pros: Vec<PathBuf> = std::fs::read_dir(&p)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|f| f.extension().is_some_and(|x| x == "kicad_pro"))
            .collect();
        match pros.as_slice() {
            [one] => Ok(Project {
                dir: p.clone(),
                name: one.file_stem().unwrap().to_string_lossy().into(),
            }),
            [] => Err("no .kicad_pro in this directory".into()),
            _ => Err("several .kicad_pro files here — pass the .kicad_pro path".into()),
        }
    }
    pub fn sch(&self) -> PathBuf {
        self.dir.join(format!("{}.kicad_sch", self.name))
    }
    pub fn pcb(&self) -> PathBuf {
        self.dir.join(format!("{}.kicad_pcb", self.name))
    }
    fn store(&self) -> PathBuf {
        self.dir.join(".voltaris").join("snapshots")
    }
}

fn wanted(rel: &Path) -> bool {
    let name = rel.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let ext = rel.extension().and_then(|e| e.to_str()).unwrap_or("");
    ["kicad_sch", "kicad_pcb", "kicad_pro", "kicad_dru"].contains(&ext)
        || name == "sym-lib-table"
        || name == "fp-lib-table"
}

pub fn project_files(dir: &Path) -> Vec<PathBuf> {
    fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().to_string();
            if p.is_dir() {
                if depth < 3
                    && !name.starts_with('.')
                    && !name.ends_with("-backups")
                    && name != "production"
                {
                    walk(root, &p, depth + 1, out);
                }
            } else if let Ok(rel) = p.strip_prefix(root) {
                if wanted(rel) {
                    out.push(rel.to_path_buf());
                }
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, 0, &mut out);
    out.sort();
    out
}

fn utc_stamp() -> (String, String) {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let (h, mi, s) = (rem / 3600, rem % 3600 / 60, rem % 60);
    (
        format!("{y:04}{m:02}{d:02}-{h:02}{mi:02}{s:02}"),
        format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z"),
    )
}

#[derive(Debug, Default)]
pub struct BoardState {
    pub footprints: BTreeMap<String, (String, f64, f64, f64, String)>,
    pub lengths: BTreeMap<String, (f64, usize)>,
}

pub fn board_state(tree: &SexpNode) -> BoardState {
    let mut b = BoardState::default();
    for fp in voltaris_sexp::board::footprints(tree) {
        let reference = fp
            .find_all("property")
            .into_iter()
            .find(|p| p.get(1).and_then(|n| n.as_str()) == Some("Reference"))
            .and_then(|p| p.get(2))
            .and_then(|n| n.as_str())
            .map(str::to_string);
        let Some(reference) = reference else { continue };
        let lib = fp.get(1).and_then(|n| n.as_str()).unwrap_or("").to_string();
        let at = fp.find("at");
        let num = |i| {
            at.and_then(|a| a.get(i))
                .and_then(|n| n.as_str())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(0.0)
        };
        let layer = fp
            .find("layer")
            .and_then(|l| l.get(1))
            .and_then(|n| n.as_str())
            .unwrap_or("")
            .to_string();
        b.footprints
            .insert(reference, (lib, num(1), num(2), num(3), layer));
    }
    for (net, l) in measure(tree) {
        b.lengths.insert(net, (l.length, l.vias));
    }
    b
}

fn truncate<T: serde::Serialize>(v: Vec<T>) -> Value {
    let n = v.len();
    if n > LIMIT {
        json!({ "count": n, "shown": LIMIT, "items": v.into_iter().take(LIMIT).collect::<Vec<_>>() })
    } else {
        json!(v)
    }
}

pub fn diff_designs(a: &Design, b: &Design) -> Value {
    let skip = ["Reference", "Value", "Footprint"];
    let added: Vec<Value> = b
        .components
        .iter()
        .filter(|(r, _)| !a.components.contains_key(*r))
        .map(|(r, c)| json!({"reference": r, "value": c.value, "footprint": c.footprint}))
        .collect();
    let removed: Vec<Value> = a
        .components
        .iter()
        .filter(|(r, _)| !b.components.contains_key(*r))
        .map(|(r, c)| json!({"reference": r, "value": c.value, "footprint": c.footprint}))
        .collect();
    let mut changed = Vec::new();
    for (r, ca) in &a.components {
        let Some(cb) = b.components.get(r) else {
            continue;
        };
        let mut ch = Vec::new();
        for (k, va, vb) in [
            ("value", &ca.value, &cb.value),
            ("footprint", &ca.footprint, &cb.footprint),
            ("lib_id", &ca.lib_id, &cb.lib_id),
        ] {
            if va != vb && !(k == "lib_id" && (va.is_empty() || vb.is_empty())) {
                ch.push(json!({"what": k, "from": va, "to": vb}));
            }
        }
        let keys: BTreeSet<&String> = ca
            .fields
            .keys()
            .chain(cb.fields.keys())
            .filter(|k| !skip.contains(&k.as_str()))
            .collect();
        for k in keys {
            let (fa, fb) = (
                ca.fields.get(k).map(String::as_str).unwrap_or(""),
                cb.fields.get(k).map(String::as_str).unwrap_or(""),
            );
            if fa != fb {
                ch.push(json!({"what": format!("field {k}"), "from": fa, "to": fb}));
            }
        }
        if !ch.is_empty() {
            changed.push(json!({"reference": r, "changes": ch}));
        }
    }
    let membership = |d: &Design| {
        let mut pin_net: BTreeMap<String, String> = BTreeMap::new();
        let mut members: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for (net, nodes) in &d.nets {
            for n in nodes {
                if design_model::is_virtual(&n.reference) {
                    continue;
                }
                let id = format!("{}.{}", n.reference, n.pin);
                pin_net.insert(id.clone(), net.clone());
                members.entry(net.clone()).or_default().insert(id);
            }
        }
        (pin_net, members)
    };
    let (pa, ma) = membership(a);
    let (pb, mb) = membership(b);
    let mut moved = Vec::new();
    let mut renamed = BTreeMap::new();
    let both: BTreeSet<&str> = a
        .components
        .keys()
        .filter(|r| b.components.contains_key(*r))
        .map(String::as_str)
        .collect();
    for (pin, na) in &pa {
        let Some(nb) = pb.get(pin) else { continue };
        if !b
            .components
            .contains_key(pin.split('.').next().unwrap_or(""))
        {
            continue;
        }
        let peers = |m: &BTreeMap<String, BTreeSet<String>>, net: &str| -> BTreeSet<String> {
            m.get(net)
                .map(|s| {
                    s.iter()
                        .filter(|p| *p != pin && both.contains(p.split('.').next().unwrap_or("")))
                        .cloned()
                        .collect()
                })
                .unwrap_or_default()
        };
        let (sa, sb) = (peers(&ma, na), peers(&mb, nb));
        if sa != sb {
            moved.push(json!({
                "pin": pin, "from_net": na, "to_net": nb,
                "now_also_connects": sb.difference(&sa).collect::<Vec<_>>(),
                "no_longer_connects": sa.difference(&sb).collect::<Vec<_>>(),
            }));
        } else if na != nb && !(na.starts_with("Net-(") && nb.starts_with("Net-(")) {
            renamed.insert(na.clone(), nb.clone());
        }
    }
    let named = |m: &BTreeMap<String, BTreeSet<String>>| {
        m.keys()
            .filter(|n| !n.starts_with("Net-(") && !design_model::is_unconnected(n))
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let (na, nb) = (named(&ma), named(&mb));
    let renamed_from: BTreeSet<String> = renamed.keys().cloned().collect();
    let renamed_to: BTreeSet<String> = renamed.values().cloned().collect();
    json!({
        "components_added": truncate(added),
        "components_removed": truncate(removed),
        "components_changed": truncate(changed),
        "connections_changed": truncate(moved),
        "nets_renamed": renamed.into_iter().map(|(f, t)| json!({"from": f, "to": t})).collect::<Vec<_>>(),
        "nets_added": nb.difference(&na).filter(|n| !renamed_to.contains(*n)).collect::<Vec<_>>(),
        "nets_removed": na.difference(&nb).filter(|n| !renamed_from.contains(*n)).collect::<Vec<_>>(),
    })
}

pub fn diff_boards(a: &BoardState, b: &BoardState) -> Value {
    let mut moved = Vec::new();
    for (r, fa) in &a.footprints {
        let Some(fb) = b.footprints.get(r) else {
            continue;
        };
        let d = (fb.1 - fa.1).hypot(fb.2 - fa.2);
        let rot = ((fb.3 - fa.3).rem_euclid(360.0) * 1000.0).round() / 1000.0;
        if d > 0.005 || (rot > 0.001 && rot < 359.999) || fa.4 != fb.4 || fa.0 != fb.0 {
            let mut e = json!({"reference": r, "from": [fa.1, fa.2, fa.3], "to": [fb.1, fb.2, fb.3], "moved_mm": (d * 1000.0).round() / 1000.0});
            if fa.4 != fb.4 {
                e["side"] = json!({"from": fa.4, "to": fb.4});
            }
            if fa.0 != fb.0 {
                e["footprint"] = json!({"from": fa.0, "to": fb.0});
            }
            moved.push(e);
        }
    }
    let placed: Vec<&String> = b
        .footprints
        .keys()
        .filter(|r| !a.footprints.contains_key(*r))
        .collect();
    let deleted: Vec<&String> = a
        .footprints
        .keys()
        .filter(|r| !b.footprints.contains_key(*r))
        .collect();
    let nets: BTreeSet<&String> = a.lengths.keys().chain(b.lengths.keys()).collect();
    let mut lengths = Vec::new();
    for n in nets {
        let (la, va) = a.lengths.get(n).copied().unwrap_or((0.0, 0));
        let (lb, vb) = b.lengths.get(n).copied().unwrap_or((0.0, 0));
        if (lb - la).abs() > 0.01 || va != vb {
            let r3 = |x: f64| (x * 1000.0).round() / 1000.0;
            lengths.push(json!({"net": n, "length_mm": [r3(la), r3(lb)], "delta_mm": r3(lb - la), "vias": [va, vb]}));
        }
    }
    let total = |s: &BoardState| s.lengths.values().map(|v| v.0).sum::<f64>();
    json!({
        "footprints_moved": truncate(moved),
        "footprints_added": placed,
        "footprints_removed": deleted,
        "routing_changed": truncate(lengths),
        "total_routed_mm": [(total(a) * 10.0).round() / 10.0, (total(b) * 10.0).round() / 10.0],
    })
}

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "snapshot_design",
            "Save a named snapshot of the project — every schematic sheet, the board, project and \
             rule files, plus a full-hierarchy netlist model — under <project>/.voltaris/snapshots. \
             Take one before any large change (template application, re-placement, autorouting) so it \
             can be diffed and restored.",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Path to the .kicad_pro or project directory" },
                    "label": { "type": "string", "description": "Short note, e.g. 'before autoroute'" }
                },
                "required": ["project"]
            }),
            |args, ctx| async move { handle_snapshot(args, ctx).await }
        ),
        tool!(
            "list_snapshots",
            "List this project's snapshots, newest first, with label, time, part and net counts.",
            json!({
                "type": "object",
                "properties": { "project": { "type": "string" } },
                "required": ["project"]
            }),
            |args, _ctx| async move { handle_list(args) }
        ),
        tool!(
            "diff_design",
            "Engineering diff between two states of the project: parts added/removed/changed (value, \
             footprint, symbol, every field such as MPN or LCSC), connections that changed (by pin, \
             independent of auto-generated net names), nets added/removed/renamed, footprints moved, \
             rotated or flipped, and per-net routed length and via changes. 'from' and 'to' are snapshot \
             ids or 'current' (the files on disk).",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string" },
                    "from": { "type": "string", "description": "Snapshot id (default: the newest)" },
                    "to": { "type": "string", "default": "current" }
                },
                "required": ["project"]
            }),
            |args, ctx| async move { handle_diff(args, ctx).await }
        ),
        tool!(
            "restore_snapshot",
            "Put a snapshot's files back into the project. The current state is snapshotted first \
             (label 'before restore of <id>'), so a restore can itself be undone. Refuses while KiCad \
             has the board open; close the schematic in KiCad without saving before restoring.",
            json!({
                "type": "object",
                "properties": { "project": { "type": "string" }, "id": { "type": "string" } },
                "required": ["project", "id"]
            }),
            |args, ctx| async move { handle_restore(args, ctx).await }
        ),
    ]
}

fn project_arg(args: &Value) -> Result<Project, CallToolResult> {
    let p = require_str(args, "project")?;
    Project::locate(p).map_err(|e| invalid_arg("project", &e))
}

pub async fn take_snapshot(
    project: &Project,
    label: &str,
    kicad_cli: &str,
) -> anyhow::Result<Value> {
    let (stamp, iso) = utc_stamp();
    let slug = crate::tools::design_checks::slug(label);
    let base = if slug.is_empty() {
        stamp
    } else {
        format!("{stamp}-{slug}")
    };
    let mut id = base.clone();
    let mut k = 2;
    while project.store().join(&id).exists() {
        id = format!("{base}-{k}");
        k += 1;
    }
    let dir = project.store().join(&id);
    let files = project_files(&project.dir);
    for rel in &files {
        let dst = dir.join("files").join(rel);
        std::fs::create_dir_all(dst.parent().unwrap())?;
        std::fs::copy(project.dir.join(rel), dst)?;
    }
    let design = if project.sch().is_file() {
        Some(design_model::load(&project.sch(), kicad_cli).await?)
    } else {
        None
    };
    if let Some(d) = &design {
        std::fs::write(dir.join("design.json"), serde_json::to_string(d)?)?;
    }
    let meta = json!({
        "id": id, "label": label, "created": iso,
        "files": files.iter().map(|f| f.to_string_lossy().replace('\\', "/")).collect::<Vec<_>>(),
        "parts": design.as_ref().map(|d| d.components.len()),
        "nets": design.as_ref().map(|d| d.nets.len()),
        "design_source": design.as_ref().map(|d| d.source.clone()),
    });
    std::fs::write(
        dir.join("snapshot.json"),
        serde_json::to_string_pretty(&meta)?,
    )?;
    Ok(meta)
}

async fn handle_snapshot(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = match project_arg(args) {
        Ok(p) => p,
        Err(e) => return Ok(e),
    };
    let label = opt_str(args, "label").unwrap_or("");
    let meta = take_snapshot(&project, label, &ctx.config.kicad_cli).await?;
    Ok(CallToolResult::json(&meta))
}

fn snapshots(project: &Project) -> Vec<Value> {
    let mut out: Vec<Value> = std::fs::read_dir(project.store())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| std::fs::read_to_string(e.path().join("snapshot.json")).ok())
                .filter_map(|t| serde_json::from_str(&t).ok())
                .collect()
        })
        .unwrap_or_default();
    out.sort_by(|a, b| b["id"].as_str().cmp(&a["id"].as_str()));
    out
}

fn handle_list(args: &Value) -> anyhow::Result<CallToolResult> {
    let project = match project_arg(args) {
        Ok(p) => p,
        Err(e) => return Ok(e),
    };
    let list = snapshots(&project);
    Ok(CallToolResult::json(
        &json!({ "count": list.len(), "snapshots": list }),
    ))
}

struct State {
    design: Option<Design>,
    board: Option<BoardState>,
}

async fn state_of(project: &Project, id: &str, kicad_cli: &str) -> Result<State, String> {
    if id == "current" {
        let design = if project.sch().is_file() {
            Some(
                design_model::load(&project.sch(), kicad_cli)
                    .await
                    .map_err(|e| e.to_string())?,
            )
        } else {
            None
        };
        let board = std::fs::read_to_string(project.pcb())
            .ok()
            .and_then(|t| parse_sexp(&t).ok())
            .map(|t| board_state(&t));
        return Ok(State { design, board });
    }
    let dir = project.store().join(id);
    if !dir.join("snapshot.json").is_file() {
        return Err(format!("no snapshot '{id}' (see list_snapshots)"));
    }
    let design = std::fs::read_to_string(dir.join("design.json"))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
    let board = std::fs::read_to_string(
        dir.join("files")
            .join(format!("{}.kicad_pcb", project.name)),
    )
    .ok()
    .and_then(|t| parse_sexp(&t).ok())
    .map(|t| board_state(&t));
    Ok(State { design, board })
}

async fn handle_diff(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = match project_arg(args) {
        Ok(p) => p,
        Err(e) => return Ok(e),
    };
    let from = match opt_str(args, "from") {
        Some(f) => f.to_string(),
        None => match snapshots(&project).first().and_then(|s| s["id"].as_str()) {
            Some(id) => id.to_string(),
            None => {
                return Ok(invalid_arg(
                    "from",
                    "this project has no snapshots yet — take one with snapshot_design",
                ))
            }
        },
    };
    let to = opt_str(args, "to").unwrap_or("current").to_string();
    let (a, b) = match (
        state_of(&project, &from, &ctx.config.kicad_cli).await,
        state_of(&project, &to, &ctx.config.kicad_cli).await,
    ) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => return Ok(invalid_arg("from", &e)),
        (_, Err(e)) => return Ok(invalid_arg("to", &e)),
    };
    let schematic = match (&a.design, &b.design) {
        (Some(x), Some(y)) => diff_designs(x, y),
        _ => json!("schematic missing on one side"),
    };
    let board = match (&a.board, &b.board) {
        (Some(x), Some(y)) => diff_boards(x, y),
        (None, None) => json!("no board on either side"),
        _ => json!("board exists on only one side"),
    };
    let count = |v: &Value, k: &str| {
        v.get(k)
            .map(|x| {
                x.get("count")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(|| x.as_array().map_or(0, |a| a.len() as u64))
            })
            .unwrap_or(0)
    };
    let summary = json!({
        "parts_added": count(&schematic, "components_added"),
        "parts_removed": count(&schematic, "components_removed"),
        "parts_changed": count(&schematic, "components_changed"),
        "connections_changed": count(&schematic, "connections_changed"),
        "footprints_moved": count(&board, "footprints_moved"),
        "nets_rerouted": count(&board, "routing_changed"),
    });
    let mut notes = Vec::new();
    for (side, s) in [("from", &a), ("to", &b)] {
        if let Some(d) = &s.design {
            if d.hierarchical && !d.source.starts_with("kicad-cli") {
                notes.push(format!(
                    "'{side}' model covers the root sheet only (kicad-cli unavailable)"
                ));
            }
        }
    }
    Ok(CallToolResult::json(
        &json!({ "from": from, "to": to, "summary": summary, "schematic": schematic, "board": board, "notes": notes }),
    ))
}

async fn handle_restore(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let project = match project_arg(args) {
        Ok(p) => p,
        Err(e) => return Ok(e),
    };
    let id = match require_str(args, "id") {
        Ok(i) => i.to_string(),
        Err(e) => return Ok(e),
    };
    let src = project.store().join(&id);
    let Ok(meta) = std::fs::read_to_string(src.join("snapshot.json"))
        .map_err(|_| ())
        .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|_| ()))
    else {
        return Ok(invalid_arg(
            "id",
            &format!("no snapshot '{id}' (see list_snapshots)"),
        ));
    };
    if project.pcb().is_file() {
        if let Some(refusal) = crate::tools::pcb_board::refuse_if_board_open_in_kicad(
            ctx,
            &project.pcb(),
            "snapshot restore",
        )
        .await?
        {
            return Ok(refusal);
        }
    }
    let backup = take_snapshot(
        &project,
        &format!("before restore of {id}"),
        &ctx.config.kicad_cli,
    )
    .await?;
    let files: Vec<String> = meta["files"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for rel in &files {
        let dst = project.dir.join(rel);
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src.join("files").join(rel), dst)?;
    }
    let restored: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let extra: Vec<String> = project_files(&project.dir)
        .into_iter()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|p| !restored.contains(p.as_str()))
        .collect();
    Ok(CallToolResult::json(&json!({
        "restored": id,
        "files": files,
        "backup_snapshot": backup["id"],
        "files_not_in_snapshot": extra,
        "next": "Reopen the schematic and board in KiCad. Files listed in files_not_in_snapshot were left as they are.",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::design_model::{Component, Node};

    fn d(parts: &[(&str, &str)], nets: &[(&str, &[(&str, &str)])]) -> Design {
        let mut d = Design::default();
        for (r, v) in parts {
            d.components.insert(
                r.to_string(),
                Component {
                    reference: r.to_string(),
                    value: v.to_string(),
                    ..Default::default()
                },
            );
        }
        for (n, nodes) in nets {
            d.nets.insert(
                n.to_string(),
                nodes
                    .iter()
                    .map(|(r, p)| Node {
                        reference: r.to_string(),
                        pin: p.to_string(),
                        function: String::new(),
                        pin_type: String::new(),
                    })
                    .collect(),
            );
        }
        d
    }

    #[test]
    fn diff_is_name_independent_and_finds_real_changes() {
        let a = d(
            &[("R1", "10k"), ("R2", "1k"), ("U1", "MCU")],
            &[
                ("Net-(R1-Pad1)", &[("R1", "1"), ("U1", "3")]),
                ("SDA", &[("U1", "5"), ("R2", "1")]),
            ],
        );
        let b = d(
            &[("R1", "4.7k"), ("U1", "MCU"), ("C9", "100n")],
            &[
                ("Net-(U1-Pad3)", &[("R1", "1"), ("U1", "3")]),
                ("I2C_SDA", &[("U1", "5"), ("C9", "1")]),
            ],
        );
        let v = diff_designs(&a, &b);
        assert_eq!(v["components_added"][0]["reference"], "C9");
        assert_eq!(v["components_removed"][0]["reference"], "R2");
        assert_eq!(v["components_changed"][0]["changes"][0]["to"], "4.7k");
        assert!(
            v["connections_changed"].as_array().unwrap().is_empty(),
            "{v}"
        );
        assert_eq!(
            v["nets_renamed"][0],
            json!({"from": "SDA", "to": "I2C_SDA"})
        );
        let c = d(
            &[("R1", "4.7k"), ("U1", "MCU")],
            &[("Net-(R1-Pad1)", &[("R1", "1")]), ("X", &[("U1", "3")])],
        );
        let moved = diff_designs(&b, &c)["connections_changed"].clone();
        assert_eq!(moved.as_array().unwrap().len(), 2, "{moved}");
    }

    #[test]
    fn board_diff_and_snapshot_round_trip() {
        let pcb_a = r#"(kicad_pcb (net 0 "") (net 1 "A") (footprint "R:R_0402" (layer "F.Cu") (at 10 10) (property "Reference" "R1")) (segment (start 0 0) (end 3 4) (width 0.2) (layer "F.Cu") (net 1)))"#;
        let pcb_b = r#"(kicad_pcb (net 0 "") (net 1 "A") (footprint "R:R_0402" (layer "B.Cu") (at 12 10 90) (property "Reference" "R1")) (segment (start 0 0) (end 6 8) (width 0.2) (layer "F.Cu") (net 1)) (via (at 6 8) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1)))"#;
        let (a, b) = (
            board_state(&parse_sexp(pcb_a).unwrap()),
            board_state(&parse_sexp(pcb_b).unwrap()),
        );
        let v = diff_boards(&a, &b);
        assert_eq!(v["footprints_moved"][0]["moved_mm"], 2.0);
        assert_eq!(v["footprints_moved"][0]["side"]["to"], "B.Cu");
        assert_eq!(v["routing_changed"][0]["delta_mm"], 5.0);
        assert_eq!(v["routing_changed"][0]["vias"], json!([0, 1]));

        let dir = std::env::temp_dir().join(format!("kcp-hist-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("p.kicad_pro"), "{}").unwrap();
        std::fs::write(dir.join("p.kicad_pcb"), pcb_a).unwrap();
        std::fs::write(dir.join("sub/power.kicad_sch"), "(kicad_sch)").unwrap();
        std::fs::write(dir.join("notes.txt"), "x").unwrap();
        let files: Vec<String> = project_files(&dir)
            .iter()
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .collect();
        assert_eq!(
            files,
            vec!["p.kicad_pcb", "p.kicad_pro", "sub/power.kicad_sch"]
        );
        let rt = tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap();
        let p = Project::locate(dir.to_str().unwrap()).unwrap();
        let meta = rt
            .block_on(take_snapshot(&p, "before autoroute", "kicad-cli-missing"))
            .unwrap();
        assert!(meta["id"].as_str().unwrap().ends_with("-before-autoroute"));
        let again = rt
            .block_on(take_snapshot(&p, "before autoroute", "kicad-cli-missing"))
            .unwrap();
        assert_ne!(meta["id"], again["id"]);
        assert_eq!(snapshots(&p).len(), 2);
        assert!(!project_files(&dir)
            .iter()
            .any(|f| f.starts_with(".voltaris")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn utc_stamp_shape() {
        let (id, iso) = utc_stamp();
        assert_eq!(id.len(), 15);
        assert!(iso.starts_with("20") && iso.ends_with('Z'));
    }
}
