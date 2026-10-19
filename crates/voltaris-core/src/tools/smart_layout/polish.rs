//! Board polish: connector pin labels on both sides, ground pours mirrored to
//! the second outer layer, a regular grid of ground stitching vias, and a
//! product name/version block — the finish of a maker-brand board.

use super::model::{
    dist, is_ground, is_unnamed, rects_overlap, segment_distance, short_net, Board, Rect,
};

#[derive(Debug, Clone)]
pub struct PolishOptions {
    pub labels: bool,
    pub both_sides: bool,
    pub label_size: f64,
    pub stitch: bool,
    pub stitch_pitch: f64,
    pub via_size: f64,
    pub via_drill: f64,
    pub clearance: f64,
    pub edge_margin: f64,
    pub mirror_pours: bool,
    pub title: Option<String>,
    pub version: Option<String>,
    pub back_text: Option<String>,
    pub title_size: f64,
}

impl Default for PolishOptions {
    fn default() -> Self {
        Self {
            labels: true,
            both_sides: true,
            label_size: 0.8,
            stitch: true,
            stitch_pitch: 5.0,
            via_size: 0.6,
            via_drill: 0.3,
            clearance: 0.25,
            edge_margin: 1.0,
            mirror_pours: true,
            title: None,
            version: None,
            back_text: None,
            title_size: 1.2,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextItem {
    pub layer: String,
    pub text: String,
    pub at: (f64, f64),
    pub size: f64,
    pub rotation: f64,
    pub mirror: bool,
    pub purpose: String,
}

#[derive(Debug, Clone)]
pub struct ZoneItem {
    pub net: String,
    pub layer: String,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Default)]
pub struct PolishPlan {
    pub texts: Vec<TextItem>,
    pub vias: Vec<(String, (f64, f64))>,
    pub zones: Vec<ZoneItem>,
    pub notes: Vec<String>,
}

pub fn pin_label(net: &str) -> String {
    let name = short_net(net);
    match name.strip_prefix('+') {
        Some(rest) if rest.starts_with(|c: char| c.is_ascii_digit()) => rest.to_string(),
        _ => name.to_string(),
    }
}

pub fn text_box(text: &str, at: (f64, f64), size: f64, rotation: f64) -> Rect {
    let w = text.chars().count() as f64 * size * 0.85 + size * 0.3;
    let h = size * 1.3;
    let (hw, hh) = if (rotation.rem_euclid(180.0) - 90.0).abs() < 1.0 {
        (h / 2.0, w / 2.0)
    } else {
        (w / 2.0, h / 2.0)
    };
    (at.0 - hw, at.1 - hh, at.0 + hw, at.1 + hh)
}

struct Space<'a> {
    board: &'a Board,
    taken: Vec<(Rect, bool)>,
    inside: Rect,
}

impl Space<'_> {
    fn free(&self, r: Rect, back: bool, own: Option<&str>) -> bool {
        if r.0 < self.inside.0 || r.1 < self.inside.1 || r.2 > self.inside.2 || r.3 > self.inside.3
        {
            return false;
        }
        for p in &self.board.pads {
            let on_side = if back { p.back } else { p.front };
            if on_side && rects_overlap(r, p.bbox(), 0.2) {
                return false;
            }
        }
        for f in &self.board.footprints {
            if Some(f.reference.as_str()) == own || f.back != back || f.is_mechanical() {
                continue;
            }
            if rects_overlap(r, f.courtyard, 0.1) {
                return false;
            }
        }
        !self
            .taken
            .iter()
            .any(|(t, b)| *b == back && rects_overlap(r, *t, 0.2))
    }
}

