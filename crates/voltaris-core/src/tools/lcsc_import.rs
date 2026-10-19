//! `lcsc_import` toolset — bring any LCSC/JLCPCB part into the project with its
//! symbol, footprint and 3D model, using the open-source easyeda2kicad converter
//! (AGPL-3.0), then register the libraries in the project tables.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::library::{handle_register_footprint_library, handle_register_symbol_library};
use crate::tools::{invalid_arg, opt_str, opt_str_list, require_str, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Imported {
    pub lcsc: String,
    pub symbol: Option<String>,
    pub footprint: Option<String>,
    pub model: Option<String>,
}

/// Parse easyeda2kicad's `[LEVEL] message` log into per-part results and errors.
pub fn parse_log(log: &str) -> (Vec<Imported>, Vec<String>, Vec<String>) {
    let mut parts: Vec<Imported> = Vec::new();
    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    let mut current: Option<usize> = None;
    let entry = |parts: &mut Vec<Imported>, id: &str| -> usize {
        match parts.iter().position(|p| p.lcsc == id) {
            Some(i) => i,
            None => {
                parts.push(Imported {
                    lcsc: id.to_string(),
                    ..Default::default()
                });
                parts.len() - 1
            }
        }
    };
    for line in log.lines() {
        let l = line.trim();
        if let Some(msg) = l.strip_prefix("[ERROR]") {
            errors.push(msg.trim().to_string());
            continue;
        }
        if let Some(msg) = l.strip_prefix("[WARNING]") {
            warnings.push(msg.trim().to_string());
            continue;
        }
        if let Some(i) = l.find("for ID") {
            let id = l[i + 6..].trim_start_matches([' ', ':']).trim();
            let id: String = id
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric())
                .collect();
            if !id.is_empty() {
                current = Some(entry(&mut parts, &id));
            }
            continue;
        }
        let value = |key: &str| {
            l.strip_prefix(key)
                .map(|r| r.trim_start_matches([' ', ':']).trim().to_string())
        };
        if let Some(i) = current {
            if let Some(v) = value("Symbol name") {
                parts[i].symbol = Some(v);
            } else if let Some(v) = value("Footprint name") {
                parts[i].footprint = Some(v);
            } else if let Some(v) = value("3D model path (step)") {
                parts[i].model = Some(v);
            } else if parts[i].model.is_none() {
                if let Some(v) = value("3D model path (wrl)") {
                    parts[i].model = Some(v);
                }
            }
        }
    }
    (parts, errors, warnings)
}

pub fn valid_lcsc(id: &str) -> bool {
    id.len() >= 2 && id.starts_with('C') && id[1..].chars().all(|c| c.is_ascii_digit())
}

fn project_dir(project: &str) -> Option<PathBuf> {
    let p = PathBuf::from(project);
    if p.is_dir() {
        Some(p)
    } else if p.extension().is_some_and(|e| e == "kicad_pro") {
        p.parent().map(Path::to_path_buf)
    } else {
        None
    }
}

