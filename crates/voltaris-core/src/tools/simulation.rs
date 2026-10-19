//! `simulation` toolset — check circuits numerically before they are built.
//!
//! `simulate_circuit` runs a built-in linear solver (no installation needed) on a
//! SPICE netlist of R, C, L, independent sources and controlled sources.
//! `run_ngspice` hands any netlist — including diodes, transistors and vendor
//! models — to an installed ngspice in batch mode.

pub mod mna;
pub mod parse;

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{invalid_arg, opt_str, opt_str_list, ToolDef};
use mna::{Circuit, Cx};
use parse::Analysis;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

fn sig(x: f64) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let p = 5 - 1 - x.abs().log10().floor() as i32;
    let m = 10f64.powi(p);
    (x * m).round() / m
}

fn db(x: f64) -> f64 {
    20.0 * x.max(1e-300).log10()
}

pub fn ac_frequencies(sweep: &str, points: usize, f1: f64, f2: f64) -> Vec<f64> {
    let points = points.max(1);
    match sweep {
        "lin" => (0..points)
            .map(|i| {
                if points == 1 {
                    f1
                } else {
                    f1 + (f2 - f1) * i as f64 / (points - 1) as f64
                }
            })
            .collect(),
        _ => {
            let per = if sweep == "oct" {
                points as f64 / 2f64.log10()
            } else {
                points as f64
            };
            let n = ((f2 / f1).log10() * per).ceil() as usize;
            (0..=n)
                .map(|i| f1 * 10f64.powf(i as f64 / per))
                .filter(|f| *f <= f2 * (1.0 + 1e-9))
                .collect()
        }
    }
}

fn probe_nodes(c: &Circuit, probes: &Option<Vec<String>>) -> Result<Vec<usize>, String> {
    match probes {
        Some(list) => list
            .iter()
            .map(|p| {
                let name = p
                    .trim()
                    .trim_start_matches("V(")
                    .trim_start_matches("v(")
                    .trim_end_matches(')');
                c.nodes
                    .iter()
                    .position(|n| n.eq_ignore_ascii_case(name))
                    .filter(|i| *i > 0)
                    .ok_or(format!("no node named '{name}'"))
            })
            .collect(),
        None => Ok((1..c.nodes.len()).collect()),
    }
}

fn decimate<T: Clone>(v: &[T], max: usize) -> Vec<T> {
    if v.len() <= max {
        return v.to_vec();
    }
    let step = (v.len() - 1) as f64 / (max - 1) as f64;
    (0..max)
        .map(|i| v[((i as f64) * step).round() as usize].clone())
        .collect()
}

