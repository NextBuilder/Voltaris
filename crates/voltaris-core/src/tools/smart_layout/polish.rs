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
    pub knockout_title: bool,
    pub references: RefMode,
    pub ref_size: f64,
    pub style: LabelStyle,
    /// Function captions for connectors and other parts, e.g. J2 → "MOTOR A".
    pub captions: Vec<(String, String)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LabelStyle {
    /// Product look: a framed strip of pin names lined up with the pins, a
    /// rounded caption badge per connector, framed test-point rows.
    Product,
    /// A plain text label beside each pin.
    Plain,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefMode {
    /// One size, upright, beside each part and off copper; hidden when no room.
    Tidy,
    /// Leave designators as they are.
    Keep,
    /// Hide every designator (clean display boards; the BOM keeps them).
    Hide,
}

impl Default for PolishOptions {
    fn default() -> Self {
        Self {
            labels: true,
            both_sides: true,
            label_size: 1.0,
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
            knockout_title: true,
            references: RefMode::Tidy,
            ref_size: 1.0,
            style: LabelStyle::Product,
            captions: Vec::new(),
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
    pub knockout: bool,
    pub bold: bool,
    /// Narrow glyphs (75 % width) so short pin names fit 2 mm cells.
    pub condensed: bool,
    pub purpose: String,
}

/// A silkscreen line or a rounded frame (corner radius > 0 draws a rounded
/// rectangle from `a` to `b`).
#[derive(Debug, Clone, PartialEq)]
pub struct GraphicItem {
    pub layer: String,
    pub a: (f64, f64),
    pub b: (f64, f64),
    pub frame: bool,
    pub radius: f64,
    pub width: f64,
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
    pub graphics: Vec<GraphicItem>,
    pub vias: Vec<(String, (f64, f64))>,
    pub zones: Vec<ZoneItem>,
    pub references: Vec<super::silk::RefPlacement>,
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
            if Some(f.reference.as_str()) == own {
                // A part's own body outline still blocks its labels.
                if f.back == back && f.silk.iter().any(|s| rects_overlap(r, *s, 0.1)) {
                    return false;
                }
                continue;
            }
            if f.back != back || f.is_mechanical() {
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
            .map(|t| {
                (
                    text_box(&t.text, t.at, t.size, t.rotation),
                    t.layer.starts_with("B."),
                )
            })
            .collect(),
        inside: (
            outline.0 + 0.3,
            outline.1 + 0.3,
            outline.2 - 0.3,
            outline.3 - 0.3,
        ),
    };
    space.taken.extend(
        board
            .art
            .iter()
            .map(|(layer, r)| (*r, layer.starts_with("B."))),
    );

    let mut done: Vec<String> = Vec::new();
    if opts.labels && opts.style == LabelStyle::Product {
        done = product_labels(board, &mut space, opts, outline, &mut out);
    }
    if opts.labels {
        for f in board
            .footprints
            .iter()
            .filter(|f| f.is_connector() && !done.contains(&f.reference))
        {
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
                            knockout: false,
                            bold: false,
                            condensed: false,
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
                                knockout: false,
                                bold: false,
                                condensed: false,
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
                        knockout: opts.knockout_title,
                        bold: true,
                        condensed: false,
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
                        knockout: false,
                        bold: true,
                        condensed: false,
                        purpose: "bottom-side brand line".into(),
                    });
                }
                None => out
                    .notes
                    .push("no free strip on the bottom for the brand line".into()),
            }
        }
    }

    match opts.references {
        RefMode::Keep => {}
        RefMode::Tidy => {
            let silk_opts = super::silk::SilkOptions {
                size: opts.ref_size.max(0.8),
                ..Default::default()
            };
            let (refs, notes) =
                super::silk::tidy_references(board, space.taken.clone(), &silk_opts);
            for r in refs.iter().filter(|r| r.visible) {
                space.taken.push((
                    text_box(&r.reference, r.at, r.size, r.rotation),
                    r.layer.starts_with("B."),
                ));
            }
            out.references = refs;
            out.notes.extend(notes);
        }
        RefMode::Hide => {
            out.references = board
                .footprints
                .iter()
                .filter_map(|f| {
                    let t = f.ref_text.as_ref()?;
                    t.visible.then(|| super::silk::RefPlacement {
                        reference: f.reference.clone(),
                        at: t.at,
                        size: t.size,
                        stroke: t.stroke,
                        rotation: t.rotation,
                        layer: t.layer.clone(),
                        mirrored: f.back,
                        visible: false,
                        spot: "hidden",
                    })
                })
                .collect();
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

/// One row of pins (or test points) to label as a framed strip.
struct Row {
    owner: String,
    caption: Option<String>,
    /// Pin positions and their labels, sorted along the row.
    pins: Vec<((f64, f64), String)>,
    vertical: bool,
    through_hole: bool,
    /// Extent the strip must stay clear of (the part's courtyard or pads).
    body: Rect,
    pads_only: Rect,
}

const FRAME_W: f64 = 0.15;
const FRAME_R: f64 = 0.35;

/// Product-style labels: for every single-row connector and every row of test
/// points, a rounded strip with one cell per pin lined up with the pins, and a
/// rounded caption badge (from `captions`) beyond it. Returns the references
/// it labelled.
fn product_labels(
    board: &Board,
    space: &mut Space<'_>,
    opts: &PolishOptions,
    outline: Rect,
    out: &mut PolishPlan,
) -> Vec<String> {
    let caption_of = |r: &str| {
        opts.captions
            .iter()
            .find(|(k, _)| k == r)
            .map(|(_, v)| v.to_uppercase())
    };
    let mut rows: Vec<Row> = Vec::new();
    let mut odd: Vec<&super::model::Footprint> = Vec::new();
    for f in board.footprints.iter().filter(|f| f.is_connector()) {
        let pads: Vec<&super::model::Pad> = f
            .pads
            .iter()
            .map(|&i| &board.pads[i])
            .filter(|p| p.net.as_deref().is_some_and(|n| !is_unnamed(n)))
            .collect();
        if pads.len() < 2 {
            continue;
        }
        let labelled = pads
            .iter()
            .filter(|p| {
                let label = pin_label(p.net.as_deref().unwrap_or_default());
                board
                    .texts
                    .iter()
                    .any(|t| t.text == label && dist(t.at, p.at) < 4.0)
                    || board
                        .art
                        .iter()
                        .any(|(_, r)| super::model::rect_distance(*r, p.at) < 3.0)
            })
            .count();
        if labelled * 2 >= pads.len() {
            continue;
        }
        let mut pins: Vec<((f64, f64), String)> = Vec::new();
        for p in &pads {
            if !pins.iter().any(|(at, _)| dist(*at, p.at) < 0.05) {
                pins.push((p.at, pin_label(p.net.as_deref().unwrap_or_default())));
            }
        }
        let pads_only = pads
            .iter()
            .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |r, p| {
                let b = p.bbox();
                (r.0.min(b.0), r.1.min(b.1), r.2.max(b.2), r.3.max(b.3))
            });
        if let Some(row) = make_row(
            &f.reference,
            caption_of(&f.reference),
            pins,
            pads.iter().any(|p| p.through_hole),
            f.courtyard,
            pads_only,
        ) {
            rows.push(row);
        } else {
            odd.push(f);
        }
    }
    // Test points in a line share one framed row of their net names.
    let tps: Vec<&super::model::Footprint> = board
        .footprints
        .iter()
        .filter(|f| f.prefix() == "TP" && f.pads.len() == 1)
        .collect();
    let mut used = vec![false; tps.len()];
    for i in 0..tps.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![i];
        for j in i + 1..tps.len() {
            let (a, b) = (tps[i].at, tps[j].at);
            if !used[j] && ((a.1 - b.1).abs() < 0.3 || (a.0 - b.0).abs() < 0.3) {
                group.push(j);
            }
        }
        if group.len() < 2 {
            continue;
        }
        let pins: Vec<((f64, f64), String)> = group
            .iter()
            .filter_map(|&k| {
                let p = &board.pads[tps[k].pads[0]];
                Some((p.at, pin_label(p.net.as_deref()?)))
            })
            .collect();
        let body = group
            .iter()
            .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |r, &k| {
                let c = tps[k].courtyard;
                (r.0.min(c.0), r.1.min(c.1), r.2.max(c.2), r.3.max(c.3))
            });
        if let Some(row) = make_row("TP", Some("TEST POINTS".into()), pins, false, body, body) {
            for &k in &group {
                used[k] = true;
            }
            rows.push(row);
        }
    }

    let centre = ((outline.0 + outline.2) / 2.0, (outline.1 + outline.3) / 2.0);
    let mut done = Vec::new();
    // USB, barrel and two-row connectors: a caption badge says what they are;
    // USB pins are never labelled one by one on a product board.
    for f in odd {
        let mid = (
            (f.courtyard.0 + f.courtyard.2) / 2.0,
            (f.courtyard.1 + f.courtyard.3) / 2.0,
        );
        let inward = if mid.1 > centre.1 { -1.0 } else { 1.0 };
        if let Some(c) = caption_of(&f.reference) {
            place_badge(space, out, &f.reference, &c, f.courtyard, inward, false);
        }
        let usb = f.library.to_ascii_uppercase().contains("USB")
            || f.pads.iter().any(|&i| {
                board.pads[i].net.as_deref().is_some_and(|n| {
                    matches!(short_net(n).to_ascii_uppercase().as_str(), "CC1" | "CC2")
                })
            });
        if usb || caption_of(&f.reference).is_some() {
            done.push(f.reference.clone());
        }
    }
    // Captions for parts that are not connectors (a button, an LED).
    for (r, c) in &opts.captions {
        if let Some(f) = board.footprint(r).filter(|f| !f.is_connector()) {
            let mid = (
                (f.courtyard.0 + f.courtyard.2) / 2.0,
                (f.courtyard.1 + f.courtyard.3) / 2.0,
            );
            let inward = if mid.1 > centre.1 { -1.0 } else { 1.0 };
            place_badge(space, out, r, &c.to_uppercase(), f.courtyard, inward, false);
        }
    }
    for row in rows {
        let mid = (
            (row.body.0 + row.body.2) / 2.0,
            (row.body.1 + row.body.3) / 2.0,
        );
        // Inward first: labels face the middle of the board.
        let inward = if row.vertical {
            if mid.0 > centre.0 {
                -1.0
            } else {
                1.0
            }
        } else if mid.1 > centre.1 {
            -1.0
        } else {
            1.0
        };
        let size = opts.label_size.max(0.8);
        let fits = |space: &Space<'_>, side: f64, back: bool, caption: bool| {
            strip_plan(&row, side, size, back, caption).is_some_and(|plan| {
                plan.iter()
                    .all(|(r, _)| space.free(*r, back, Some(&row.owner)))
            })
        };
        let mut placed = false;
        for side in [inward, -inward] {
            if fits(space, side, false, true) {
                commit_strip(&row, side, size, false, true, space, out);
                if row.through_hole && opts.both_sides && fits(space, side, true, false) {
                    commit_strip(&row, side, size, true, false, space, out);
                }
                placed = true;
                break;
            }
        }
        // No room on top: the strip goes on the bottom (under through-hole
        // leads, or behind a surface-mount connector) and the top keeps only
        // the caption, the way Scribe and Mini UPS print their pin legends.
        if !placed && opts.both_sides {
            for side in [inward, -inward] {
                if fits(space, side, true, false) {
                    commit_strip(&row, side, size, true, false, space, out);
                    if let Some(c) = &row.caption {
                        place_badge(space, out, &row.owner, c, row.body, inward, row.vertical);
                    }
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            for side in [inward, -inward] {
                if fits(space, side, false, false) {
                    commit_strip(&row, side, size, false, false, space, out);
                    if let Some(c) = &row.caption {
                        place_badge(space, out, &row.owner, c, row.body, inward, row.vertical);
                    }
                    placed = true;
                    break;
                }
            }
        }
        if !placed {
            if let Some(c) = &row.caption {
                place_badge(space, out, &row.owner, c, row.body, inward, row.vertical);
            }
        }
        if let Some(c) = &row.caption {
            if !out
                .texts
                .iter()
                .any(|t| &t.text == c && t.purpose.starts_with(&row.owner))
            {
                out.notes.push(format!(
                    "{}: no room for the \"{c}\" caption; leave a {:.1} mm lane beside it (smart_place label_lane)",
                    row.owner, 2.6
                ));
            }
        }
        if placed {
            done.push(row.owner.clone());
        } else if row.owner != "TP" {
            out.notes.push(format!(
                "{}: no room for a framed pin strip; plain labels used instead",
                row.owner
            ));
        }
    }
    done
}

