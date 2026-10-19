//! Layout audit: engineering and EMC findings DRC does not report (return
//! paths, fast signals near edges, decoupling distance, power copper, sharp
//! corners, vias in pads) and a beauty score for the finished look.

use super::model::{
    dist, is_fast, is_ground, is_unnamed, rect_distance, segment_distance, short_net, Board, Rect,
};
use crate::tools::design_model::is_power_net;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone)]
pub struct AuditOptions {
    pub min_power_width: f64,
    pub decoupling_mm: f64,
    pub edge_mm: f64,
    pub return_via_mm: f64,
}

impl Default for AuditOptions {
    fn default() -> Self {
        Self {
            min_power_width: 0.4,
            decoupling_mm: 3.0,
            edge_mm: 1.0,
            return_via_mm: 1.0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Finding {
    pub severity: &'static str,
    pub area: &'static str,
    pub check: &'static str,
    pub message: String,
    pub at: Option<(f64, f64)>,
    pub fix: &'static str,
}

#[derive(Debug, Clone)]
pub struct Audit {
    pub engineering: i64,
    pub beauty: i64,
    pub findings: Vec<Finding>,
    pub metrics: Value,
}

struct Ledger {
    caps: BTreeMap<&'static str, (i64, i64)>,
}

impl Ledger {
    fn take(&mut self, check: &'static str, each: i64, cap: i64) {
        let e = self.caps.entry(check).or_insert((0, cap));
        e.0 = (e.0 + each).min(e.1);
    }
    fn total(&self) -> i64 {
        self.caps.values().map(|v| v.0).sum()
    }
}

/// Track ends meeting at one point: (net, layer, rounded point).
type EndKey = (String, String, (i64, i64));
/// A track end and the far end of the same track.
type Spoke = ((f64, f64), (f64, f64));

/// Folds more than three findings of one check into a single line with a
/// count and examples, so one systematic issue does not bury the others.
fn group_findings(findings: Vec<Finding>) -> Vec<Finding> {
    let mut order: Vec<(&'static str, &'static str)> = Vec::new();
    let mut by: BTreeMap<(&'static str, &'static str), Vec<Finding>> = BTreeMap::new();
    for f in findings {
        let k = (f.area, f.check);
        if !by.contains_key(&k) {
            order.push(k);
        }
        by.entry(k).or_default().push(f);
    }
    let mut out = Vec::new();
    for k in order {
        let list = by.remove(&k).unwrap_or_default();
        if list.len() <= 3 {
            out.extend(list);
            continue;
        }
        let first = &list[0];
        let examples: Vec<&str> = list.iter().take(3).map(|f| f.message.as_str()).collect();
        out.push(Finding {
            severity: if list.iter().any(|f| f.severity == "error") {
                "error"
            } else if list.iter().any(|f| f.severity == "warning") {
                "warning"
            } else {
                "info"
            },
            area: first.area,
            check: first.check,
            message: format!("{} cases, e.g. {}", list.len(), examples.join("; ")),
            at: first.at,
            fix: first.fix,
        });
    }
    out
}

/// Copper layers in stack order (F.Cu, In1.Cu, In2.Cu, …, B.Cu); KiCad 9+
/// files list B.Cu second.
fn stack_order(layers: &[String]) -> Vec<String> {
    let rank = |l: &str| match l {
        "F.Cu" => 0,
        "B.Cu" => 1000,
        _ => l
            .trim_start_matches("In")
            .trim_end_matches(".Cu")
            .parse::<i32>()
            .unwrap_or(500),
    };
    let mut out = layers.to_vec();
    out.sort_by_key(|l| rank(l));
    out
}

#[derive(Debug, PartialEq, Eq)]
enum Reference {
    /// Every layer the signal uses sits next to one shared ground plane
    /// (F.Cu → In2.Cu over an In1.Cu ground plane): no ground via needed.
    Shared,
    /// Each layer has a ground plane beside it, but not the same one.
    Planes,
    /// At least one layer has no ground plane beside it (2-layer boards).
    Loose,
}

/// What the signal's return current can follow at this via.
fn plane_reference(
    board: &Board,
    v: &super::model::Via,
    net: &str,
    stack: &[String],
    planes: &BTreeSet<String>,
) -> Reference {
    let mut used: BTreeSet<&str> = BTreeSet::new();
    for s in &board.segments {
        if s.net.as_deref() == Some(net)
            && (dist(s.a, v.at) <= v.size / 2.0 + 1e-3 || dist(s.b, v.at) <= v.size / 2.0 + 1e-3)
        {
            used.insert(s.layer.as_str());
        }
    }
    if used.len() < 2 {
        return Reference::Loose;
    }
    let refs = |layer: &str| -> BTreeSet<&str> {
        let Some(i) = stack.iter().position(|l| l == layer) else {
            return BTreeSet::new();
        };
        [i.checked_sub(1), Some(i + 1)]
            .into_iter()
            .flatten()
            .filter_map(|j| stack.get(j))
            .filter(|l| planes.contains(*l))
            .map(String::as_str)
            .collect()
    };
    let mut common: Option<BTreeSet<&str>> = None;
    let mut all_referenced = true;
    for layer in used {
        let r = refs(layer);
        all_referenced &= !r.is_empty();
        common = Some(match common {
            None => r,
            Some(c) => c.intersection(&r).copied().collect(),
        });
    }
    if common.is_some_and(|c| !c.is_empty()) {
        Reference::Shared
    } else if all_referenced && stack.len() >= 4 {
        Reference::Planes
    } else {
        Reference::Loose
    }
}

/// A supply rail, not a control or sense signal named after one
/// (VCC_IO_EN, VBUS_DET, VDD_PG carry no current).
fn is_power_rail(net: &str) -> bool {
    const CONTROL: &[&str] = &[
        "EN", "ENABLE", "PG", "PGOOD", "SENSE", "SNS", "FB", "MON", "OK", "CTRL", "ON", "DET",
        "FLT", "FAULT", "ALERT", "SEL",
    ];
    is_power_net(net)
        && !short_net(net)
            .to_ascii_uppercase()
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|w| CONTROL.contains(&w))
}

fn edge_distance(outline: Rect, p: (f64, f64)) -> f64 {
    (p.0 - outline.0)
        .min(outline.2 - p.0)
        .min(p.1 - outline.1)
        .min(outline.3 - p.1)
}

pub fn audit(board: &Board, opts: &AuditOptions) -> Audit {
    let mut findings = Vec::new();
    let mut eng = Ledger {
        caps: BTreeMap::new(),
    };
    let mut beauty = Ledger {
        caps: BTreeMap::new(),
    };
    let outline = board.outline;
    let ground_nets: BTreeSet<String> = board.nets().into_iter().filter(|n| is_ground(n)).collect();

    if outline.is_none() {
        findings.push(Finding {
            severity: "error",
            area: "engineering",
            check: "outline",
            message: "The board has no Edge.Cuts outline".into(),
            at: None,
            fix: "add_board_outline",
        });
        eng.take("outline", 30, 30);
    }

    let stack = stack_order(&board.copper_layers);
    let ground_planes: BTreeSet<String> = board
        .zones
        .iter()
        .filter(|z| z.net.as_deref().is_some_and(is_ground))
        .flat_map(|z| z.layers.iter().cloned())
        .filter(|l| l.ends_with(".Cu"))
        .collect();
    let mut fast_vias = 0;
    for v in &board.vias {
        let Some(net) = v.net.as_deref() else {
            continue;
        };
        if !is_fast(net) {
            continue;
        }
        fast_vias += 1;
        let reference = match plane_reference(board, v, net, &stack, &ground_planes) {
            Reference::Shared => continue,
            r => r,
        };
        // Between two different ground planes the return current finds the
        // nearest of the many stitching vias a multilayer board carries, so
        // a looser radius applies there and a miss is advice, not a defect.
        let planes = reference == Reference::Planes;
        let radius = if planes {
            opts.return_via_mm.max(2.5)
        } else {
            opts.return_via_mm
        };
        let has_return = board.vias.iter().any(|g| {
            g.net.as_deref().is_some_and(is_ground) && dist(g.at, v.at) <= radius + g.size / 2.0
        }) || board.pads.iter().any(|p| {
            p.through_hole
                && p.net.as_deref().is_some_and(is_ground)
                && p.distance_to(v.at) <= radius
        });
        if !has_return {
            findings.push(Finding {
                severity: if planes { "info" } else { "warning" },
                area: "engineering",
                check: "return_via",
                message: format!(
                    "{} changes layer without a ground via within {radius} mm, so its return current loops around",
                    short_net(net),
                ),
                at: Some(v.at),
                fix: "add_via",
            });
            if planes {
                eng.take("return_via", 1, 8);
            } else {
                eng.take("return_via", 4, 20);
            }
        }
    }

    if let Some(o) = outline {
        let mut flagged = BTreeSet::new();
        for s in &board.segments {
            let Some(net) = s.net.as_deref() else {
                continue;
            };
            if !is_fast(net) || flagged.contains(net) {
                continue;
            }
            let d = edge_distance(o, s.a).min(edge_distance(o, s.b));
            if d < opts.edge_mm {
                flagged.insert(net.to_string());
                findings.push(Finding {
                    severity: "warning",
                    area: "engineering",
                    check: "fast_trace_near_edge",
                    message: format!(
                        "{} runs {:.2} mm from the board edge, where it radiates and picks up ESD",
                        short_net(net),
                        d
                    ),
                    at: Some(s.a),
                    fix: "route_board",
                });
                eng.take("fast_trace_near_edge", 3, 12);
            }
        }
    }

    // A capacitor between a rail and ground is fine when it sits beside any
    // part on that rail (an IC pin, a regulator, the input connector); it is
    // only "far" when nothing it could serve is near.
    let served: Vec<&super::model::Footprint> = board
        .footprints
        .iter()
        .filter(|f| f.pads.len() >= 3 || f.is_connector())
        .collect();
    for f in &board.footprints {
        if f.prefix() != "C" || f.pads.len() != 2 {
            continue;
        }
        let pads: Vec<&super::model::Pad> = f.pads.iter().map(|&i| &board.pads[i]).collect();
        let power = pads.iter().find(|p| {
            p.net
                .as_deref()
                .is_some_and(|n| is_power_net(n) && !is_ground(n))
        });
        let grounded = pads.iter().any(|p| p.net.as_deref().is_some_and(is_ground));
        let (Some(power), true) = (power, grounded) else {
            continue;
        };
        let net = power.net.as_deref().unwrap_or_default();
        let nearest = served
            .iter()
            .filter(|o| o.reference != f.reference)
            .flat_map(|ic| ic.pads.iter().map(|&i| &board.pads[i]))
            .filter(|q| q.net.as_deref() == Some(net))
            .map(|q| (dist(q.at, power.at), q.reference.clone()))
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        if let Some((d, ic)) = nearest {
            if d > opts.decoupling_mm {
                let far = d > opts.decoupling_mm + 2.0;
                findings.push(Finding {
                    severity: if far { "warning" } else { "info" },
                    area: "engineering",
                    check: "decoupling_distance",
                    message: format!(
                        "{} decouples {ic} on {} but sits {:.1} mm from the nearest {} pin (aim for under {} mm)",
                        f.reference,
                        short_net(net),
                        d,
                        short_net(net),
                        opts.decoupling_mm
                    ),
                    at: Some(f.at),
                    fix: "smart_place",
                });
                eng.take("decoupling_distance", if far { 3 } else { 1 }, 15);
            }
        }
    }

    // Power copper narrower than min_power_width over more than a few
    // millimetres. Short neck-downs into small pads are normal, and a ground
    // net that has a pour carries its current in the pour.
    let mut thin: BTreeMap<String, (f64, f64)> = BTreeMap::new();
    for s in &board.segments {
        let Some(net) = s.net.as_deref() else {
            continue;
        };
        if (is_power_rail(net) || is_ground(net))
            && board.poured_layers(net).is_empty()
            && s.width + 1e-9 < opts.min_power_width
        {
            let e = thin.entry(net.to_string()).or_insert((s.width, 0.0));
            e.0 = e.0.min(s.width);
            e.1 += dist(s.a, s.b);
        }
    }
    for (net, (w, length)) in thin.iter().filter(|(_, (_, l))| *l > 3.0) {
        let serious = *w < 0.25 && *length > 20.0;
        findings.push(Finding {
            severity: if serious { "warning" } else { "info" },
            area: "engineering",
            check: "thin_power_track",
            message: format!(
                "{} runs {:.0} mm of track at {:.2} mm; check its current with calc_trace_width (power copper is usually {} mm or wider)",
                short_net(net),
                length,
                w,
                opts.min_power_width
            ),
            at: None,
            fix: "calc_trace_width",
        });
        eng.take("thin_power_track", if serious { 2 } else { 1 }, 8);
    }

    // A corner is a point where exactly two tracks of one net meet on one
    // layer, outside any pad or via of that net (where several tracks
    // legitimately converge). It is acute when the angle between them is
    // under 90°.
    let key = |p: (f64, f64)| ((p.0 * 100.0).round() as i64, (p.1 * 100.0).round() as i64);
    let mut ends: BTreeMap<EndKey, Vec<Spoke>> = BTreeMap::new();
    for s in &board.segments {
        if dist(s.a, s.b) < 1e-6 {
            continue;
        }
        let net = s.net.clone().unwrap_or_default();
        for (p, o) in [(s.a, s.b), (s.b, s.a)] {
            ends.entry((net.clone(), s.layer.clone(), key(p)))
                .or_default()
                .push((p, o));
        }
    }
    let mut acute = 0;
    for ((net, _, _), list) in &ends {
        let [(p, o1), (_, o2)] = list.as_slice() else {
            continue;
        };
        let in_copper = board
            .pads
            .iter()
            .any(|q| q.net.as_deref() == Some(net.as_str()) && q.distance_to(*p) < 1e-6)
            || board
                .vias
                .iter()
                .any(|v| v.net.as_deref() == Some(net.as_str()) && dist(v.at, *p) <= v.size / 2.0);
        if in_copper {
            continue;
        }
        let v1 = (o1.0 - p.0, o1.1 - p.1);
        let v2 = (o2.0 - p.0, o2.1 - p.1);
        let n1 = (v1.0 * v1.0 + v1.1 * v1.1).sqrt();
        let n2 = (v2.0 * v2.0 + v2.1 * v2.1).sqrt();
        let cos = (v1.0 * v2.0 + v1.1 * v2.1) / (n1 * n2);
        // Under about 10° the two tracks lie on top of each other: solid copper,
        // not a corner that traps etchant.
        if cos > 0.05 && cos < 0.985 {
            acute += 1;
            findings.push(Finding {
                severity: "warning",
                area: "engineering",
                check: "acute_angle",
                message: format!(
                    "{} turns through a {:.0}° corner, an acid trap and an impedance step",
                    if net.is_empty() {
                        "a track"
                    } else {
                        short_net(net)
                    },
                    cos.clamp(-1.0, 1.0).acos().to_degrees()
                ),
                at: Some(*p),
                fix: "modify_trace",
            });
            eng.take("acute_angle", 2, 10);
        }
    }

    // Dense multilayer boards put vias in pads on purpose and order them
    // filled and capped; on a cheap 2-layer board it is usually an accident.
    let dense = board.copper_layers.len() >= 4;
    let mut via_pads = BTreeSet::new();
    for v in &board.vias {
        if let Some(p) = board
            .pads
            .iter()
            .find(|p| !p.through_hole && p.distance_to(v.at) < 1e-6)
        {
            if !via_pads.insert((p.reference.clone(), p.number.clone())) {
                continue;
            }
            findings.push(Finding {
                severity: if dense { "info" } else { "warning" },
                area: "engineering",
                check: "via_in_pad",
                message: format!(
                    "A via sits inside pad {}.{}; without plugging it wicks solder away",
                    p.reference, p.number
                ),
                at: Some(v.at),
                fix: "delete_trace",
            });
            eng.take("via_in_pad", if dense { 1 } else { 3 }, 12);
        }
    }

    // Rule areas: an antenna keep-out or a mechanical zone is a hard rule.
    let mut kept_out = Vec::new();
    for k in &board.keepouts {
        if k.no_tracks {
            for s in board.segments.iter().filter(|s| k.on(&s.layer)) {
                let mid = ((s.a.0 + s.b.0) / 2.0, (s.a.1 + s.b.1) / 2.0);
                if [s.a, s.b, mid].into_iter().any(|p| k.contains(p)) {
                    kept_out.push((
                        format!(
                            "a {} track",
                            s.net.as_deref().map(short_net).unwrap_or("unconnected")
                        ),
                        mid,
                    ));
                }
            }
        }
        if k.no_vias && k.layers.iter().any(|l| l.ends_with(".Cu")) {
            for v in board.vias.iter().filter(|v| k.contains(v.at)) {
                kept_out.push(("a via".to_string(), v.at));
            }
        }
        if k.no_pads || k.no_footprints {
            for f in board
                .footprints
                .iter()
                .filter(|f| k.owner.as_deref() != Some(f.reference.as_str()))
            {
                let side = if f.back { "B.Cu" } else { "F.Cu" };
                let pads_in = k.no_pads
                    && f.pads.iter().any(|&i| {
                        let p = &board.pads[i];
                        // Netless holes (mounting, NPTH) carry no copper to keep out.
                        p.net.is_some()
                            && k.layers.iter().any(|l| p.on_layer(l))
                            && k.contains(p.at)
                    });
                let body_in = k.no_footprints && k.on(side) && k.contains(f.at);
                if pads_in || body_in {
                    kept_out.push((f.reference.clone(), f.at));
                }
            }
        }
    }
    for (what, at) in kept_out {
        findings.push(Finding {
            severity: "error",
            area: "engineering",
            check: "keepout_violation",
            message: format!(
                "{what} sits inside a keep-out area (antenna or mechanical clearance)"
            ),
            at: Some(at),
            fix: "move_component",
        });
        eng.take("keepout_violation", 8, 24);
    }

    let mut unrouted = 0usize;
    let mut open_nets: Vec<String> = Vec::new();
    for net in board.nets() {
        let pads: Vec<usize> = board
            .pads
            .iter()
            .enumerate()
            .filter(|(_, p)| p.net.as_deref() == Some(net.as_str()))
            .map(|(i, _)| i)
            .collect();
        if pads.len() < 2 {
            continue;
        }
        let groups = super::router::component_count(board, &net, &pads);
        if groups > 1 {
            unrouted += groups - 1;
            open_nets.push(short_net(&net).to_string());
        }
    }
    if unrouted > 0 {
        findings.push(Finding {
            severity: "error",
            area: "engineering",
            check: "unrouted",
            message: format!(
                "{unrouted} connection(s) are not routed yet ({}{})",
                open_nets
                    .iter()
                    .take(4)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", "),
                if open_nets.len() > 4 { ", …" } else { "" }
            ),
            at: None,
            fix: "route_board",
        });
        eng.take("unrouted", (unrouted as i64) * 4, 30);
    }

    if board.copper_layers.len() >= 2
        && board.pads.len() > 4
        && ground_nets
            .iter()
            .all(|n| board.poured_layers(n).is_empty())
    {
        findings.push(Finding {
            severity: "warning",
            area: "engineering",
            check: "no_ground_plane",
            message: "No ground pour: every signal's return current has to find its own way back"
                .into(),
            at: None,
            fix: "add_copper_pour",
        });
        eng.take("no_ground_plane", 10, 10);
    }

    let mut off_angle = 0;
    for s in board.segments.iter().filter(|s| !s.from_arc) {
        let dx = (s.b.0 - s.a.0).abs();
        let dy = (s.b.1 - s.a.1).abs();
        if dx.hypot(dy) > 0.5 && dx > 1e-3 && dy > 1e-3 && (dx - dy).abs() > 0.02 * dx.max(dy) {
            off_angle += 1;
        }
    }
    if off_angle > 0 {
        findings.push(Finding {
            severity: "info",
            area: "beauty",
            check: "off_angle_tracks",
            message: format!(
                "{off_angle} track segment(s) run at angles other than 0°, 45° or 90°"
            ),
            at: None,
            fix: "route_board",
        });
        beauty.take("off_angle_tracks", off_angle as i64, 10);
    }
    // Count only widths that carry a real share of the copper: a few short
    // neck-downs into fine-pitch pads are not a style problem.
    let mut by_width: BTreeMap<i64, f64> = BTreeMap::new();
    for s in &board.segments {
        *by_width
            .entry((s.width * 1000.0).round() as i64)
            .or_default() += dist(s.a, s.b);
    }
    let total: f64 = by_width.values().sum();
    let widths: Vec<i64> = by_width
        .iter()
        .filter(|(_, l)| **l >= 0.03 * total)
        .map(|(w, _)| *w)
        .collect();
    let allowed = if board.copper_layers.len() >= 4 { 7 } else { 5 };
    if widths.len() > allowed {
        findings.push(Finding {
            severity: "info",
            area: "beauty",
            check: "too_many_widths",
            message: format!(
                "{} different track widths; use one per net class",
                widths.len()
            ),
            at: None,
            fix: "set_predefined_sizes",
        });
        beauty.take("too_many_widths", 5, 5);
    }
    let mut unlabelled = Vec::new();
    for f in board.footprints.iter().filter(|f| f.is_connector()) {
        let named: Vec<&super::model::Pad> = f
            .pads
            .iter()
            .map(|&i| &board.pads[i])
            .filter(|p| p.net.as_deref().is_some_and(|n| !is_unnamed(n)))
            .collect();
        if named.is_empty() {
            continue;
        }
        // A pin counts as labelled when any silkscreen text (top or bottom)
        // sits beside it, or a pin table near the connector names it — pro
        // boards label with GPIO numbers and bottom-side tables too.
        let silk: Vec<&super::model::Text> = board
            .texts
            .iter()
            .filter(|t| t.layer.ends_with(".SilkS"))
            .collect();
        let near_table = |label: &str| {
            silk.iter().any(|t| {
                super::model::rect_distance(f.courtyard, t.at) < 6.0
                    && t.text
                        .split(|c: char| c.is_whitespace() || c == '\\')
                        .any(|w| w.eq_ignore_ascii_case(label))
            })
        };
        let hits = named
            .iter()
            .filter(|p| {
                let label = super::polish::pin_label(p.net.as_deref().unwrap_or_default());
                silk.iter().any(|t| dist(t.at, p.at) < 3.0) || near_table(&label)
            })
            .count();
        let labelled = hits * 2 >= named.len();
        if !labelled {
            unlabelled.push(f.reference.clone());
        }
    }
    if !unlabelled.is_empty() {
        findings.push(Finding {
            severity: "info",
            area: "beauty",
            check: "unlabelled_connectors",
            message: format!(
                "Connector pins without silkscreen labels: {}",
                unlabelled.join(", ")
            ),
            at: None,
            fix: "polish_board",
        });
        beauty.take("unlabelled_connectors", unlabelled.len() as i64 * 6, 18);
    }
    let has_logo = board.footprints.iter().any(|f| {
        let l = f.library.to_ascii_lowercase();
        l.contains("logo")
            || l.contains("oshw")
            || f.reference.to_ascii_uppercase().starts_with("LOGO")
    });
    let has_name = has_logo
        || board.texts.iter().any(|t| {
            t.layer.ends_with(".SilkS")
                && t.text.chars().filter(|c| c.is_ascii_alphabetic()).count() >= 4
                && (t.size >= 1.0 - 1e-6 || t.knockout)
        });
    let has_version = board.texts.iter().any(|t| {
        let l = t.text.to_ascii_lowercase();
        t.layer.ends_with(".SilkS")
            && (l.contains("rev")
                || l.split(|c: char| !c.is_ascii_alphanumeric() && c != '.')
                    .any(|w| {
                        w.len() >= 2
                            && w.starts_with('v')
                            && w[1..].chars().next().is_some_and(|c| c.is_ascii_digit())
                    }))
    });
    if !has_name {
        findings.push(Finding {
            severity: "info",
            area: "beauty",
            check: "no_product_name",
            message: "No product name, logo or brand on the silkscreen".into(),
            at: None,
            fix: "polish_board",
        });
        beauty.take("no_product_name", 6, 6);
    }
    if !has_version {
        findings.push(Finding {
            severity: "info",
            area: "beauty",
            check: "no_version",
            message: "No version or revision mark (v1.0, REV A) to tell board spins apart".into(),
            at: None,
            fix: "polish_board",
        });
        beauty.take("no_version", 3, 3);
    }
    if let Some(o) = outline {
        if board.outline_rectangular {
            findings.push(Finding {
                severity: "info",
                area: "beauty",
                check: "sharp_corners",
                message: "The outline has sharp corners; round them (1–3 mm) for a finished look and safer handling".into(),
                at: Some((o.0, o.1)),
                fix: "add_board_outline",
            });
            beauty.take("sharp_corners", 4, 4);
        }
        if (o.2 - o.0) > 30.0 && (o.3 - o.1) > 30.0 && board.mounting_holes == 0 {
            findings.push(Finding {
                severity: "info",
                area: "beauty",
                check: "no_mounting_holes",
                message: "A board this size usually carries corner mounting holes".into(),
                at: None,
                fix: "add_mounting_hole",
            });
            beauty.take("no_mounting_holes", 4, 4);
        }
        if let Some(gnd) = ground_nets
            .iter()
            .find(|n| board.poured_layers(n).len() >= 2)
        {
            let area = (o.2 - o.0) * (o.3 - o.1);
            let stitches = board
                .vias
                .iter()
                .filter(|v| v.net.as_deref() == Some(gnd.as_str()))
                .count();
            if area > 1000.0 && (stitches as f64) < area / 150.0 {
                findings.push(Finding {
                    severity: "info",
                    area: "beauty",
                    check: "sparse_stitching",
                    message: format!("{stitches} ground stitching via(s) for {area:.0} mm²; aim for one every 5–10 mm"),
                    at: None,
                    fix: "polish_board",
                });
                beauty.take("sparse_stitching", 4, 4);
            }
        }
    }
    let mut odd_rotation = 0;
    for f in &board.footprints {
        if (f.rotation.rem_euclid(90.0)).abs() > 0.01
            && (f.rotation.rem_euclid(90.0) - 90.0).abs() > 0.01
        {
            odd_rotation += 1;
        }
    }
    if odd_rotation > 0 {
        findings.push(Finding {
            severity: "info",
            area: "beauty",
            check: "odd_rotation",
            message: format!("{odd_rotation} part(s) are rotated off the 90° grid"),
            at: None,
            fix: "smart_place",
        });
        beauty.take("odd_rotation", odd_rotation as i64, 8);
    }
    let silk = super::silk::reference_findings(board);
    let mut by_check: BTreeMap<&'static str, Vec<String>> = BTreeMap::new();
    for (_, check, message) in &silk {
        by_check.entry(check).or_default().push(message.clone());
    }
    for (check, messages) in by_check {
        let (severity, per, cap) = match check {
            "designator_on_copper" | "designators_overlap" => ("warning", 3, 15),
            "designator_too_small" | "tilted_text" => ("warning", 2, 10),
            "designator_on_via" | "designator_small" => ("info", 1, 4),
            _ => ("info", 2, 6),
        };
        let shown: Vec<&str> = messages.iter().take(6).map(String::as_str).collect();
        findings.push(Finding {
            severity,
            area: "beauty",
            check,
            message: format!(
                "{}{}",
                shown.join("; "),
                if messages.len() > 6 {
                    format!(" and {} more", messages.len() - 6)
                } else {
                    String::new()
                }
            ),
            at: None,
            fix: "polish_board",
        });
        beauty.take(check, messages.len() as i64 * per, cap);
    }
    let _ = rect_distance;
    let _ = segment_distance;

    let findings = group_findings(findings);
    let metrics = json!({
        "footprints": board.footprints.len(),
        "pads": board.pads.len(),
        "track_segments": board.segments.len(),
        "vias": board.vias.len(),
        "fast_signal_vias": fast_vias,
        "track_length_mm": super::model::round3(board.segments.iter().map(|s| dist(s.a, s.b)).sum::<f64>()),
        "track_widths_mm": widths.iter().map(|w| *w as f64 / 1000.0).collect::<Vec<_>>(),
        "unrouted_connections": unrouted,
        "acute_corners": acute,
        "ground_pour_layers": ground_nets.iter().map(|n| (n.clone(), board.poured_layers(n))).collect::<BTreeMap<_, _>>(),
        "board_texts": board.texts.len(),
    });
    Audit {
        engineering: (100 - eng.total()).max(0),
        beauty: (100 - beauty.total()).max(0),
        findings,
        metrics,
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{fixture, Board};
    use super::*;

    fn four_layer(body: &str) -> Board {
        let text = format!(
            r#"(kicad_pcb (version 20250114) (generator "test")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (4 "In1.Cu" signal) (6 "In2.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "GND") (net 2 "USB_DP")
  (gr_rect (start 100 100) (end 130 120) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
  (zone (net 1) (net_name "GND") (layer "In1.Cu") (polygon (pts (xy 100 100) (xy 130 100) (xy 130 120) (xy 100 120))))
  (segment (start 105 110) (end 110 110) (width 0.2) (layer "F.Cu") (net 2))
  (via (at 110 110) (size 0.6) (drill 0.3) (layers "F.Cu" "B.Cu") (net 2))
{body})"#
        );
        Board::from_text(&text).unwrap()
    }

    #[test]
    fn return_via_follows_the_reference_plane() {
        let shared = four_layer(
            r#"  (segment (start 110 110) (end 120 110) (width 0.2) (layer "In2.Cu") (net 2))"#,
        );
        let a = audit(&shared, &AuditOptions::default());
        assert!(!a.findings.iter().any(|f| f.check == "return_via"));
        let split = four_layer(
            r#"  (segment (start 110 110) (end 120 110) (width 0.2) (layer "B.Cu") (net 2))"#,
        );
        let a = audit(&split, &AuditOptions::default());
        assert!(a.findings.iter().any(|f| f.check == "return_via"));
    }

    #[test]
    fn arcs_are_not_off_angle_and_enables_are_not_rails() {
        let body = r#"  (arc (start 110 110) (mid 112.93 110.76) (end 115 113) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (segment (start 105 115) (end 125 115) (width 0.15) (layer "F.Cu") (net 9 "VCC_IO_EN"))
"#;
        let b = Board::from_text(&fixture::board(body)).unwrap();
        let a = audit(&b, &AuditOptions::default());
        assert!(!a.findings.iter().any(|f| f.check == "off_angle_tracks"));
        assert!(!a.findings.iter().any(|f| f.check == "thin_power_track"));
    }

    #[test]
    fn flags_copper_inside_a_keepout() {
        let body = r#"  (zone (net 0) (net_name "") (layers "F.Cu") (keepout (tracks not_allowed) (vias not_allowed) (pads allowed) (copperpour allowed) (footprints allowed)) (polygon (pts (xy 110 101) (xy 120 101) (xy 120 110) (xy 110 110))))
  (segment (start 105 105) (end 125 105) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
"#;
        let b = Board::from_text(&fixture::board(body)).unwrap();
        let a = audit(&b, &AuditOptions::default());
        assert!(a.findings.iter().any(|f| f.check == "keepout_violation"));
    }

    #[test]
    fn a_track_ending_on_another_track_is_connected() {
        let body = format!(
            "{}{}{}",
            fixture::smd("R1", 110.0, 110.0, 0.0, ("SIG", "GND")),
            fixture::smd("R2", 120.0, 116.0, 0.0, ("SIG", "GND")),
            r#"  (segment (start 109.1 110) (end 125 110) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (segment (start 119.1 116) (end 119.1 110.1) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
"#
        );
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let sig: Vec<usize> = (0..b.pads.len())
            .filter(|&i| b.pads[i].net.as_deref() == Some("SIG"))
            .collect();
        assert_eq!(super::super::router::component_count(&b, "SIG", &sig), 1);
    }

    #[test]
    fn flags_unrouted_thin_power_and_missing_finish() {
        let mut body = fixture::header("J1", 104.0, 106.0, &["+5V", "GND"]);
        body.push_str(&fixture::smd("R1", 115.0, 106.0, 0.0, ("+5V", "SIG")));
        body.push_str(&fixture::smd("R2", 120.0, 106.0, 0.0, ("SIG", "GND")));
        body.push_str(
            r#"  (segment (start 104 106) (end 110 106) (width 0.2) (layer "F.Cu") (net 2 "+5V"))
"#,
        );
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let a = audit(&b, &AuditOptions::default());
        let checks: BTreeSet<&str> = a.findings.iter().map(|f| f.check).collect();
        assert!(checks.contains("unrouted"), "{checks:?}");
        assert!(checks.contains("thin_power_track"));
        assert!(checks.contains("unlabelled_connectors"));
        assert!(checks.contains("no_product_name"));
        assert!(checks.contains("no_version"));
        assert!(checks.contains("no_ground_plane"));
        assert!(a.engineering < 90 && a.beauty < 90);
    }

    #[test]
    fn flags_acute_corners_and_off_angle_tracks() {
        let body = r#"  (segment (start 110 110) (end 120 110) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (segment (start 110 110) (end 118 112) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
"#;
        let b = Board::from_text(&fixture::board(body)).unwrap();
        let a = audit(&b, &AuditOptions::default());
        assert!(a.findings.iter().any(|f| f.check == "acute_angle"));
        assert!(a.findings.iter().any(|f| f.check == "off_angle_tracks"));
    }

    #[test]
    fn accepts_45_degree_bends_and_junctions_in_pads() {
        let body = format!(
            "{}{}",
            fixture::smd("R1", 104.0, 110.0, 0.0, ("SIG", "GND")),
            r#"  (segment (start 103.1 110) (end 110 110) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (segment (start 110 110) (end 113 113) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (segment (start 113 113) (end 120 113) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (segment (start 103.1 110) (end 104.2 108.9) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
"#
        );
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let a = audit(&b, &AuditOptions::default());
        assert!(
            !a.findings.iter().any(|f| f.check == "acute_angle"),
            "{:?}",
            a.findings
        );
    }

    #[test]
    fn joins_pads_that_share_a_number_and_pads_inside_a_pour() {
        let body = r#"  (footprint "Castellated:Pin" (layer "F.Cu") (at 110 110)
    (property "Reference" "J1" (at 0 -2 0) (layer "F.SilkS"))
    (pad "1" smd rect (at 0 0) (size 1 1) (layers "F.Cu") (net 3 "SIG"))
    (pad "1" thru_hole circle (at 1.5 0) (size 1 1) (drill 0.6) (layers "*.Cu") (net 3 "SIG"))
    (pad "2" smd rect (at 0 3) (size 1 1) (layers "F.Cu") (net 1 "GND"))
    (pad "3" smd rect (at 0 6) (size 1 1) (layers "F.Cu") (net 1 "GND")))
  (zone (net 1) (net_name "GND") (layers "F.Cu") (polygon (pts (xy 105 112) (xy 115 112) (xy 115 118) (xy 105 118))))
"#;
        let b = Board::from_text(&fixture::board(body)).unwrap();
        let a = audit(&b, &AuditOptions::default());
        assert!(
            !a.findings.iter().any(|f| f.check == "unrouted"),
            "{:?}",
            a.findings
        );
    }
}

#[cfg(test)]
mod corpus {
    use super::super::model::Board;
    use super::*;

    #[test]
    fn audits_and_polishes_a_real_board() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../voltaris-sexp/tests/fixtures/pic_programmer.kicad_pcb");
        let b = Board::from_text(&std::fs::read_to_string(path).unwrap()).unwrap();
        let a = audit(&b, &AuditOptions::default());
        eprintln!(
            "pic_programmer: engineering {} beauty {} with {} findings",
            a.engineering,
            a.beauty,
            a.findings.len()
        );
        assert!((0..=100).contains(&a.engineering) && (0..=100).contains(&a.beauty));
        let p = super::super::polish::plan(&b, "pic programmer", &Default::default()).unwrap();
        for t in &p.texts {
            let r = super::super::polish::text_box(&t.text, t.at, t.size, t.rotation);
            for q in &b.pads {
                let side_ok = if t.layer.starts_with("B.") {
                    q.back
                } else {
                    q.front
                };
                assert!(
                    !side_ok || !super::super::model::rects_overlap(r, q.bbox(), 0.0),
                    "{} overlaps a pad",
                    t.text
                );
            }
        }
        eprintln!(
            "polish: {} texts, {} vias, {} zones",
            p.texts.len(),
            p.vias.len(),
            p.zones.len()
        );
    }
}

#[cfg(test)]
mod reference_boards {
    use super::super::model::Board;
    use super::*;