pub fn run_analysis(
    parsed: &parse::Parsed,
    a: &Analysis,
    probes: &[usize],
    csv: Option<&Path>,
) -> Result<Value, String> {
    let c = &parsed.circuit;
    let name = |n: usize| c.nodes[n].clone();
    match a {
        Analysis::Op => {
            let s = c.dc().map_err(|e| e.0)?;
            let mut nodes = serde_json::Map::new();
            for n in 1..c.nodes.len() {
                nodes.insert(name(n), json!(sig(c.node_v(&s.x, n).re)));
            }
            let mut elements = Vec::new();
            for (i, e) in c.elements.iter().enumerate() {
                let v = c.node_v(&s.x, e.p).re - c.node_v(&s.x, e.n).re;
                let current = match &e.kind {
                    mna::Kind::R(r) => Some(v / r),
                    mna::Kind::I(src) => Some(src.t0()),
                    _ => c.branch_current(&s.x, i).map(|x| x.re),
                };
                if let Some(cur) = current {
                    elements.push(json!({ "element": e.name, "voltage": sig(v), "current": sig(cur), "power_w": sig(v * cur) }));
                }
            }
            Ok(json!({ "analysis": "op", "node_voltages": nodes, "elements": elements }))
        }
        Analysis::Ac {
            sweep,
            points,
            fstart,
            fstop,
        } => {
            let freqs = ac_frequencies(sweep, *points, *fstart, *fstop);
            if freqs.len() > 20_000 {
                return Err("AC sweep exceeds 20000 points".into());
            }
            let mut rows: Vec<(f64, Vec<Cx>)> = Vec::new();
            for f in &freqs {
                let s = c.ac(*f).map_err(|e| e.0)?;
                rows.push((*f, probes.iter().map(|n| c.node_v(&s.x, *n)).collect()));
            }
            if let Some(path) = csv {
                let mut text = String::from("freq_hz");
                for n in probes {
                    text.push_str(&format!(",{0}_db,{0}_deg", name(*n)));
                }
                text.push('\n');
                for (f, v) in &rows {
                    text.push_str(&format!("{f:e}"));
                    for x in v {
                        text.push_str(&format!(",{:.6},{:.4}", db(x.abs()), x.arg_deg()));
                    }
                    text.push('\n');
                }
                std::fs::write(path, text).map_err(|e| e.to_string())?;
            }
            let mut summary = Vec::new();
            for (k, n) in probes.iter().enumerate() {
                let mags: Vec<f64> = rows.iter().map(|r| db(r.1[k].abs())).collect();
                let (imax, max) = mags
                    .iter()
                    .copied()
                    .enumerate()
                    .fold((0, f64::MIN), |m, x| if x.1 > m.1 { x } else { m });
                let first = mags[0];
                let cross = |level: f64, from: usize| -> Option<f64> {
                    (from + 1..mags.len())
                        .find(|&i| mags[i] < level && mags[i - 1] >= level)
                        .map(|i| {
                            let (f0, f1, m0, m1) =
                                (rows[i - 1].0.ln(), rows[i].0.ln(), mags[i - 1], mags[i]);
                            (f0 + (level - m0) * (f1 - f0) / (m1 - m0)).exp()
                        })
                };
                let low_ref = cross(first - 3.0, 0);
                let peak_ref = cross(max - 3.0, imax);
                let rise_ref = (1..mags.len())
                    .find(|&i| mags[i] >= max - 3.0 && mags[i - 1] < max - 3.0)
                    .map(|i| rows[i].0);
                summary.push(json!({
                    "node": name(*n),
                    "gain_at_fstart_db": sig(first),
                    "gain_at_fstop_db": sig(*mags.last().unwrap()),
                    "phase_at_fstart_deg": sig(rows[0].1[k].arg_deg()),
                    "peak_db": sig(max), "peak_freq_hz": sig(rows[imax].0),
                    "minus3db_below_start_hz": low_ref.map(sig),
                    "minus3db_below_peak_hz": peak_ref.map(sig),
                    "lower_minus3db_of_peak_hz": rise_ref.map(sig),
                    "peaking_db": sig(max - first),
                }));
            }
            let table: Vec<Value> = decimate(&rows, 40)
                .iter()
                .map(|(f, v)| {
                    let mut row = serde_json::Map::new();
                    row.insert("freq_hz".into(), json!(sig(*f)));
                    for (k, n) in probes.iter().enumerate() {
                        row.insert(format!("{}_db", name(*n)), json!(sig(db(v[k].abs()))));
                        row.insert(format!("{}_deg", name(*n)), json!(sig(v[k].arg_deg())));
                    }
                    Value::Object(row)
                })
                .collect();
            Ok(
                json!({ "analysis": "ac", "points": rows.len(), "summary": summary, "table": table }),
            )
        }
        Analysis::Tran { tstep, tstop, uic } => {
            if *tstep <= 0.0 || *tstop <= 0.0 || tstop / tstep > 2_000_000.0 {
                return Err(".tran needs 0 < tstep and at most 2,000,000 steps".into());
            }
            let (t, rows) = c.tran(*tstop, *tstep, *uic, &parsed.ic).map_err(|e| e.0)?;
            if let Some(path) = csv {
                let mut text = String::from("time_s");
                for n in probes {
                    text.push_str(&format!(",{}", name(*n)));
                }
                text.push('\n');
                for (i, ti) in t.iter().enumerate() {
                    text.push_str(&format!("{ti:e}"));
                    for n in probes {
                        text.push_str(&format!(",{:.9}", rows[i][n - 1]));
                    }
                    text.push('\n');
                }
                std::fs::write(path, text).map_err(|e| e.to_string())?;
            }
            let mut summary = Vec::new();
            for n in probes {
                let v: Vec<f64> = rows.iter().map(|r| r[n - 1]).collect();
                let (v0, vf) = (v[0], *v.last().unwrap());
                let tail = &v[(v.len() as f64 * 0.8) as usize..];
                let (tmin, tmax) = tail
                    .iter()
                    .fold((f64::MAX, f64::MIN), |m, x| (m.0.min(*x), m.1.max(*x)));
                let (gmin, gmax) = v
                    .iter()
                    .fold((f64::MAX, f64::MIN), |m, x| (m.0.min(*x), m.1.max(*x)));
                let delta = vf - v0;
                let reach = |frac: f64| -> Option<f64> {
                    if delta.abs() < 1e-12 {
                        return None;
                    }
                    let target = v0 + frac * delta;
                    v.iter()
                        .position(|x| {
                            if delta > 0.0 {
                                *x >= target
                            } else {
                                *x <= target
                            }
                        })
                        .map(|i| t[i])
                };
                let overshoot = if delta.abs() > 1e-12 {
                    let peak = if delta > 0.0 { gmax } else { gmin };
                    ((peak - vf) / delta * 100.0).max(0.0)
                } else {
                    0.0
                };
                summary.push(json!({
                    "node": name(*n), "initial": sig(v0), "final": sig(vf), "min": sig(gmin), "max": sig(gmax),
                    "ripple_pp_last_20pct": sig(tmax - tmin),
                    "t_10pct": reach(0.1).map(sig), "t_63pct": reach(0.632).map(sig), "t_90pct": reach(0.9).map(sig),
                    "overshoot_pct": sig(overshoot),
                }));
            }
            let idx: Vec<usize> = decimate(&(0..t.len()).collect::<Vec<_>>(), 40);
            let table: Vec<Value> = idx
                .iter()
                .map(|&i| {
                    let mut row = serde_json::Map::new();
                    row.insert("time_s".into(), json!(sig(t[i])));
                    for n in probes {
                        row.insert(name(*n), json!(sig(rows[i][n - 1])));
                    }
                    Value::Object(row)
                })
                .collect();
            Ok(
                json!({ "analysis": "tran", "steps": t.len() - 1, "method": "trapezoidal, fixed step = tstep", "summary": summary, "table": table }),
            )
        }
    }
}

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "simulate_circuit",
            "Simulate a linear circuit with the built-in solver — nothing to install. Takes a SPICE \
             netlist using R, C, L, V, I, E (VCVS) and G (VCCS) elements with SI suffixes (10k, 4.7u, \
             1meg; as in SPICE 'm' is milli), sources with DC, AC, PULSE, SIN or PWL, and analyses \
             .op, .ac dec|oct|lin N fstart fstop, .tran tstep tstop [uic], plus .ic. Returns node \
             voltages and element currents/power (op); gain/phase with −3 dB points and peaking (ac); \
             final value, ripple, rise times and overshoot (tran); a decimated table and optionally \
             the full data as CSV. Use for filters, dividers, RC/LC timing, decoupling and output-filter \
             checks, ideal op-amp stages. Diodes, transistors and vendor models need run_ngspice.",
            json!({
                "type": "object",
                "properties": {
                    "netlist": { "type": "string", "description": "SPICE netlist text, one element or directive per line" },
                    "probes": { "type": "array", "items": { "type": "string" }, "description": "Node names to report (default: every node)" },
                    "csv_dir": { "type": "string", "description": "Write full results as <analysis>.csv into this directory" }
                },
                "required": ["netlist"]
            }),
            |args, _ctx| async move { handle_simulate(args) }
        ),
        tool!(
            "run_ngspice",
            "Run a netlist through an installed ngspice in batch mode — for anything nonlinear: \
             diodes, BJTs, MOSFETs, switches, behavioural sources, vendor .model/.subckt files. Give \
             the netlist as text or as a file (export one from the schematic with generate_netlist \
             using format 'spice', after assigning simulation models). 'control' lines run inside a \
             .control block (default: run, then print the requested vectors). Returns .meas results, \
             printed values and the tail of ngspice's log. Finds ngspice via the 'ngspice' argument, \
             the NGSPICE environment variable, PATH (ngspice / ngspice_con) or the usual Windows \
             install folders.",
            json!({
                "type": "object",
                "properties": {
                    "netlist": { "type": "string", "description": "Netlist text (mutually exclusive with netlist_file)" },
                    "netlist_file": { "type": "string", "description": "Path to a .cir/.spice/.net netlist" },
                    "control": { "type": "array", "items": { "type": "string" }, "description": "ngspice commands, e.g. [\"tran 1u 5m\", \"meas tran vavg AVG v(out) from=4m to=5m\", \"print v(out)\"]" },
                    "ngspice": { "type": "string", "description": "Path to the ngspice executable (ngspice_con.exe on Windows)" },
                    "timeout_s": { "type": "integer", "minimum": 1, "maximum": 600, "default": 120 }
                }
            }),
            |args, _ctx| async move { handle_ngspice(args).await }
        ),
    ]
}