fn make_row(
    owner: &str,
    caption: Option<String>,
    mut pins: Vec<((f64, f64), String)>,
    through_hole: bool,
    body: Rect,
    pads_only: Rect,
) -> Option<Row> {
    let (xs, ys): (Vec<f64>, Vec<f64>) = pins.iter().map(|(p, _)| *p).unzip();
    let spread = |v: &[f64]| {
        v.iter().cloned().fold(f64::MIN, f64::max) - v.iter().cloned().fold(f64::MAX, f64::min)
    };
    let vertical = spread(&ys) > spread(&xs);
    // A single straight row only: two-row headers and odd connectors keep
    // plain labels.
    let off_axis = if vertical { spread(&xs) } else { spread(&ys) };
    if off_axis > 0.3 {
        return None;
    }
    pins.sort_by(|a, b| {
        let (ka, kb) = if vertical {
            (a.0 .1, b.0 .1)
        } else {
            (a.0 .0, b.0 .0)
        };
        ka.partial_cmp(&kb).unwrap_or(std::cmp::Ordering::Equal)
    });
    shorten_labels(&mut pins);
    Some(Row {
        owner: owner.to_string(),
        caption,
        pins,
        vertical,
        through_hole,
        body,
        pads_only,
    })
}

/// Drops a prefix that several pins share up to an underscore, so MA_1A,
/// MA_2A, MA_1B print as 1A, 2A, 1B; power and ground names stay whole.
fn shorten_labels(pins: &mut [((f64, f64), String)]) {
    let prefix = |l: &str| l.find('_').map(|i| l[..=i].to_string());
    let mut counts: std::collections::BTreeMap<String, usize> = Default::default();
    for (_, l) in pins.iter() {
        if let Some(p) = prefix(l) {
            *counts.entry(p).or_default() += 1;
        }
    }
    for (_, l) in pins.iter_mut() {
        if let Some(p) = prefix(l).filter(|p| counts.get(p).copied().unwrap_or(0) >= 2) {
            let rest = &l[p.len()..];
            if !rest.is_empty() {
                *l = rest.to_string();
            }
        }
    }
}