    /// Prints the copper islands of one net: VOLTARIS_DEBUG_NET="board::net".
    #[test]
    #[ignore = "needs VOLTARIS_DEBUG_NET"]
    fn debug_net() {
        let Ok(spec) = std::env::var("VOLTARIS_DEBUG_NET") else {
            return;
        };
        let (path, want) = spec.split_once("::").expect("board::net");
        let b = Board::from_text(&std::fs::read_to_string(path).unwrap()).unwrap();
        for net in b.nets().into_iter().filter(|n| short_net(n) == want) {
            let pads: Vec<usize> = (0..b.pads.len())
                .filter(|&i| b.pads[i].net.as_deref() == Some(net.as_str()))
                .collect();
            for g in super::super::router::islands(&b, &net, &pads) {
                eprintln!("island:");
                for i in g {
                    let p = &b.pads[i];
                    eprintln!(
                        "  {}.{} at {:?} size {:?} rot {} f{} b{} th{}",
                        p.reference,
                        p.number,
                        p.at,
                        p.size,
                        p.rotation,
                        p.front,
                        p.back,
                        p.through_hole
                    );
                }
            }
            for s in b
                .segments
                .iter()
                .filter(|s| s.net.as_deref() == Some(net.as_str()))
            {
                eprintln!("  seg {:?} {:?} w{} {}", s.a, s.b, s.width, s.layer);
            }
            for v in b
                .vias
                .iter()
                .filter(|v| v.net.as_deref() == Some(net.as_str()))
            {
                eprintln!("  via {:?} {}", v.at, v.size);
            }
            for z in b
                .zones
                .iter()
                .filter(|z| z.net.as_deref() == Some(net.as_str()))
            {
                eprintln!("  zone {:?} {} pts", z.layers, z.points.len());
            }
        }
    }

    /// Audits every board listed in VOLTARIS_REFERENCE_BOARDS (one path per
    /// line): a check on the auditor itself against boards real teams shipped.
    #[test]
    #[ignore = "needs VOLTARIS_REFERENCE_BOARDS"]
    fn audits_reference_boards() {
        let Ok(list) = std::env::var("VOLTARIS_REFERENCE_BOARDS") else {
            return;
        };
        for path in list.lines().filter(|l| !l.trim().is_empty()) {
            let Ok(text) = std::fs::read_to_string(path) else {
                println!("{path}: unreadable");
                continue;
            };
            let b = match Board::from_text(&text) {
                Ok(b) => b,
                Err(e) => {
                    println!("{path}: parse failed: {e}");
                    continue;
                }
            };
            let a = audit(&b, &AuditOptions::default());
            eprintln!(
                "\n== {path}\n   engineering {} beauty {} layers {}",
                a.engineering,
                a.beauty,
                b.copper_layers.len()
            );
            for f in &a.findings {
                eprintln!(
                    "   [{}/{}] {}: {} @{:?}",
                    f.area,
                    f.severity,
                    f.check,
                    f.message.chars().take(160).collect::<String>(),
                    f.at
                );
            }
        }
    }
}
