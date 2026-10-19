//! Reference designators the way product boards print them: one size and
//! stroke for the whole board (JLCPCB legible: 1.0 mm, never under 0.8 mm),
//! upright text that reads left to right or bottom to top, beside its own part
//! and never on a pad, a via, another part's body or other silkscreen. A
//! designator with no clean spot is hidden rather than printed over copper.

use super::model::{rects_overlap, Board, Footprint, Rect};
use super::polish::text_box;

#[derive(Debug, Clone)]
pub struct SilkOptions {
    pub size: f64,
    pub min_size: f64,
    pub pad_gap: f64,
    pub gap: f64,
    pub edge: f64,
}

impl Default for SilkOptions {
    fn default() -> Self {
        Self {
            size: 1.0,
            min_size: 0.8,
            pad_gap: 0.2,
            gap: 0.15,
            edge: 0.3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RefPlacement {
    pub reference: String,
    pub at: (f64, f64),
    pub size: f64,
    pub stroke: f64,
    pub rotation: f64,
    pub layer: String,
    pub mirrored: bool,
    pub visible: bool,
    pub spot: &'static str,
}

/// Stroke for a glyph height: a sixth of it (JLCPCB's recommended ratio), at
/// least 0.15 mm.
pub fn stroke_for(size: f64) -> f64 {
    ((size / 6.0).max(0.15) * 1000.0).round() / 1000.0
}

fn shrink(r: Rect, by: f64) -> Rect {
    (r.0 + by, r.1 + by, r.2 - by, r.3 - by)
}

/// Everything a designator must not touch on one side of the board.
pub struct SilkSpace<'a> {
    board: &'a Board,
    inside: Rect,
    pub taken: Vec<(Rect, bool)>,
    opts: SilkOptions,
}

impl<'a> SilkSpace<'a> {
    pub fn new(board: &'a Board, taken: Vec<(Rect, bool)>, opts: &SilkOptions) -> Self {
        let o = board.outline.unwrap_or_default();
        Self {
            board,
            inside: (
                o.0 + opts.edge,
                o.1 + opts.edge,
                o.2 - opts.edge,
                o.3 - opts.edge,
            ),
            taken,
            opts: opts.clone(),
        }
    }

    /// Why `r` is not a clean spot for silkscreen on this side, if it is not.
    pub fn conflict(&self, r: Rect, back: bool, own: &str) -> Option<String> {
        if r.0 < self.inside.0 || r.1 < self.inside.1 || r.2 > self.inside.2 || r.3 > self.inside.3
        {
            return Some("too close to the board edge".into());
        }
        for p in &self.board.pads {
            let on_side = p.through_hole || if back { p.back } else { p.front };
            if on_side && rects_overlap(r, p.bbox(), self.opts.pad_gap) {
                return Some(format!("on pad {}.{}", p.reference, p.number));
            }
        }
        for v in &self.board.vias {
            let h = v.size / 2.0;
            if rects_overlap(r, (v.at.0 - h, v.at.1 - h, v.at.0 + h, v.at.1 + h), 0.1) {
                return Some("on a via".into());
            }
        }
        for f in &self.board.footprints {
            if f.back != back && !has_through_hole(self.board, f) {
                continue;
            }
            if f.reference != own && rects_overlap(r, shrink(f.courtyard, 0.25), 0.0) {
                return Some(format!("under {}", f.reference));
            }
            if f.back == back && f.silk.iter().any(|s| rects_overlap(r, *s, 0.1)) {
                return Some(format!("on {}'s outline", f.reference));
            }
        }
        if self
            .taken
            .iter()
            .any(|(t, b)| *b == back && rects_overlap(r, *t, self.opts.gap))
        {
            return Some("on other silkscreen text".into());
        }
        None
    }
}

fn has_through_hole(board: &Board, f: &Footprint) -> bool {
    f.pads.iter().any(|&i| board.pads[i].through_hole)
}

/// Candidate spots around a courtyard, best first: above, left, below,
/// right, then turned 90° (reading bottom to top) on the left and right.
fn candidates(c: Rect, text: &str, size: f64, gap: f64) -> Vec<((f64, f64), f64, &'static str)> {
    let flat = text_box(text, (0.0, 0.0), size, 0.0);
    let (w, h) = (flat.2 - flat.0, flat.3 - flat.1);
    let (cx, cy) = ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0);
    let wide = (c.2 - c.0) >= (c.3 - c.1);
    let above = ((cx, c.1 - gap - h / 2.0), 0.0, "above");
    let below = ((cx, c.3 + gap + h / 2.0), 0.0, "below");
    let left = ((c.0 - gap - w / 2.0, cy), 0.0, "left");
    let right = ((c.2 + gap + w / 2.0, cy), 0.0, "right");
    let left_up = ((c.0 - gap - h / 2.0, cy), 90.0, "left, turned");
    let right_up = ((c.2 + gap + h / 2.0, cy), 90.0, "right, turned");
    let mut out = if wide {
        vec![above, below, left, right, left_up, right_up]
    } else {
        vec![left_up, right_up, above, below, left, right]
    };
    for (dx, dy, name) in [
        (-1.0, -1.0, "above left"),
        (1.0, -1.0, "above right"),
        (-1.0, 1.0, "below left"),
        (1.0, 1.0, "below right"),
    ] {
        let x = if dx < 0.0 {
            c.0 + w / 2.0
        } else {
            c.2 - w / 2.0
        };
        let y = if dy < 0.0 {
            c.1 - gap - h / 2.0
        } else {
            c.3 + gap + h / 2.0
        };
        out.push(((x, y), 0.0, name));
    }
    out
}

/// Plans a tidy designator for every part. `taken` holds silkscreen already
/// planned (pin labels, the name block) so designators avoid it.
pub fn tidy_references(
    board: &Board,
    taken: Vec<(Rect, bool)>,
    opts: &SilkOptions,
) -> (Vec<RefPlacement>, Vec<String>) {
    let mut space = SilkSpace::new(board, taken, opts);
    let mut notes = Vec::new();
    let mut parts: Vec<&Footprint> = board
        .footprints
        .iter()
        .filter(|f| !f.reference.is_empty() && !f.is_mechanical() && f.ref_text.is_some())
        .filter(|f| {
            board
                .outline
                .is_some_and(|o| rects_overlap(f.courtyard, o, 0.0))
        })
        .collect();
    let freedom = |f: &Footprint, space: &SilkSpace<'_>| {
        candidates(f.courtyard, &f.reference, opts.size, opts.gap)
            .iter()
            .filter(|(at, rot, _)| {
                space
                    .conflict(
                        text_box(&f.reference, *at, opts.size, *rot),
                        f.back,
                        &f.reference,
                    )
                    .is_none()
            })
            .count()
    };
    parts.sort_by_key(|f| (freedom(f, &space), f.reference.clone()));
    let mut out = Vec::new();
    let mut hidden = Vec::new();
    let mut done: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // Rows and columns of identical parts get their designators on the same
    // side, so the silkscreen reads as an ordered array, not a scatter.
    for group in aligned_groups(&parts) {
        'spots: for k in 0..candidates((0.0, 0.0, 1.0, 1.0), "R1", opts.size, opts.gap).len() {
            for size in [opts.size, opts.min_size] {
                let mut planned = Vec::new();
                let mut trial = Vec::new();
                for f in &group {
                    let (at, rot, spot) = candidates(f.courtyard, &f.reference, size, opts.gap)[k];
                    let r = text_box(&f.reference, at, size, rot);
                    let clash = trial
                        .iter()
                        .any(|(t, b)| *b == f.back && rects_overlap(r, *t, opts.gap));
                    if clash || space.conflict(r, f.back, &f.reference).is_some() {
                        planned.clear();
                        break;
                    }
                    trial.push((r, f.back));
                    planned.push(placement(f, at, rot, size, spot, true));
                }
                if planned.len() == group.len() {
                    space.taken.extend(trial);
                    for p in planned {
                        done.insert(p.reference.clone());
                        out.push(p);
                    }
                    break 'spots;
                }
            }
        }
    }

    for f in parts.iter().filter(|f| !done.contains(&f.reference)) {
        let mut chosen = None;
        for size in [opts.size, opts.min_size] {
            for (at, rot, spot) in candidates(f.courtyard, &f.reference, size, opts.gap) {
                let r = text_box(&f.reference, at, size, rot);
                if space.conflict(r, f.back, &f.reference).is_none() {
                    chosen = Some((at, rot, size, spot, r));
                    break;
                }
            }
            if chosen.is_some() {
                break;
            }
        }
        match chosen {
            Some((at, rot, size, spot, r)) => {
                space.taken.push((r, f.back));
                out.push(placement(f, at, rot, size, spot, true));
            }
            None => {
                hidden.push(f.reference.clone());
                let current = f.ref_text.clone().expect("filtered on ref_text");
                out.push(placement(
                    f,
                    current.at,
                    0.0,
                    opts.min_size,
                    "hidden",
                    false,
                ));
            }
        }
    }
    if !hidden.is_empty() {
        notes.push(format!(
            "no clean spot for {} designator(s), hidden (they stay in the BOM and assembly drawing): {}",
            hidden.len(),
            hidden.join(", ")
        ));
    }
    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    (out, notes)
}