/// A rounded caption badge beside a part, trying the inward side first.
fn place_badge(
    space: &mut Space<'_>,
    out: &mut PolishPlan,
    owner: &str,
    caption: &str,
    body: Rect,
    inward: f64,
    vertical: bool,
) -> bool {
    let size = 1.0;
    let tb = text_box(caption, (0.0, 0.0), size, 0.0);
    let (w, h) = (tb.2 - tb.0 + 0.6, tb.3 - tb.1 + 0.3);
    let (cx, cy) = ((body.0 + body.2) / 2.0, (body.1 + body.3) / 2.0);
    let gap = 0.4;
    let below = (cx - w / 2.0, body.3 + gap, cx + w / 2.0, body.3 + gap + h);
    let above = (cx - w / 2.0, body.1 - gap - h, cx + w / 2.0, body.1 - gap);
    let right = (body.2 + gap, cy - h / 2.0, body.2 + gap + w, cy + h / 2.0);
    let left = (body.0 - gap - w, cy - h / 2.0, body.0 - gap, cy + h / 2.0);
    let order = match (vertical, inward > 0.0) {
        (false, true) => [below, above, right, left],
        (false, false) => [above, below, right, left],
        (true, true) => [right, left, below, above],
        (true, false) => [left, right, below, above],
    };
    for r in order {
        if space.free(r, false, Some(owner)) {
            out.graphics.push(GraphicItem {
                layer: "F.SilkS".into(),
                a: (r.0, r.1),
                b: (r.2, r.3),
                frame: true,
                radius: FRAME_R,
                width: FRAME_W,
                purpose: format!("{owner} caption badge"),
            });
            out.texts.push(TextItem {
                layer: "F.SilkS".into(),
                text: caption.to_uppercase(),
                at: ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0),
                size,
                rotation: 0.0,
                mirror: false,
                knockout: false,
                bold: true,
                condensed: false,
                purpose: format!("{owner} caption"),
            });
            space.taken.push((r, false));
            return true;
        }
    }
    false
}