pub fn plan(board: &Board, project: &str, opts: &PolishOptions) -> anyhow::Result<PolishPlan> {
    let outline = board
        .outline
        .ok_or_else(|| anyhow::anyhow!("the board has no Edge.Cuts outline"))?;
    let mut out = PolishPlan::default();
    let mut space = Space {
        board,
        taken: board
            .texts
            .iter()
            .map(|t| (text_box(&t.text, t.at, 1.0, 0.0), t.layer.starts_with("B.")))
            .collect(),
        inside: (
            outline.0 + 0.3,
            outline.1 + 0.3,
            outline.2 - 0.3,
            outline.3 - 0.3,
        ),
    };

    if opts.labels {
        for f in board.footprints.iter().filter(|f| f.is_connector()) {
            let pads: Vec<&super::model::Pad> = f.pads.iter().map(|&i| &board.pads[i]).collect();
            if pads.is_empty() {
                continue;
            }
            let (xs, ys): (Vec<f64>, Vec<f64>) = pads.iter().map(|p| p.at).unzip();
            let spread_x = xs.iter().cloned().fold(f64::MIN, f64::max)
                - xs.iter().cloned().fold(f64::MAX, f64::min);
            let spread_y = ys.iter().cloned().fold(f64::MIN, f64::max)
                - ys.iter().cloned().fold(f64::MAX, f64::min);
            let vertical = spread_y >= spread_x;
            let centre = ((outline.0 + outline.2) / 2.0, (outline.1 + outline.3) / 2.0);
            let fc = (
                (f.courtyard.0 + f.courtyard.2) / 2.0,
                (f.courtyard.1 + f.courtyard.3) / 2.0,
            );
            let sides: [f64; 2] = if vertical {
                if fc.0 > centre.0 {
                    [-1.0, 1.0]
                } else {
                    [1.0, -1.0]
                }
            } else if fc.1 > centre.1 {
                [-1.0, 1.0]
            } else {
                [1.0, -1.0]
            };
            let mut labelled = 0;
            let mut missing = Vec::new();
            for p in &pads {
                let Some(net) = p.net.as_deref() else {
                    continue;
                };
                if is_unnamed(net) {
                    missing.push(p.number.clone());
                    continue;
                }
                let text = pin_label(net);
                let already = board
                    .texts
                    .iter()
                    .any(|t| t.text == text && dist(t.at, p.at) < 4.0);
                if already {
                    labelled += 1;
                    continue;
                }
                let rotation = if vertical { 0.0 } else { 90.0 };
                let (hx, hy) = p.half_extent();
                let mut placed = false;
                for s in sides {
                    let probe = text_box(&text, (0.0, 0.0), opts.label_size, rotation);
                    let (lw, lh) = ((probe.2 - probe.0) / 2.0, (probe.3 - probe.1) / 2.0);
                    let at = if vertical {
                        (p.at.0 + s * (hx + 0.35 + lw), p.at.1)
                    } else {
                        (p.at.0, p.at.1 + s * (hy + 0.35 + lh))
                    };
                    let r = text_box(&text, at, opts.label_size, rotation);
                    let front_ok = space.free(r, false, Some(&f.reference));
                    let back_ok = !opts.both_sides
                        || !p.through_hole
                        || space.free(r, true, Some(&f.reference));
                    if front_ok && back_ok {
                        out.texts.push(TextItem {
                            layer: "F.SilkS".into(),
                            text: text.clone(),
                            at,
                            size: opts.label_size,
                            rotation,
                            mirror: false,
                            purpose: format!("{} pin {} label", f.reference, p.number),
                        });
                        space.taken.push((r, false));
                        if opts.both_sides && p.through_hole {
                            out.texts.push(TextItem {
                                layer: "B.SilkS".into(),
                                text: text.clone(),
                                at,
                                size: opts.label_size,
                                rotation,
                                mirror: true,
                                purpose: format!("{} pin {} label (bottom)", f.reference, p.number),
                            });
                            space.taken.push((r, true));
                        }
                        labelled += 1;
                        placed = true;
                        break;
                    }
                }
                if !placed {
                    missing.push(p.number.clone());
                }
            }
            if !missing.is_empty() {
                out.notes.push(format!(
                    "{}: no clean spot for the label of pin(s) {} (unnamed net or no room)",
                    f.reference,
                    missing.join(", ")
                ));
            }
            let _ = labelled;
        }
    }

    let title = opts
        .title
        .clone()
        .unwrap_or_else(|| project.replace(['_', '-'], " ").to_uppercase());
    if !title.trim().is_empty() {
        let text = match &opts.version {
            Some(v) => format!("{title} {v}"),
            None => format!("{title} v1.0"),
        };
        let exists = board.texts.iter().any(|t| t.text == text);
        if !exists {
            match find_spot(&space, &text, opts.title_size, false, outline) {
                Some(at) => {
                    space
                        .taken
                        .push((text_box(&text, at, opts.title_size, 0.0), false));
                    out.texts.push(TextItem {
                        layer: "F.SilkS".into(),
                        text,
                        at,
                        size: opts.title_size,
                        rotation: 0.0,
                        mirror: false,
                        purpose: "product name and version".into(),
                    });
                }
                None => out.notes.push(
                    "no free strip for the product name; enlarge the board or shrink the text"
                        .into(),
                ),
            }
        }
    }
    if let Some(back) = &opts.back_text {
        if !board.texts.iter().any(|t| &t.text == back) {
            match find_spot(&space, back, 1.0, true, outline) {
                Some(at) => {
                    space.taken.push((text_box(back, at, 1.0, 0.0), true));
                    out.texts.push(TextItem {
                        layer: "B.SilkS".into(),
                        text: back.clone(),
                        at,
                        size: 1.0,
                        rotation: 0.0,
                        mirror: true,
                        purpose: "bottom-side brand line".into(),
                    });
                }
                None => out
                    .notes
                    .push("no free strip on the bottom for the brand line".into()),
            }
        }
    }

    let outer = ["F.Cu", "B.Cu"];
    let mut pour_nets: Vec<String> = board
        .zones
        .iter()
        .filter_map(|z| z.net.clone())
        .filter(|n| is_ground(n))
        .collect();
    pour_nets.sort();
    pour_nets.dedup();
    type Pour = (String, Vec<String>, Vec<Vec<(f64, f64)>>);
    let mut covered: Vec<Pour> = Vec::new();
    for net in &pour_nets {
        let mut layers = board.poured_layers(net);
        let mut polys: Vec<Vec<(f64, f64)>> = board
            .zones
            .iter()
            .filter(|z| z.net.as_deref() == Some(net.as_str()))
            .map(|z| z.points.clone())
            .collect();
        if opts.mirror_pours && board.copper_layers.len() == 2 {
            let present: Vec<&str> = outer
                .iter()
                .copied()
                .filter(|l| layers.iter().any(|x| x == l))
                .collect();
            if present.len() == 1 {
                let missing = if present[0] == "F.Cu" { "B.Cu" } else { "F.Cu" };
                if let Some(z) = board.zones.iter().find(|z| {
                    z.net.as_deref() == Some(net.as_str())
                        && z.layers.iter().any(|l| l == present[0])
                }) {
                    out.zones.push(ZoneItem {
                        net: net.clone(),
                        layer: missing.into(),
                        points: z.points.clone(),
                    });
                    polys.push(z.points.clone());
                    layers.push(missing.into());
                }
            }
        }
        covered.push((net.clone(), layers, polys));
    }
    if pour_nets.is_empty() && opts.stitch {
        out.notes.push(
            "no ground pour on the board; add one with add_copper_pour, then polish again for stitching vias"
                .into(),
        );
    }

    if opts.stitch {
        for (net, layers, polys) in &covered {
            let outer_layers = layers
                .iter()
                .filter(|l| outer.contains(&l.as_str()))
                .count();
            if outer_layers < 2 {
                continue;
            }
            let keep = opts.via_size / 2.0 + opts.clearance;
            let inset = opts.edge_margin + opts.via_size / 2.0;
            let pitch = opts.stitch_pitch.max(1.5);
            let mut y = outline.1 + inset;
            let mut added = 0;
            while y <= outline.3 - inset + 1e-9 {
                let mut x = outline.0 + inset;
                while x <= outline.2 - inset + 1e-9 {
                    let p = (x, y);
                    let in_pour = polys
                        .iter()
                        .any(|poly| super::model::point_in_polygon(p, poly));
                    let ok = in_pour
                        && !board.footprints.iter().any(|f| {
                            !f.is_mechanical()
                                && p.0 > f.courtyard.0 - 0.3
                                && p.0 < f.courtyard.2 + 0.3
                                && p.1 > f.courtyard.1 - 0.3
                                && p.1 < f.courtyard.3 + 0.3
                        })
                        && !board.pads.iter().any(|q| q.distance_to(p) < keep + 0.1)
                        && !board
                            .segments
                            .iter()
                            .any(|s| segment_distance(s.a, s.b, p) - s.width / 2.0 < keep)
                        && !board
                            .vias
                            .iter()
                            .any(|v| dist(v.at, p) < keep + v.size / 2.0)
                        && !space
                            .taken
                            .iter()
                            .any(|(r, _)| super::model::rect_distance(*r, p) < opts.via_size / 2.0);
                    if ok {
                        out.vias.push((net.clone(), p));
                        added += 1;
                    }
                    x += pitch;
                }
                y += pitch;
            }
            if added == 0 {
                out.notes.push(format!(
                    "{net}: no free spot for stitching vias at this pitch"
                ));
            }
        }
    }
    Ok(out)
}