fn placement(
    f: &Footprint,
    at: (f64, f64),
    rotation: f64,
    size: f64,
    spot: &'static str,
    visible: bool,
) -> RefPlacement {
    RefPlacement {
        reference: f.reference.clone(),
        at: (super::model::round3(at.0), super::model::round3(at.1)),
        size,
        stroke: stroke_for(size),
        rotation,
        layer: if f.back { "B.SilkS" } else { "F.SilkS" }.into(),
        mirrored: f.back,
        visible,
        spot,
    }
}

/// Runs of three or more identical, equally turned parts lined up in a row or
/// a column with no big gap between neighbours. Larger runs come first.
fn aligned_groups<'a>(parts: &[&'a Footprint]) -> Vec<Vec<&'a Footprint>> {
    let centre = |f: &Footprint| {
        (
            (f.courtyard.0 + f.courtyard.2) / 2.0,
            (f.courtyard.1 + f.courtyard.3) / 2.0,
        )
    };
    let mut groups: Vec<Vec<&'a Footprint>> = Vec::new();
    for column in [true, false] {
        let mut buckets: Vec<Vec<&'a Footprint>> = Vec::new();
        for f in parts {
            let c = centre(f);
            let key = |g: &Footprint| {
                g.library == f.library
                    && g.back == f.back
                    && ((g.rotation - f.rotation).rem_euclid(180.0)).abs() < 1.0
                    && if column {
                        (centre(g).0 - c.0).abs() < 0.3
                    } else {
                        (centre(g).1 - c.1).abs() < 0.3
                    }
            };
            match buckets.iter_mut().find(|b| key(b[0])) {
                Some(b) => b.push(f),
                None => buckets.push(vec![f]),
            }
        }
        for mut b in buckets {
            b.sort_by(|x, y| {
                let (a, c) = (centre(x), centre(y));
                let (p, q) = if column { (a.1, c.1) } else { (a.0, c.0) };
                p.partial_cmp(&q).unwrap_or(std::cmp::Ordering::Equal)
            });
            let mut run: Vec<&'a Footprint> = Vec::new();
            for f in b {
                if let Some(last) = run.last() {
                    let span = if column {
                        last.courtyard.3 - last.courtyard.1
                    } else {
                        last.courtyard.2 - last.courtyard.0
                    };
                    let gap = if column {
                        f.courtyard.1 - last.courtyard.3
                    } else {
                        f.courtyard.0 - last.courtyard.2
                    };
                    if gap > span.max(1.0) * 1.5 {
                        if run.len() >= 3 {
                            groups.push(std::mem::take(&mut run));
                        }
                        run.clear();
                    }
                }
                run.push(f);
            }
            if run.len() >= 3 {
                groups.push(run);
            }
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.len()));
    let mut seen = std::collections::BTreeSet::new();
    groups
        .into_iter()
        .filter_map(|g| {
            let g: Vec<&Footprint> = g
                .into_iter()
                .filter(|f| !seen.contains(&f.reference))
                .collect();
            if g.len() < 3 {
                return None;
            }
            for f in &g {
                seen.insert(f.reference.clone());
            }
            Some(g)
        })
        .collect()
}

