//! `signal_integrity` toolset — routed length, via count, propagation delay
//! and differential-pair / group skew, measured from the saved board.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::engineering::calc::delay_ps_per_mm;
use crate::tools::{get_path, invalid_arg, opt_f64, opt_str_list, ToolDef};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use voltaris_sexp::board::{track_arcs, tracks, vias};
use voltaris_sexp::parser::parse_sexp;

#[derive(Default, Debug, Clone)]
pub struct NetLength {
    pub length: f64,
    pub vias: usize,
    pub by_layer: BTreeMap<String, f64>,
    pub segments: usize,
}

pub fn measure(tree: &voltaris_sexp::parser::SexpNode) -> BTreeMap<String, NetLength> {
    let mut m: BTreeMap<String, NetLength> = BTreeMap::new();
    for t in tracks(tree).items {
        let Some(net) = t.net else { continue };
        let len = (t.x2 - t.x1).hypot(t.y2 - t.y1);
        let e = m.entry(net).or_default();
        e.length += len;
        e.segments += 1;
        *e.by_layer.entry(t.layer).or_default() += len;
    }
    for a in track_arcs(tree).items {
        let Some(net) = a.net.clone() else { continue };
        let len = a.length();
        let e = m.entry(net).or_default();
        e.length += len;
        e.segments += 1;
        *e.by_layer.entry(a.layer).or_default() += len;
    }
    for v in vias(tree).items {
        if let Some(net) = v.net {
            m.entry(net).or_default().vias += 1;
        }
    }
    m
}

/// Pair two net names that differ only in a polarity marker:
/// `USB_DP`/`USB_DN`, `USB_DP_MCU`/`USB_DN_MCU`, `LVDS_P`/`LVDS_N`, `USB_D+`/`USB_D-`, `CLK+`/`CLK-`.
pub fn pair_key(net: &str) -> Option<(String, bool)> {
    let name = net.trim_start_matches('/');
    if let Some(base) = name.strip_suffix('+') {
        return Some((format!("{base}±"), true));
    }
    if let Some(base) = name.strip_suffix('-') {
        return Some((format!("{base}±"), false));
    }
    let parts: Vec<&str> = name.split('_').collect();
    for (i, p) in parts.iter().enumerate().rev() {
        let upper = p.to_ascii_uppercase();
        let polarity = match upper.as_str() {
            "DP" | "P" | "D+" => Some(true),
            "DN" | "N" | "M" | "D-" => Some(false),
            _ => None,
        };
        if let Some(pos) = polarity {
            if upper == "M" && i == 0 {
                continue;
            }
            let mut key = parts.clone();
            let marker = match upper.as_str() {
                "DP" | "DN" => "D±",
                "D+" | "D-" => "D±",
                _ => "±",
            };
            key[i] = marker;
            return Some((key.join("_"), pos));
        }
    }
    None
}

pub fn auto_pairs(nets: &BTreeSet<String>) -> Vec<(String, String)> {
    let mut by_key: BTreeMap<String, (Option<String>, Option<String>)> = BTreeMap::new();
    for n in nets {
        if let Some((k, pos)) = pair_key(n) {
            let e = by_key.entry(k).or_default();
            if pos {
                e.0 = Some(n.clone());
            } else {
                e.1 = Some(n.clone());
            }
        }
    }
    by_key
        .into_values()
        .filter_map(|(p, n)| Some((p?, n?)))
        .collect()
}

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "analyze_net_lengths",
        "Measure routed copper per net from the saved board — straight segments and arcs, per-layer \
         split, via count, and propagation delay — then check differential-pair skew and \
         length-match groups. Pairs are found automatically from names that differ only by a \
         polarity marker (USB_DP/USB_DN, X_P/X_N, D+/D−, CLK+/CLK−) or given explicitly. Delay uses \
         εeff (default 3.2, typical outer-layer FR-4 microstrip; take the exact value from \
         calc_impedance). Use after routing USB, Ethernet, HDMI, DDR or clock nets; KiCad's \
         interactive length tuner then adds the meanders this reports as missing.",
        json!({
            "type": "object",
            "properties": {
                "board": { "type": "string", "description": "Path to the saved .kicad_pcb" },
                "nets": { "type": "array", "items": { "type": "string" }, "description": "Only report these nets (default: every routed net, top 40 by length)" },
                "pairs": { "type": "array", "items": { "type": "array", "items": { "type": "string" }, "minItems": 2, "maxItems": 2 }, "description": "Explicit [positive, negative] pairs; adds to auto-detected ones" },
                "pair_tolerance_mm": { "type": "number", "minimum": 0, "default": 0.15, "description": "Allowed intra-pair length difference" },
                "groups": { "type": "array", "items": { "type": "array", "items": { "type": "string" }, "minItems": 2 }, "description": "Net groups that must match each other (e.g. a parallel bus)" },
                "group_tolerance_mm": { "type": "number", "minimum": 0, "default": 2.5 },
                "max_length_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Flag nets longer than this" },
                "eeff": { "type": "number", "exclusiveMinimum": 1, "default": 3.2, "description": "Effective permittivity for delay" },
                "include_vias": { "type": "boolean", "default": true, "description": "Add one board thickness of length per via" }
            },
            "required": ["board"]
        }),
        |args, _ctx| async move { handle_lengths(args) }
    )]
}