fn handle_simulate(args: &Value) -> anyhow::Result<CallToolResult> {
    let Some(text) = opt_str(args, "netlist") else {
        return Ok(invalid_arg("netlist", "required"));
    };
    let parsed = match parse::parse(text) {
        Ok(p) => p,
        Err(e) => return Ok(CallToolResult::error(format!("Netlist error: {e}"))),
    };
    if parsed.analyses.is_empty() {
        return Ok(invalid_arg(
            "netlist",
            "add at least one analysis: .op, .ac or .tran",
        ));
    }
    let probes = match opt_str_list(args, "probes") {
        Ok(p) => p,
        Err(e) => return Ok(e),
    };
    let probe_idx = match probe_nodes(&parsed.circuit, &probes) {
        Ok(p) => p,
        Err(e) => return Ok(invalid_arg("probes", &e)),
    };
    let mut warnings = parsed.warnings.clone();
    let dc_floating = parsed.circuit.floating_nodes(true);
    if !dc_floating.is_empty() {
        warnings.push(format!(
            "No DC path to ground for {} (only capacitors/current sources): their DC level is set by a 1e-12 S leak, so treat .op values there as undefined",
            dc_floating.join(", ")
        ));
    }
    let csv_dir = opt_str(args, "csv_dir").map(PathBuf::from);
    if let Some(d) = &csv_dir {
        std::fs::create_dir_all(d)?;
    }
    let mut results = Vec::new();
    for a in &parsed.analyses {
        let file = csv_dir.as_ref().map(|d| {
            d.join(match a {
                Analysis::Op => "op.csv",
                Analysis::Ac { .. } => "ac.csv",
                Analysis::Tran { .. } => "tran.csv",
            })
        });
        let file = if matches!(a, Analysis::Op) {
            None
        } else {
            file
        };
        match run_analysis(&parsed, a, &probe_idx, file.as_deref()) {
            Ok(mut v) => {
                if let Some(f) = &file {
                    v["csv"] = json!(f.display().to_string());
                }
                results.push(v)
            }
            Err(e) => return Ok(CallToolResult::error(format!("Simulation failed: {e}"))),
        }
    }
    Ok(CallToolResult::json(&json!({
        "solver": "built-in linear MNA (Voltaris)",
        "nodes": parsed.circuit.nodes.iter().skip(1).collect::<Vec<_>>(),
        "elements": parsed.circuit.elements.len(),
        "results": results,
        "warnings": warnings,
        "limits": "Ideal linear elements only: no parasitics, saturation, ESR or DC-bias derating unless you model them as R/L/C."
    })))
}