/// Problems with the designators as they are printed now.
pub fn reference_findings(board: &Board) -> Vec<(String, &'static str, String)> {
    let opts = SilkOptions::default();
    let mut out = Vec::new();
    let shown: Vec<(&Footprint, Rect)> = board
        .footprints
        .iter()
        .filter_map(|f| {
            let t = f.ref_text.as_ref()?;
            (t.visible && !f.is_mechanical())
                .then(|| (f, text_box(&f.reference, t.at, t.size, t.rotation)))
        })
        .collect();
    let space = SilkSpace::new(
        board,
        Vec::new(),
        &SilkOptions {
            pad_gap: 0.15,
            ..opts
        },
    );
    for (f, r) in &shown {
        let t = f.ref_text.as_ref().expect("shown has text");
        if t.size < 0.8 - 1e-6 || t.stroke < 0.15 - 1e-6 {
            out.push((
                f.reference.clone(),
                "designator_too_small",
                format!(
                    "{} is {:.2} mm high with a {:.2} mm stroke (JLCPCB minimum 0.8 / 0.15)",
                    f.reference, t.size, t.stroke
                ),
            ));
        }
        let angle = t.rotation.rem_euclid(360.0);
        let readable = if f.back {
            angle < 1.0 || (angle - 90.0).abs() < 1.0 || (angle - 270.0).abs() < 1.0
        } else {
            angle < 1.0 || (angle - 90.0).abs() < 1.0
        };
        if !readable {
            out.push((
                f.reference.clone(),
                "upside_down_text",
                format!(
                    "{} reads at {angle:.0}°; text reads left to right or bottom to top only",
                    f.reference
                ),
            ));
        }
        if let Some(why) = space.conflict(*r, f.back, &f.reference) {
            if why.starts_with("on pad") || why.starts_with("on a via") || why.starts_with("under")
            {
                out.push((
                    f.reference.clone(),
                    "designator_on_copper",
                    format!("{} is {why}", f.reference),
                ));
            }
        }
        let d = super::model::rect_distance(f.courtyard, t.at);
        if d > 3.0 {
            out.push((
                f.reference.clone(),
                "designator_far_from_part",
                format!("{} is {d:.1} mm from its part", f.reference),
            ));
        }
    }
    for i in 0..shown.len() {
        for j in i + 1..shown.len() {
            let (a, ra) = shown[i];
            let (b, rb) = shown[j];
            if a.back == b.back && rects_overlap(ra, rb, 0.0) {
                out.push((
                    a.reference.clone(),
                    "designators_overlap",
                    format!("{} and {} overlap", a.reference, b.reference),
                ));
            }
        }
    }
    let sizes: std::collections::BTreeSet<i64> = shown
        .iter()
        .filter_map(|(f, _)| f.ref_text.as_ref().map(|t| (t.size * 100.0).round() as i64))
        .collect();
    if sizes.len() > 2 {
        out.push((
            String::new(),
            "mixed_designator_sizes",
            format!(
                "designators use {} different sizes; product boards use one",
                sizes.len()
            ),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::model::{fixture, Board};
    use super::*;

    fn row() -> Board {
        let mut body = String::new();
        for (i, x) in [106.0, 110.0, 114.0, 118.0].iter().enumerate() {
            body.push_str(&fixture::smd(
                &format!("R{}", i + 1),
                *x,
                110.0,
                0.0,
                ("SIG", "GND"),
            ));
        }
        body.push_str(&fixture::header("J1", 126.0, 104.0, &["+5V", "GND", "SIG"]));
        Board::from_text(&fixture::board(&body)).unwrap()
    }

    #[test]
    fn places_every_designator_upright_off_copper_and_apart() {
        let b = row();
        let (refs, notes) = tidy_references(&b, Vec::new(), &SilkOptions::default());
        assert_eq!(refs.len(), 5, "{notes:?}");
        let space = SilkSpace::new(&b, Vec::new(), &SilkOptions::default());
        let boxes: Vec<Rect> = refs
            .iter()
            .map(|r| text_box(&r.reference, r.at, r.size, r.rotation))
            .collect();
        for (r, bx) in refs.iter().zip(&boxes) {
            assert!(r.visible, "{} hidden: {notes:?}", r.reference);
            assert!(r.size >= 0.8 && r.stroke >= 0.15);
            assert!(r.rotation == 0.0 || r.rotation == 90.0);
            assert_eq!(
                space.conflict(*bx, false, &r.reference),
                None,
                "{}",
                r.reference
            );
            let f = b.footprint(&r.reference).unwrap();
            assert!(super::super::model::rect_distance(f.courtyard, r.at) < 2.0);
        }
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                assert!(
                    !rects_overlap(boxes[i], boxes[j], 0.1),
                    "{} / {}",
                    refs[i].reference,
                    refs[j].reference
                );
            }
        }
        assert!(refs
            .iter()
            .filter(|r| r.reference.starts_with('R'))
            .all(|r| r.spot == "above"));
    }

    #[test]
    fn flags_designators_on_pads_and_upside_down() {
        let mut body = fixture::smd("R1", 110.0, 110.0, 0.0, ("SIG", "GND"));
        body = body.replace(
            "(property \"Reference\" \"R1\" (at 0 -1.5 0)",
            "(property \"Reference\" \"R1\" (at 0.9 0 180)",
        );
        let b = Board::from_text(&fixture::board(&body)).unwrap();
        let checks: Vec<&str> = reference_findings(&b).iter().map(|f| f.1).collect();
        assert!(checks.contains(&"designator_on_copper"), "{checks:?}");
        assert!(checks.contains(&"upside_down_text"), "{checks:?}");
    }
}

#[cfg(test)]
mod corpus {
    use super::super::model::Board;
    use super::*;

    #[test]
    fn tidies_a_real_board() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../voltaris-sexp/tests/fixtures/pic_programmer.kicad_pcb");
        let b = Board::from_text(&std::fs::read_to_string(path).unwrap()).unwrap();
        let before = reference_findings(&b);
        let (refs, notes) = tidy_references(&b, Vec::new(), &SilkOptions::default());
        let shown = refs.iter().filter(|r| r.visible).count();
        eprintln!(
            "pic_programmer: {} findings before; {shown}/{} designators placed; {notes:?}",
            before.len(),
            refs.len()
        );
        let space = SilkSpace::new(&b, Vec::new(), &SilkOptions::default());
        let mut boxes = Vec::new();
        for r in refs.iter().filter(|r| r.visible) {
            let bx = text_box(&r.reference, r.at, r.size, r.rotation);
            let back = r.layer.starts_with("B.");
            assert_eq!(
                space.conflict(bx, back, &r.reference),
                None,
                "{}",
                r.reference
            );
            assert!(
                !boxes
                    .iter()
                    .any(|(o, ob)| *ob == back && rects_overlap(bx, *o, 0.1)),
                "{} overlaps",
                r.reference
            );
            boxes.push((bx, back));
        }
        assert!(
            shown * 10 >= refs.len() * 8,
            "only {shown} of {} shown",
            refs.len()
        );
        if let Ok(dir) = std::env::var("VOLTARIS_SVG_DIR") {
            let o = b.outline.unwrap();
            let mut svg = format!(
                "<svg xmlns='http://www.w3.org/2000/svg' viewBox='{} {} {} {}' width='{}' height='{}'><rect x='{}' y='{}' width='{}' height='{}' fill='#0b3d2e'/>",
                o.0, o.1, o.2 - o.0, o.3 - o.1, (o.2 - o.0) * 12.0, (o.3 - o.1) * 12.0, o.0, o.1, o.2 - o.0, o.3 - o.1
            );
            for f in &b.footprints {
                for s in &f.silk {
                    svg.push_str(&format!("<rect x='{}' y='{}' width='{}' height='{}' fill='none' stroke='#eee' stroke-width='0.12'/>", s.0, s.1, s.2 - s.0, s.3 - s.1));
                }
            }
            for p in b.pads.iter().filter(|p| p.front || p.through_hole) {
                let bb = p.bbox();
                svg.push_str(&format!(
                    "<rect x='{}' y='{}' width='{}' height='{}' fill='#c8a85a'/>",
                    bb.0,
                    bb.1,
                    bb.2 - bb.0,
                    bb.3 - bb.1
                ));
            }
            for r in refs.iter().filter(|r| r.visible && !r.mirrored) {
                svg.push_str(&format!(
                    "<text x='{}' y='{}' font-family='monospace' font-size='{}' fill='white' text-anchor='middle' dominant-baseline='central' transform='rotate({} {} {})'>{}</text>",
                    r.at.0, r.at.1, r.size * 1.2, -r.rotation, r.at.0, r.at.1, r.reference
                ));
            }
            svg.push_str("</svg>");
            std::fs::write(format!("{dir}/silk.svg"), svg).unwrap();
        }
    }
}