/// Cell edges along the row: midway between pins, half a pitch past the ends.
fn cell_edges(row: &Row) -> Vec<f64> {
    let t: Vec<f64> = row
        .pins
        .iter()
        .map(|(p, _)| if row.vertical { p.1 } else { p.0 })
        .collect();
    let first_pitch = t.get(1).map(|x| x - t[0]).unwrap_or(2.54);
    let last_pitch = if t.len() > 1 {
        t[t.len() - 1] - t[t.len() - 2]
    } else {
        2.54
    };
    let mut e = vec![t[0] - first_pitch / 2.0];
    for w in t.windows(2) {
        e.push((w[0] + w[1]) / 2.0);
    }
    e.push(t[t.len() - 1] + last_pitch / 2.0);
    e
}

/// The strip and caption boxes for one side, and each label's size and
/// rotation: (box, (text size, text rotation)). The first entry is the strip.
fn strip_plan(
    row: &Row,
    side: f64,
    size: f64,
    back: bool,
    caption: bool,
) -> Option<Vec<(Rect, (f64, f64))>> {
    let edges = cell_edges(row);
    let narrowest = edges
        .windows(2)
        .map(|w| w[1] - w[0])
        .fold(f64::MAX, f64::min);
    let longest = row
        .pins
        .iter()
        .map(|(_, l)| l.chars().count() as f64 * size * 0.64 + size * 0.3)
        .fold(0.0, f64::max);
    // Text runs across the strip when it fits a cell, otherwise along it.
    let (depth, rotation) = if row.vertical {
        (longest + 0.5, 0.0)
    } else if longest + 0.2 <= narrowest {
        (size * 1.3 + 0.4, 0.0)
    } else if size * 1.3 + 0.3 <= narrowest {
        (longest + 0.5, 90.0)
    } else {
        return None;
    };
    let base = if back { row.pads_only } else { row.body };
    let gap = 0.3;
    let near = if row.vertical {
        if side > 0.0 {
            base.2 + gap
        } else {
            base.0 - gap
        }
    } else if side > 0.0 {
        base.3 + gap
    } else {
        base.1 - gap
    };
    let far = near + side * depth;
    let (lo, hi) = (near.min(far), near.max(far));
    let (a0, a1) = (edges[0], edges[edges.len() - 1]);
    let strip: Rect = if row.vertical {
        (lo, a0, hi, a1)
    } else {
        (a0, lo, a1, hi)
    };
    let mut plan = vec![(strip, (size, rotation))];
    if let Some(c) = row.caption.as_ref().filter(|_| caption) {
        let csize = 1.0;
        let tb = text_box(c, (0.0, 0.0), csize, 0.0);
        let (w, h) = (tb.2 - tb.0 + 0.6, tb.3 - tb.1 + 0.3);
        let along = (a0 + a1) / 2.0;
        let start = far + side * 0.4;
        let end = start + side * if row.vertical { w } else { h };
        let (c0, c1) = (start.min(end), start.max(end));
        let badge: Rect = if row.vertical {
            (c0, along - h / 2.0, c1, along + h / 2.0)
        } else {
            (along - w / 2.0, c0, along + w / 2.0, c1)
        };
        plan.push((badge, (csize, 0.0)));
    }
    Some(plan)
}