/// Candidate command lines that run easyeda2kicad, most specific first.
pub fn runners(explicit: Option<&str>) -> Vec<Vec<String>> {
    if let Some(p) = explicit {
        return vec![vec![p.to_string()]];
    }
    let mut v = Vec::new();
    let exe = if cfg!(windows) {
        "easyeda2kicad.exe"
    } else {
        "easyeda2kicad"
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if dir.join(exe).is_file() {
                v.push(vec![dir.join(exe).display().to_string()]);
                break;
            }
        }
    }
    let pythons: &[&str] = if cfg!(windows) {
        &["py", "python", "python3"]
    } else {
        &["python3", "python"]
    };
    for py in pythons {
        v.push(vec![py.to_string(), "-m".into(), "easyeda2kicad".into()]);
    }
    v
}

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "import_lcsc_part",
        "Import parts by LCSC number (the C-number shown on JLCPCB/LCSC, e.g. C2040) with symbol, \
         footprint and 3D model, using the easyeda2kicad converter, into a project-local library \
         (<project>/libraries/<library_name>.kicad_sym / .pretty / .3dshapes, 3D paths relative to \
         the project), then register both libraries in the project's tables. Returns each part's \
         lib_id and footprint for add_schematic_component. EasyEDA library data is community-made: \
         before using a part in a real design, accept it against its datasheet per the kicad-library \
         skill's physical pin-map acceptance contract. Requires easyeda2kicad on the computer \
         running this server (pip install easyeda2kicad) and internet access.",
        json!({
            "type": "object",
            "properties": {
                "lcsc_ids": { "type": "array", "items": { "type": "string" }, "minItems": 1, "maxItems": 50, "description": "LCSC part numbers, e.g. [\"C2040\", \"C6186\"]" },
                "project": { "type": "string", "description": "Path to the .kicad_pro, or its directory" },
                "library_name": { "type": "string", "default": "lcsc", "description": "Nickname and file stem of the project library" },
                "overwrite": { "type": "boolean", "default": false, "description": "Replace parts already in the library" },
                "easyeda2kicad": { "type": "string", "description": "Path to the easyeda2kicad executable, if not on PATH" },
                "timeout_s": { "type": "integer", "minimum": 10, "maximum": 900, "default": 180 }
            },
            "required": ["lcsc_ids", "project"]
        }),
        |args, ctx| async move { handle_import(args, ctx).await }
    )]
}