pub fn find_ngspice(explicit: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = explicit {
        return Some(PathBuf::from(p)).filter(|p| p.is_file());
    }
    if let Ok(p) = std::env::var("NGSPICE") {
        let p = PathBuf::from(p);
        if p.is_file() {
            return Some(p);
        }
    }
    let names: &[&str] = if cfg!(windows) {
        &["ngspice_con.exe", "ngspice.exe"]
    } else {
        &["ngspice"]
    };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            for n in names {
                let c = dir.join(n);
                if c.is_file() {
                    return Some(c);
                }
            }
        }
    }
    let common: &[&str] = if cfg!(windows) {
        &[
            r"C:\Spice64\bin\ngspice_con.exe",
            r"C:\Program Files\Spice64\bin\ngspice_con.exe",
            r"C:\Spice\bin\ngspice_con.exe",
        ]
    } else {
        &[
            "/usr/bin/ngspice",
            "/usr/local/bin/ngspice",
            "/opt/homebrew/bin/ngspice",
        ]
    };
    common.iter().map(PathBuf::from).find(|p| p.is_file())
}

/// Build the batch deck: the user's circuit without its .end, then a control block.
pub fn batch_deck(netlist: &str, control: &[String]) -> String {
    let mut body: Vec<&str> = netlist
        .lines()
        .filter(|l| !l.trim().eq_ignore_ascii_case(".end"))
        .collect();
    let has_title = body
        .first()
        .is_some_and(|l| l.trim_start().starts_with(".title") || l.trim_start().starts_with('*'));
    let mut deck = String::new();
    if !has_title {
        deck.push_str("* Voltaris batch deck\n");
    }
    for l in body.drain(..) {
        deck.push_str(l);
        deck.push('\n');
    }
    deck.push_str(".control\nset noaskquit\n");
    if control.is_empty() {
        deck.push_str("run\n");
    }
    for c in control {
        deck.push_str(c);
        deck.push('\n');
    }
    deck.push_str("quit\n.endc\n.end\n");
    deck
}