#[allow(clippy::too_many_arguments)]
fn commit_strip(
    row: &Row,
    side: f64,
    size: f64,
    back: bool,
    caption: bool,
    space: &mut Space<'_>,
    out: &mut PolishPlan,
) {
    let Some(plan) = strip_plan(row, side, size, back, caption) else {
        return;
    };
    let layer = if back { "B.SilkS" } else { "F.SilkS" };
    let (strip, (tsize, rotation)) = plan[0];
    out.graphics.push(GraphicItem {
        layer: layer.into(),
        a: (strip.0, strip.1),
        b: (strip.2, strip.3),
        frame: true,
        radius: FRAME_R,
        width: FRAME_W,
        purpose: format!("{} pin strip", row.owner),
    });
    let edges = cell_edges(row);
    for e in &edges[1..edges.len() - 1] {
        let (a, b) = if row.vertical {
            ((strip.0, *e), (strip.2, *e))
        } else {
            ((*e, strip.1), (*e, strip.3))
        };
        out.graphics.push(GraphicItem {
            layer: layer.into(),
            a,
            b,
            frame: false,
            radius: 0.0,
            width: FRAME_W,
            purpose: format!("{} pin divider", row.owner),
        });
    }
    for (i, (_, label)) in row.pins.iter().enumerate() {
        let c = (edges[i] + edges[i + 1]) / 2.0;
        let at = if row.vertical {
            ((strip.0 + strip.2) / 2.0, c)
        } else {
            (c, (strip.1 + strip.3) / 2.0)
        };
        out.texts.push(TextItem {
            layer: layer.into(),
            text: label.clone(),
            at,
            size: tsize,
            rotation,
            mirror: back,
            knockout: false,
            bold: true,
            condensed: true,
            purpose: format!("{} pin {} label", row.owner, i + 1),
        });
    }
    space.taken.push((strip, back));
    if let (Some(caption), Some((badge, (csize, _)))) = (&row.caption, plan.get(1)) {
        out.graphics.push(GraphicItem {
            layer: layer.into(),
            a: (badge.0, badge.1),
            b: (badge.2, badge.3),
            frame: true,
            radius: FRAME_R,
            width: FRAME_W,
            purpose: format!("{} caption badge", row.owner),
        });
        out.texts.push(TextItem {
            layer: layer.into(),
            text: caption.clone(),
            at: ((badge.0 + badge.2) / 2.0, (badge.1 + badge.3) / 2.0),
            size: *csize,
            rotation: 0.0,
            mirror: back,
            knockout: false,
            bold: true,
            condensed: false,
            purpose: format!("{} caption", row.owner),
        });
        space.taken.push((*badge, back));
    }
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

    /// Prints a polish plan as JSON: VOLTARIS_POLISH_PREVIEW="board.kicad_pcb"
    /// and optional VOLTARIS_POLISH_CAPTIONS="J2=MOTOR A,J3=MOTOR B".
    #[test]
    #[ignore = "needs VOLTARIS_POLISH_PREVIEW"]
    fn polish_preview() {
        let Ok(path) = std::env::var("VOLTARIS_POLISH_PREVIEW") else {
            return;
        };
        let b = Board::from_text(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let captions = std::env::var("VOLTARIS_POLISH_CAPTIONS")
            .unwrap_or_default()
            .split(',')
            .filter_map(|kv| kv.split_once('='))
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        let p = plan(
            &b,
            "preview",
            &PolishOptions {
                captions,
                stitch: false,
                ..Default::default()
            },
        )
        .unwrap();
        let texts: Vec<serde_json::Value> = p
            .texts
            .iter()
            .map(|t| {
                serde_json::json!([t.layer, t.text, t.at.0, t.at.1, t.size, t.rotation, t.knockout])
            })
            .collect();
        let graphics: Vec<serde_json::Value> = p
            .graphics
            .iter()
            .map(|g| serde_json::json!([g.layer, g.a.0, g.a.1, g.b.0, g.b.1, g.frame]))
            .collect();
        println!(
            "PREVIEW {}",
            serde_json::json!({"texts": texts, "graphics": graphics, "notes": p.notes})
        );
    }

    #[test]
    fn frames_a_connector_row_and_captions_it() {
        let body = fixture::header("J1", 112.0, 106.0, &["+5V", "GND", "SIG"]);
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let opts = PolishOptions {
            captions: vec![("J1".into(), "Sensor".into())],
            ..Default::default()
        };
        let p = plan(&b, "x", &opts).unwrap();
        let frames = p
            .graphics
            .iter()
            .filter(|g| g.frame && g.layer == "F.SilkS")
            .count();
        let dividers = p
            .graphics
            .iter()
            .filter(|g| !g.frame && g.layer == "F.SilkS")
            .count();
        assert_eq!(frames, 2, "pin strip and caption badge");
        assert_eq!(dividers, 2, "three cells");
        assert!(p.texts.iter().any(|t| t.text == "SENSOR" && t.bold));
        for t in p.texts.iter().filter(|t| t.condensed) {
            for q in &b.pads {
                assert!(!rects_overlap(
                    text_box(&t.text, t.at, t.size * 0.75, t.rotation),
                    q.bbox(),
                    0.0
                ));
            }
        }
        let plain = plan(
            &b,
            "x",
            &PolishOptions {
                style: LabelStyle::Plain,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(plain.graphics.is_empty());
    }

    #[test]
    fn shortens_shared_pin_prefixes() {
        let mut pins = vec![
            ((0.0, 0.0), "MA_1A".to_string()),
            ((2.0, 0.0), "MA_2A".to_string()),
            ((4.0, 0.0), "GND".to_string()),
        ];
        shorten_labels(&mut pins);
        let names: Vec<&str> = pins.iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(names, ["1A", "2A", "GND"]);
    }

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