fn find_spot(
    space: &Space<'_>,
    text: &str,
    size: f64,
    back: bool,
    outline: Rect,
) -> Option<(f64, f64)> {
    let probe = text_box(text, (0.0, 0.0), size, 0.0);
    let (hw, hh) = ((probe.2 - probe.0) / 2.0, (probe.3 - probe.1) / 2.0);
    let cx = (outline.0 + outline.2) / 2.0;
    let rows = [
        outline.3 - 1.0 - hh,
        outline.1 + 1.0 + hh,
        (outline.1 + outline.3) / 2.0,
    ];
    let step = 0.5;
    let span = ((outline.2 - outline.0) / 2.0 / step) as i32;
    for y in rows {
        for k in 0..span {
            let off = if k % 2 == 0 { k / 2 } else { -(k + 1) / 2 } as f64 * step;
            let at = (cx + off, y);
            let r = (at.0 - hw, at.1 - hh, at.0 + hw, at.1 + hh);
            if space.free(r, back, None) {
                return Some(at);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::super::model::{fixture, Board};
    use super::*;

    #[test]
    fn labels_header_pins_on_both_sides_and_adds_a_title() {
        let body = fixture::header("J1", 112.0, 106.0, &["+5V", "GND", "SIG"]);
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let p = plan(&b, "led-test", &PolishOptions::default()).unwrap();
        let front: Vec<&TextItem> = p.texts.iter().filter(|t| t.layer == "F.SilkS").collect();
        let back: Vec<&TextItem> = p.texts.iter().filter(|t| t.layer == "B.SilkS").collect();
        assert!(front.iter().any(|t| t.text == "5V"));
        assert!(front.iter().any(|t| t.text == "GND"));
        assert!(front.iter().any(|t| t.text == "SIG"));
        assert_eq!(back.len(), 3);
        assert!(back.iter().all(|t| t.mirror));
        assert!(front.iter().any(|t| t.text == "LED TEST v1.0"));
        for t in &p.texts {
            let r = text_box(&t.text, t.at, t.size, t.rotation);
            for q in &b.pads {
                assert!(
                    !rects_overlap(r, q.bbox(), 0.0),
                    "{} overlaps pad {}",
                    t.text,
                    q.number
                );
            }
        }
    }

    #[test]
    fn mirrors_a_single_ground_pour_and_stitches_it() {
        let mut body = fixture::smd("R1", 110.0, 110.0, 0.0, ("GND", "SIG"));
        body.push_str(
            r#"  (zone (net 1) (net_name "GND") (layers "B.Cu") (polygon (pts (xy 100 100) (xy 130 100) (xy 130 120) (xy 100 120))))
"#,
        );
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let p = plan(&b, "x", &PolishOptions::default()).unwrap();
        assert_eq!(p.zones.len(), 1);
        assert_eq!(p.zones[0].layer, "F.Cu");
        assert!(p.vias.len() >= 6, "{} vias", p.vias.len());
        for (_, v) in &p.vias {
            for q in &b.pads {
                assert!(q.distance_to(*v) >= 0.55, "via too close to pad");
            }
        }
    }
}