/// Parse `name = value` measure lines and `index time v(out)` print tables.
pub fn parse_ngspice_output(out: &str) -> Value {
    let mut meas = serde_json::Map::new();
    let mut printed = Vec::new();
    for line in out.lines() {
        let l = line.trim();
        if let Some((k, v)) = l.split_once('=') {
            let key = k.trim();
            let val = v
                .split_whitespace()
                .next()
                .and_then(|x| x.parse::<f64>().ok());
            if let Some(val) = val {
                if !key.is_empty()
                    && key.chars().all(|c| {
                        c.is_ascii_alphanumeric() || c == '_' || c == '(' || c == ')' || c == '#'
                    })
                {
                    meas.insert(key.to_string(), json!(val));
                }
            }
        }
        let cols: Vec<&str> = l.split_whitespace().collect();
        if cols.len() >= 2
            && cols[0].parse::<usize>().is_ok()
            && cols[1..].iter().all(|c| c.parse::<f64>().is_ok())
        {
            printed.push(json!(cols[1..]
                .iter()
                .map(|c| c.parse::<f64>().unwrap())
                .collect::<Vec<_>>()));
        }
    }
    json!({ "measures": meas, "printed_rows": printed.len(), "printed_sample": decimate(&printed, 30) })
}

async fn handle_ngspice(args: &Value) -> anyhow::Result<CallToolResult> {
    let netlist = match (opt_str(args, "netlist"), opt_str(args, "netlist_file")) {
        (Some(t), None) => t.to_string(),
        (None, Some(p)) => match std::fs::read_to_string(p) {
            Ok(t) => t,
            Err(e) => {
                return Ok(invalid_arg(
                    "netlist_file",
                    &format!("cannot read {p}: {e}"),
                ))
            }
        },
        _ => {
            return Ok(invalid_arg(
                "netlist",
                "pass exactly one of netlist or netlist_file",
            ))
        }
    };
    let Some(exe) = find_ngspice(opt_str(args, "ngspice")) else {
        return Ok(CallToolResult::error(
            "ngspice was not found. Install it from https://ngspice.sourceforge.io/download.html \
             (Windows: unzip to C:\\Spice64), or pass its path in 'ngspice'. For linear circuits \
             simulate_circuit needs no installation.",
        ));
    };
    let control = match opt_str_list(args, "control") {
        Ok(c) => c.unwrap_or_default(),
        Err(e) => return Ok(e),
    };
    let deck = batch_deck(&netlist, &control);
    let dir = std::env::temp_dir().join(format!("kcp-ngspice-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    let deck_path = dir.join("deck.cir");
    std::fs::write(&deck_path, &deck)?;
    let timeout = std::time::Duration::from_secs(args["timeout_s"].as_u64().unwrap_or(120));
    let run = tokio::process::Command::new(&exe)
        .arg("-b")
        .arg(&deck_path)
        .current_dir(&dir)
        .kill_on_drop(true)
        .output();
    let output = match tokio::time::timeout(timeout, run).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) => {
            return Ok(CallToolResult::error(format!(
                "could not start {}: {e}",
                exe.display()
            )))
        }
        Err(_) => {
            return Ok(CallToolResult::error(format!(
                "ngspice did not finish within {}s",
                timeout.as_secs()
            )))
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
    let _ = std::fs::remove_dir_all(&dir);
    let errors: Vec<&str> = stdout
        .lines()
        .chain(stderr.lines())
        .filter(|l| l.to_ascii_lowercase().contains("error"))
        .take(20)
        .collect();
    let tail: String = {
        let all: Vec<&str> = stdout.lines().collect();
        all[all.len().saturating_sub(60)..].join("\n")
    };
    Ok(CallToolResult::json(&json!({
        "ngspice": exe.display().to_string(),
        "exit_status": output.status.code(),
        "completed": output.status.success() && errors.is_empty(),
        "errors": errors,
        "results": parse_ngspice_output(&stdout),
        "log_tail": tail,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::PI;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn spice_numbers() {
        assert_eq!(parse::number("10k"), Some(10e3));
        assert!(close(parse::number("4.7u").unwrap(), 4.7e-6, 1e-18));
        assert_eq!(parse::number("1meg"), Some(1e6));
        assert!(close(parse::number("100nF").unwrap(), 100e-9, 1e-20));
        assert!(close(parse::number("2m").unwrap(), 2e-3, 1e-15));
        assert_eq!(parse::number("2.2e-6"), Some(2.2e-6));
        assert_eq!(parse::number("-5"), Some(-5.0));
        assert_eq!(parse::number("abc"), None);
    }

    #[test]
    fn divider_operating_point() {
        let p = parse::parse("V1 in 0 DC 12\nR1 in out 30k\nR2 out 0 10k\n.op").unwrap();
        let s = p.circuit.dc().unwrap();
        let out = p.circuit.nodes.iter().position(|n| n == "out").unwrap();
        assert!(close(p.circuit.node_v(&s.x, out).re, 3.0, 1e-6));
    }

    #[test]
    fn rc_lowpass_is_minus_3db_at_fc() {
        let (r, c) = (1e3, 159.154943e-9);
        let fc = 1.0 / (2.0 * PI * r * c);
        let p = parse::parse(&format!(
            "V1 in 0 AC 1\nR1 in out {r}\nC1 out 0 {c}\n.ac dec 10 10 1meg"
        ))
        .unwrap();
        let s = p.circuit.ac(fc).unwrap();
        let out = p.circuit.nodes.iter().position(|n| n == "out").unwrap();
        let v = p.circuit.node_v(&s.x, out);
        assert!(close(db(v.abs()), -3.0103, 1e-3), "{}", db(v.abs()));
        assert!(close(v.arg_deg(), -45.0, 1e-3));
    }

    #[test]
    fn rc_step_reaches_63pct_at_tau() {
        let p =
            parse::parse("V1 in 0 PULSE(0 1 0 1n 1n 1 2)\nR1 in out 1k\nC1 out 0 1u\n.tran 1u 5m")
                .unwrap();
        let (t, rows) = p.circuit.tran(5e-3, 1e-6, false, &p.ic).unwrap();
        let out = p.circuit.nodes.iter().position(|n| n == "out").unwrap() - 1;
        let i = t.iter().position(|x| (*x - 1e-3).abs() < 1e-9).unwrap();
        assert!(
            close(rows[i][out], 1.0 - (-1.0f64).exp(), 2e-3),
            "{}",
            rows[i][out]
        );
    }

    #[test]
    fn series_rlc_resonance() {
        let (l, c): (f64, f64) = (10e-6, 100e-9);
        let f0 = 1.0 / (2.0 * PI * (l * c).sqrt());
        let p = parse::parse(&format!(
            "V1 in 0 AC 1\nL1 in a {l}\nC1 a b {c}\nR1 b 0 10\n.ac dec 50 1k 10meg"
        ))
        .unwrap();
        let s = p.circuit.ac(f0).unwrap();
        let b = p.circuit.nodes.iter().position(|n| n == "b").unwrap();
        assert!(close(p.circuit.node_v(&s.x, b).abs(), 1.0, 1e-6));
    }

    #[test]
    fn ideal_noninverting_amplifier() {
        let p = parse::parse("V1 in 0 DC 0.5\nE1 out 0 in fb 1e6\nR1 out fb 9k\nR2 fb 0 1k\n.op")
            .unwrap();
        let s = p.circuit.dc().unwrap();
        let out = p.circuit.nodes.iter().position(|n| n == "out").unwrap();
        assert!(close(p.circuit.node_v(&s.x, out).re, 5.0, 1e-4));
    }

    #[test]
    fn rlc_step_matches_scipy_reference() {
        // Series R-L-C low-pass, output across C: R=2Ω, L=10µH, C=10µF, 1 V step.
        // Reference from scipy.signal.step for H(s) = 1/(LCs² + RCs + 1), see tests/data/simulation_reference.py.
        let p = parse::parse(
            "V1 in 0 PULSE(0 1 0 1p 1p 1 2)\nR1 in a 2\nL1 a b 10u\nC1 b 0 10u\n.tran 10n 200u",
        )
        .unwrap();
        let (t, rows) = p.circuit.tran(200e-6, 10e-9, false, &p.ic).unwrap();
        let b = p.circuit.nodes.iter().position(|n| n == "b").unwrap() - 1;
        for (tr, vr) in REF_RLC_STEP {
            let i = t.iter().position(|x| (*x - tr).abs() < 5e-12).unwrap();
            assert!(
                close(rows[i][b], *vr, 2e-3),
                "t={tr}: {} vs {vr}",
                rows[i][b]
            );
        }
    }

    #[test]
    fn floating_node_is_reported() {
        let p = parse::parse("V1 in 0 1\nR1 a b 1k\n.op").unwrap();
        let err = p.circuit.dc().err().unwrap().0;
        assert!(err.contains("floating") || err.contains("singular"));
    }

    #[test]
    fn nonlinear_elements_are_routed_to_ngspice() {
        let e = parse::parse("V1 in 0 5\nD1 in out D1N4148\nR1 out 0 1k\n.op").unwrap_err();
        assert!(e.contains("run_ngspice"));
    }

    #[test]
    fn ngspice_deck_and_output_parsing() {
        let deck = batch_deck(
            "V1 in 0 1\nR1 in 0 1k\n.end\n",
            &["op".into(), "print v(in)".into()],
        );
        assert!(deck.contains(".control\nset noaskquit\nop\nprint v(in)\nquit\n.endc\n.end"));
        assert_eq!(deck.matches(".end\n").count(), 1);
        let out = parse_ngspice_output("vavg = 3.29871e+00 from= 4.0e-3 to= 5.0e-3\nIndex time v(out)\n0 0.000000e+00 0.0\n1 1.0e-06 0.5\n");
        assert_eq!(out["measures"]["vavg"], json!(3.29871));
        assert_eq!(out["printed_rows"], json!(2));
    }

    #[test]
    fn ac_sweep_points_follow_spice_semantics() {
        let f = ac_frequencies("dec", 10, 10.0, 1000.0);
        assert_eq!(f.len(), 21);
        assert!(close(f[10], 100.0, 1e-9));
        assert_eq!(
            ac_frequencies("lin", 5, 1.0, 5.0),
            vec![1.0, 2.0, 3.0, 4.0, 5.0]
        );
    }

    /// Every netlist in the kicad-simulation skill's recipes must run as written.
    #[test]
    fn skill_recipes_simulate_cleanly() {
        let md =
            include_str!("../../../voltaris/assets/skills/kicad-simulation/references/recipes.md");
        let blocks: Vec<&str> = md.split("```").skip(1).step_by(2).collect();
        assert!(blocks.len() >= 5, "expected the five recipes");
        for b in blocks {
            let parsed =
                parse::parse(b).unwrap_or_else(|e| panic!("recipe does not parse: {e}\n{b}"));
            assert!(parsed.warnings.is_empty(), "{:?}", parsed.warnings);
            let probes: Vec<usize> = (1..parsed.circuit.nodes.len()).collect();
            for a in &parsed.analyses {
                run_analysis(&parsed, a, &probes, None)
                    .unwrap_or_else(|e| panic!("recipe failed: {e}\n{b}"));
            }
        }
    }

    const REF_RLC_STEP: &[(f64, f64)] = &[
        (5.000000e-06, 0.090204),
        (1.000000e-05, 0.264241),
        (2.000000e-05, 0.593994),
        (3.140000e-05, 0.820809),
        (5.000000e-05, 0.959572),
        (8.000000e-05, 0.996981),
        (1.500000e-04, 0.999995),
    ];
}
