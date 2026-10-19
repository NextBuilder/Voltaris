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

    let mut fast_vias = 0;
    for v in &board.vias {
        let Some(net) = v.net.as_deref() else {
            continue;
        };
        if !is_fast(net) {
            continue;
        }
        fast_vias += 1;
        let has_return = board.vias.iter().any(|g| {
            g.net.as_deref().is_some_and(is_ground)
                && dist(g.at, v.at) <= opts.return_via_mm + g.size / 2.0
        }) || board.pads.iter().any(|p| {
            p.through_hole
                && p.net.as_deref().is_some_and(is_ground)
                && p.distance_to(v.at) <= opts.return_via_mm
        });
        if !has_return {
            findings.push(Finding {
                severity: "warning",
                area: "engineering",
                check: "return_via",
                message: format!(
                    "{} changes layer without a ground via within {} mm, so its return current loops around",
                    short_net(net),
                    opts.return_via_mm
                ),
                at: Some(v.at),
                fix: "add_via",
            });
            eng.take("return_via", 4, 20);
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
                findings.push(Finding {
                    severity: "warning",
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
                eng.take("decoupling_distance", 3, 15);
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
        let poured_ground = is_ground(net) && !board.poured_layers(net).is_empty();
        if (is_power_net(net) || is_ground(net))
            && !poured_ground
            && s.width + 1e-9 < opts.min_power_width
        {
            let e = thin.entry(net.to_string()).or_insert((s.width, 0.0));
            e.0 = e.0.min(s.width);
            e.1 += dist(s.a, s.b);
        }
    }
    for (net, (w, length)) in thin.iter().filter(|(_, (_, l))| *l > 3.0) {
        findings.push(Finding {
            severity: if *w < 0.25 { "warning" } else { "info" },
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
        eng.take("thin_power_track", if *w < 0.25 { 2 } else { 1 }, 8);
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
        if cos > 0.05 {
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

    for v in &board.vias {
        if let Some(p) = board
            .pads
            .iter()
            .find(|p| !p.through_hole && p.distance_to(v.at) < 1e-6)
        {
            findings.push(Finding {
                severity: "warning",
                area: "engineering",
                check: "via_in_pad",
                message: format!(
                    "A via sits inside pad {}.{}; without plugging it wicks solder away",
                    p.reference, p.number
                ),
                at: Some(v.at),
                fix: "delete_trace",
            });
            eng.take("via_in_pad", 3, 12);
        }
    }

    let mut unrouted = 0usize;
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
        }
    }
    if unrouted > 0 {
        findings.push(Finding {
            severity: "error",
            area: "engineering",
            check: "unrouted",
            message: format!("{unrouted} connection(s) are not routed yet"),
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
    for s in &board.segments {
        let dx = (s.b.0 - s.a.0).abs();
        let dy = (s.b.1 - s.a.1).abs();
        if dx > 1e-3 && dy > 1e-3 && (dx - dy).abs() > 0.02 * dx.max(dy) {
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
        beauty.take("off_angle_tracks", off_angle as i64 * 2, 16);
    }
    let widths: BTreeSet<i64> = board
        .segments
        .iter()
        .map(|s| (s.width * 1000.0).round() as i64)
        .collect();
    if widths.len() > 4 {
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
            "designator_on_via" => ("info", 1, 4),
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

    /// Audits every board listed in VOLTARIS_REFERENCE_BOARDS (one path per
    /// line): a check on the auditor itself against boards real teams shipped.
    #[test]
    #[ignore = "needs VOLTARIS_REFERENCE_BOARDS"]
    fn audits_reference_boards() {
        let Ok(list) = std::env::var("VOLTARIS_REFERENCE_BOARDS") else {
            return;
        };
        for path in list.lines().filter(|l| !l.trim().is_empty()) {
            let text = std::fs::read_to_string(path).unwrap();
            let b = Board::from_text(&text).unwrap();
            let a = audit(&b, &AuditOptions::default());
            eprintln!(
                "\n== {path}\n   engineering {} beauty {}",
                a.engineering, a.beauty
            );
            for f in &a.findings {
                eprintln!(
                    "   [{}/{}] {}: {}",
                    f.area,
                    f.severity,
                    f.check,
                    f.message.chars().take(160).collect::<String>()
                );
            }
        }
    }
}