fn handle_lengths(args: &Value) -> anyhow::Result<CallToolResult> {
    let path = get_path(args, "board")?;
    let text = std::fs::read_to_string(&path)?;
    let tree = parse_sexp(&text).map_err(|e| anyhow::anyhow!("{}: {e:?}", path.display()))?;
    let thickness = tree
        .find("general")
        .and_then(|g| g.find_f64("thickness"))
        .unwrap_or(1.6);
    let include_vias = args["include_vias"].as_bool().unwrap_or(true);
    let eeff = opt_f64(args, "eeff").unwrap_or(3.2);
    let ps_mm = delay_ps_per_mm(eeff);
    let lengths = measure(&tree);
    let total = |n: &NetLength| {
        n.length
            + if include_vias {
                n.vias as f64 * thickness
            } else {
                0.0
            }
    };
    let requested = match opt_str_list(args, "nets") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let mut report = Vec::new();
    let mut missing = Vec::new();
    let names: Vec<String> = match &requested {
        Some(list) => list.clone(),
        None => {
            let mut v: Vec<_> = lengths.iter().collect();
            v.sort_by(|a, b| total(b.1).partial_cmp(&total(a.1)).unwrap());
            v.into_iter().take(40).map(|(k, _)| k.clone()).collect()
        }
    };
    let max_len = opt_f64(args, "max_length_mm");
    let mut too_long = Vec::new();
    for n in &names {
        match lengths.get(n) {
            Some(l) => {
                let t = total(l);
                if max_len.is_some_and(|m| t > m) {
                    too_long.push(json!({ "net": n, "length_mm": r3(t), "limit_mm": max_len }));
                }
                report.push(json!({
                    "net": n, "routed_mm": r3(l.length), "vias": l.vias, "length_mm": r3(t),
                    "delay_ps": r3(t * ps_mm), "segments": l.segments,
                    "by_layer_mm": l.by_layer.iter().map(|(k, v)| (k.clone(), json!(r3(*v)))).collect::<serde_json::Map<_, _>>()
                }));
            }
            None => missing.push(n.clone()),
        }
    }
    let all: BTreeSet<String> = lengths.keys().cloned().collect();
    let mut pairs = auto_pairs(&all);
    if let Some(explicit) = args["pairs"].as_array() {
        for p in explicit {
            let (Some(a), Some(b)) = (p[0].as_str(), p[1].as_str()) else {
                return Ok(invalid_arg("pairs", "each pair is [positive, negative]"));
            };
            if !pairs.iter().any(|(x, y)| x == a && y == b) {
                pairs.push((a.into(), b.into()));
            }
        }
    }
    let tol = opt_f64(args, "pair_tolerance_mm").unwrap_or(0.15);
    let mut pair_report = Vec::new();
    let mut failures = Vec::new();
    for (p, n) in &pairs {
        let (lp, ln) = (lengths.get(p), lengths.get(n));
        match (lp, ln) {
            (Some(a), Some(b)) => {
                let (ta, tb) = (total(a), total(b));
                let skew = (ta - tb).abs();
                let ok = skew <= tol;
                if !ok {
                    failures.push(format!("{p}/{n} skew {:.3} mm exceeds {tol} mm — lengthen {} by {:.3} mm", skew, if ta < tb { p } else { n }, skew));
                }
                if a.vias != b.vias {
                    failures.push(format!("{p}/{n} via counts differ ({} vs {}) — route both lines through the same layer changes", a.vias, b.vias));
                }
                pair_report.push(json!({
                    "positive": p, "negative": n, "positive_mm": r3(ta), "negative_mm": r3(tb),
                    "skew_mm": r3(skew), "skew_ps": r3(skew * ps_mm), "vias": [a.vias, b.vias],
                    "within_tolerance": ok
                }));
            }
            _ => pair_report.push(json!({ "positive": p, "negative": n, "status": "unrouted or missing on the board" })),
        }
    }
    let gtol = opt_f64(args, "group_tolerance_mm").unwrap_or(2.5);
    let mut group_report = Vec::new();
    if let Some(groups) = args["groups"].as_array() {
        for g in groups {
            let nets: Vec<String> = g
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let ls: Vec<(String, f64)> = nets
                .iter()
                .filter_map(|n| lengths.get(n).map(|l| (n.clone(), total(l))))
                .collect();
            if ls.len() < nets.len() {
                group_report.push(json!({ "nets": nets, "status": "some nets are unrouted" }));
                continue;
            }
            let (min, max) = ls
                .iter()
                .fold((f64::MAX, f64::MIN), |m, x| (m.0.min(x.1), m.1.max(x.1)));
            let ok = max - min <= gtol;
            if !ok {
                failures.push(format!(
                    "group {:?} spread {:.3} mm exceeds {gtol} mm",
                    nets,
                    max - min
                ));
            }
            group_report.push(json!({ "nets": nets, "min_mm": r3(min), "max_mm": r3(max), "spread_mm": r3(max - min), "spread_ps": r3((max - min) * ps_mm), "within_tolerance": ok }));
        }
    }
    Ok(CallToolResult::json(&json!({
        "basis": { "eeff": eeff, "delay_ps_per_mm": r3(ps_mm), "via_length_mm": if include_vias { json!(thickness) } else { json!(0) } },
        "nets": report,
        "missing_or_unrouted": missing,
        "pairs": pair_report,
        "groups": group_report,
        "too_long": too_long,
        "failures": failures,
        "verdict": if failures.is_empty() && too_long.is_empty() { "pass" } else { "fail" },
        "note": "Lengths come from the saved file: save the board in KiCad first. Pad-to-pad length inside footprints (package pin length) is not included."
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polarity_pairs_are_recognised() {
        let nets: BTreeSet<String> = [
            "USB_DP",
            "USB_DN",
            "USB_DP_MCU",
            "USB_DN_MCU",
            "LVDS_P",
            "LVDS_N",
            "CLK+",
            "CLK-",
            "/eth/TX_P",
            "/eth/TX_N",
            "SDA",
            "MISO",
            "MOSI",
            "N_RESET",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pairs = auto_pairs(&nets);
        for want in [
            ("USB_DP", "USB_DN"),
            ("USB_DP_MCU", "USB_DN_MCU"),
            ("LVDS_P", "LVDS_N"),
            ("CLK+", "CLK-"),
            ("/eth/TX_P", "/eth/TX_N"),
        ] {
            assert!(
                pairs.contains(&(want.0.into(), want.1.into())),
                "missing {want:?} in {pairs:?}"
            );
        }
        assert_eq!(pairs.len(), 5, "{pairs:?}");
    }

    #[test]
    fn lengths_and_skew_from_board() {
        let board = r#"(kicad_pcb (general (thickness 1.6))
          (net 0 "") (net 1 "USB_DP") (net 2 "USB_DN")
          (segment (start 0 0) (end 10 0) (width 0.2) (layer "F.Cu") (net 1))
          (segment (start 10 0) (end 10 5) (width 0.2) (layer "F.Cu") (net 1))
          (segment (start 0 0.4) (end 12 0.4) (width 0.2) (layer "F.Cu") (net 2))
          (arc (start 12 0.4) (mid 13.4142 0.9858) (end 14 2.4) (width 0.2) (layer "F.Cu") (net 2))
          (via (at 10 5) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 1)))"#;
        let tree = parse_sexp(board).unwrap();
        let m = measure(&tree);
        assert!((m["USB_DP"].length - 15.0).abs() < 1e-9);
        assert_eq!(m["USB_DP"].vias, 1);
        let quarter = std::f64::consts::PI; // radius 2 quarter circle = π
        assert!(
            (m["USB_DN"].length - (12.0 + quarter)).abs() < 1e-3,
            "{}",
            m["USB_DN"].length
        );
    }
}