async fn handle_import(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let ids = match opt_str_list(args, "lcsc_ids") {
        Ok(Some(v)) if !v.is_empty() => v,
        Ok(_) => return Ok(invalid_arg("lcsc_ids", "give at least one LCSC number")),
        Err(e) => return Ok(e),
    };
    let ids: Vec<String> = ids.iter().map(|s| s.trim().to_ascii_uppercase()).collect();
    if let Some(bad) = ids.iter().find(|i| !valid_lcsc(i)) {
        return Ok(invalid_arg(
            "lcsc_ids",
            &format!("'{bad}' is not an LCSC number (C followed by digits)"),
        ));
    }
    let project = match require_str(args, "project") {
        Ok(p) => p.to_string(),
        Err(e) => return Ok(e),
    };
    let Some(dir) = project_dir(&project) else {
        return Ok(invalid_arg(
            "project",
            "pass the .kicad_pro file or the project directory",
        ));
    };
    let name = opt_str(args, "library_name").unwrap_or("lcsc").to_string();
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Ok(invalid_arg(
            "library_name",
            "use letters, digits, '_' or '-'",
        ));
    }
    let lib_dir = dir.join("libraries");
    std::fs::create_dir_all(&lib_dir)?;
    let base = lib_dir.join(&name);
    let mut tail: Vec<String> = vec!["--full".into(), "--lcsc_id".into()];
    tail.extend(ids.iter().cloned());
    tail.extend([
        "--output".into(),
        base.display().to_string(),
        "--project-relative".into(),
    ]);
    if args["overwrite"].as_bool().unwrap_or(false) {
        tail.push("--overwrite".into());
    }
    let timeout = std::time::Duration::from_secs(args["timeout_s"].as_u64().unwrap_or(180));
    let mut attempts = Vec::new();
    let mut ran: Option<(Vec<String>, std::process::Output)> = None;
    for runner in runners(opt_str(args, "easyeda2kicad")) {
        let mut cmd = tokio::process::Command::new(&runner[0]);
        cmd.args(&runner[1..])
            .args(&tail)
            .current_dir(&dir)
            .kill_on_drop(true);
        match tokio::time::timeout(timeout, cmd.output()).await {
            Err(_) => {
                return Ok(CallToolResult::error(format!(
                    "easyeda2kicad did not finish within {}s",
                    timeout.as_secs()
                )))
            }
            Ok(Err(e)) => attempts.push(format!("{}: {e}", runner.join(" "))),
            Ok(Ok(out)) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                );
                if text.contains("No module named easyeda2kicad")
                    || text.contains("No module named 'easyeda2kicad'")
                {
                    attempts.push(format!("{}: module not installed", runner.join(" ")));
                    continue;
                }
                ran = Some((runner, out));
                break;
            }
        }
    }
    let Some((runner, out)) = ran else {
        return Ok(CallToolResult::error(format!(
            "easyeda2kicad is not installed on this computer. Install it with `pip install easyeda2kicad` \
             (or pass its path in 'easyeda2kicad'), then retry. Tried: {}",
            attempts.join("; ")
        )));
    };
    let log = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let (parts, errors, warnings) = parse_log(&log);
    let sym_lib = base.with_extension("kicad_sym");
    let fp_lib = base.with_extension("pretty");
    let mut registration = Vec::new();
    let project_arg = dir.display().to_string();
    if sym_lib.is_file() {
        let r = handle_register_symbol_library(&json!({ "library_path": sym_lib.display().to_string(), "nickname": name, "scope": "project", "project": project_arg }), ctx).await?;
        registration.push(json!({ "library": "symbol", "ok": !r.is_error, "detail": r.content }));
    }
    if fp_lib.is_dir() {
        let r = handle_register_footprint_library(&json!({ "library_path": fp_lib.display().to_string(), "nickname": name, "scope": "project", "project": project_arg }), ctx).await?;
        registration
            .push(json!({ "library": "footprint", "ok": !r.is_error, "detail": r.content }));
    }
    let imported: Vec<Value> = parts
        .iter()
        .map(|p| {
            json!({
                "lcsc": p.lcsc,
                "lib_id": p.symbol.as_ref().map(|s| format!("{name}:{s}")),
                "footprint": p.footprint.as_ref().map(|f| format!("{name}:{f}")),
                "model_3d": p.model,
                "complete": p.symbol.is_some() && p.footprint.is_some(),
            })
        })
        .collect();
    let not_imported: Vec<&String> = ids
        .iter()
        .filter(|i| !parts.iter().any(|p| &p.lcsc == *i && p.symbol.is_some()))
        .collect();
    Ok(CallToolResult::json(&json!({
        "converter": runner.join(" "),
        "exit_status": out.status.code(),
        "library": { "nickname": name, "symbols": sym_lib.display().to_string(), "footprints": fp_lib.display().to_string(), "models": base.with_extension("3dshapes").display().to_string() },
        "imported": imported,
        "not_imported": not_imported,
        "errors": errors,
        "warnings": warnings,
        "registration": registration,
        "acceptance_required": "Library data from EasyEDA is community-created. For each part compare pin numbers, names and functions, footprint pad numbering and dimensions, and pin-1/orientation with the manufacturer datasheet before use, per the kicad-library skill. Confirm stock and price with get_jlcpcb_part.",
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    const LOG: &str = "[INFO] Created Kicad symbol for ID : C2040\n       Symbol name : RP2040\n       Library path : /p/libraries/lcsc.kicad_sym\n\
[INFO] Created Kicad footprint for ID: C2040\n       Footprint name: LQFN-56_L7.0-W7.0-P0.4-EP\n       Footprint path: /p/libraries/lcsc.pretty/LQFN-56.kicad_mod\n\
[INFO] Created 3D model for ID: C2040\n       3D model name: LQFN-56\n       3D model path (wrl): /p/libraries/lcsc.3dshapes/LQFN-56.wrl\n       3D model path (step): /p/libraries/lcsc.3dshapes/LQFN-56.step\n\
[ERROR] Failed to fetch data from EasyEDA API for part C999999999\n[WARNING] No 3D model available for ID: C6186\n";

    #[test]
    fn log_is_parsed_per_part() {
        let (parts, errors, warnings) = parse_log(LOG);
        let p = parts.iter().find(|p| p.lcsc == "C2040").unwrap();
        assert_eq!(p.symbol.as_deref(), Some("RP2040"));
        assert_eq!(p.footprint.as_deref(), Some("LQFN-56_L7.0-W7.0-P0.4-EP"));
        assert_eq!(
            p.model.as_deref(),
            Some("/p/libraries/lcsc.3dshapes/LQFN-56.step")
        );
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("C999999999"));
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn lcsc_numbers_are_validated() {
        assert!(valid_lcsc("C2040") && valid_lcsc("C6186"));
        assert!(
            !valid_lcsc("2040") && !valid_lcsc("C") && !valid_lcsc("C20A0") && !valid_lcsc("LM358")
        );
    }
}
