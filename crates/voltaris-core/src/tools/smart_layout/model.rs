//! A geometric snapshot of a board, read from KiCad's own S-expression (the live
//! board serialized over IPC, or the saved file), shared by the router, the
//! placement planner, the polisher and the auditor.

use std::collections::{BTreeMap, BTreeSet};
use voltaris_sexp::board::{
    board_outline_bbox, board_outline_shape, footprint_courtyards, footprints, track_arcs, tracks,
    vias, zones, OutlineShape, PcbConnectivityIndex,
};
use voltaris_sexp::geometry::transform_pad;
use voltaris_sexp::parser::SexpNode;

pub type Rect = (f64, f64, f64, f64);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PadShape {
    Circle,
    Rect,
}

#[derive(Debug, Clone)]
pub struct Pad {
    pub reference: String,
    pub number: String,
    pub net: Option<String>,
    pub at: (f64, f64),
    pub local: (f64, f64),
    pub size: (f64, f64),
    pub rotation: f64,
    pub shape: PadShape,
    pub front: bool,
    pub back: bool,
    pub through_hole: bool,
}

impl Pad {
    pub fn on_layer(&self, layer: &str) -> bool {
        match layer {
            "F.Cu" => self.front,
            "B.Cu" => self.back,
            _ => self.through_hole,
        }
    }

    pub fn half_extent(&self) -> (f64, f64) {
        let (w, h) = self.size;
        let r = self.rotation.to_radians();
        let (c, s) = (r.cos().abs(), r.sin().abs());
        ((w * c + h * s) / 2.0, (w * s + h * c) / 2.0)
    }

    pub fn bbox(&self) -> Rect {
        let (hx, hy) = self.half_extent();
        (
            self.at.0 - hx,
            self.at.1 - hy,
            self.at.0 + hx,
            self.at.1 + hy,
        )
    }

    pub fn distance_to(&self, p: (f64, f64)) -> f64 {
        match self.shape {
            PadShape::Circle => {
                let r = self.size.0.min(self.size.1) / 2.0;
                (dist(self.at, p) - r).max(0.0)
            }
            PadShape::Rect => rect_distance(self.bbox(), p),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Footprint {
    pub reference: String,
    pub library: String,
    pub at: (f64, f64),
    pub rotation: f64,
    pub back: bool,
    pub locked: bool,
    pub courtyard: Rect,
    pub pads: Vec<usize>,
    /// The reference designator as printed: board position, absolute angle,
    /// glyph height, stroke, layer and whether it is shown.
    pub ref_text: Option<FieldText>,
    /// Bounding boxes of the footprint's own silkscreen graphics, in board coordinates.
    pub silk: Vec<Rect>,
}

#[derive(Debug, Clone)]
pub struct FieldText {
    pub at: (f64, f64),
    pub rotation: f64,
    pub size: f64,
    pub stroke: f64,
    pub layer: String,
    pub visible: bool,
    /// Horizontal (-1 left, 0 centre, 1 right) and vertical (-1 top, 0 centre,
    /// 1 bottom) justification around `at`.
    pub justify: (i8, i8),
}

impl FieldText {
    /// The area the printed text covers, the way KiCad draws footprint text:
    /// justified around its anchor and kept upright (text turned past 90° is
    /// drawn the other way round, which also swaps its justification).
    pub fn bbox(&self, text: &str, back: bool) -> Rect {
        let w = text.chars().count() as f64 * self.size * 0.85 + self.size * 0.3;
        let h = self.size * 1.3;
        let mut a = self.rotation.rem_euclid(360.0);
        let (mut jh, mut jv) = self.justify;
        if a > 90.0 + 1e-6 && a <= 270.0 + 1e-6 {
            a -= 180.0;
            jh = -jh;
            jv = -jv;
        }
        if back {
            jh = -jh;
        }
        let (x0, x1) = match jh {
            -1 => (0.0, w),
            1 => (-w, 0.0),
            _ => (-w / 2.0, w / 2.0),
        };
        let (y0, y1) = match jv {
            -1 => (0.0, h),
            1 => (-h, 0.0),
            _ => (-h / 2.0, h / 2.0),
        };
        let r = a.to_radians();
        let (c, s) = (r.cos(), r.sin());
        let mut out = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
            let p = (self.at.0 + x * c + y * s, self.at.1 - x * s + y * c);
            out = (
                out.0.min(p.0),
                out.1.min(p.1),
                out.2.max(p.0),
                out.3.max(p.1),
            );
        }
        out
    }
}

impl Footprint {
    pub fn prefix(&self) -> String {
        self.reference
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect::<String>()
            .to_ascii_uppercase()
    }

    pub fn local_courtyard(&self) -> Rect {
        let corners = [
            (self.courtyard.0, self.courtyard.1),
            (self.courtyard.2, self.courtyard.1),
            (self.courtyard.0, self.courtyard.3),
            (self.courtyard.2, self.courtyard.3),
        ];
        let mut out = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in corners {
            let (lx, ly) = transform_pad(x - self.at.0, y - self.at.1, 0.0, 0.0, -self.rotation);
            out = (out.0.min(lx), out.1.min(ly), out.2.max(lx), out.3.max(ly));
        }
        out
    }

    pub fn courtyard_at(&self, at: (f64, f64), rotation: f64) -> Rect {
        let l = self.local_courtyard();
        let corners = [(l.0, l.1), (l.2, l.1), (l.0, l.3), (l.2, l.3)];
        let mut out = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        for (x, y) in corners {
            let (bx, by) = transform_pad(x, y, at.0, at.1, rotation);
            out = (out.0.min(bx), out.1.min(by), out.2.max(bx), out.3.max(by));
        }
        out
    }

    pub fn is_connector(&self) -> bool {
        let p = self.prefix();
        matches!(p.as_str(), "J" | "P" | "CN" | "USB" | "X" | "CON")
            || self.library.to_ascii_lowercase().starts_with("connector")
    }

    pub fn is_mechanical(&self) -> bool {
        let p = self.prefix();
        matches!(p.as_str(), "H" | "MH" | "FID" | "LOGO" | "TP" | "G")
            || self.library.starts_with("MountingHole")
            || self.pads.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub net: Option<String>,
    pub layer: String,
    pub a: (f64, f64),
    pub b: (f64, f64),
    pub width: f64,
    /// A chord standing in for part of a curved track.
    pub from_arc: bool,
}

#[derive(Debug, Clone)]
pub struct Via {
    pub net: Option<String>,
    pub at: (f64, f64),
    pub size: f64,
}

#[derive(Debug, Clone)]
pub struct Zone {
    pub net: Option<String>,
    pub layers: Vec<String>,
    pub points: Vec<(f64, f64)>,
}

#[derive(Debug, Clone)]
pub struct Text {
    pub layer: String,
    pub at: (f64, f64),
    pub text: String,
    pub size: f64,
    pub rotation: f64,
    /// Inverted text: a solid silkscreen box with the letters cut out.
    pub knockout: bool,
}

#[derive(Debug, Clone)]
pub struct Board {
    pub outline: Option<Rect>,
    pub outline_rectangular: bool,
    pub copper_layers: Vec<String>,
    pub footprints: Vec<Footprint>,
    pub pads: Vec<Pad>,
    pub segments: Vec<Segment>,
    pub vias: Vec<Via>,
    pub zones: Vec<Zone>,
    pub texts: Vec<Text>,
    pub mounting_holes: usize,
    /// Rule areas (keep-outs): what may not be placed inside each polygon.
    pub keepouts: Vec<Keepout>,
}

#[derive(Debug, Clone, Default)]
pub struct Keepout {
    /// The footprint that carries this keep-out (an antenna module's, say);
    /// it moves with that part, so it never blocks the part itself.
    pub owner: Option<String>,
    pub layers: Vec<String>,
    pub points: Vec<(f64, f64)>,
    pub no_tracks: bool,
    pub no_vias: bool,
    pub no_pads: bool,
    pub no_footprints: bool,
}

impl Keepout {
    pub fn contains(&self, p: (f64, f64)) -> bool {
        point_in_polygon(p, &self.points)
    }
    pub fn on(&self, layer: &str) -> bool {
        self.layers.iter().any(|l| l == layer)
    }
}

fn keepouts(tree: &SexpNode, copper: &[String]) -> Vec<Keepout> {
    let mut zones: Vec<(Option<String>, &SexpNode)> = tree
        .find_all("zone")
        .into_iter()
        .map(|z| (None, z))
        .collect();
    for fp in footprints(tree) {
        let owner = reference_of(fp);
        zones.extend(fp.find_all("zone").into_iter().map(|z| (owner.clone(), z)));
    }
    let mut out = Vec::new();
    for (owner, zone) in zones {
        let Some(rules) = zone.find("keepout") else {
            continue;
        };
        let banned = |what: &str| rules.find_str(what) == Some("not_allowed");
        let layers: Vec<String> = match zone.find("layers").and_then(SexpNode::children) {
            Some(list) => list
                .iter()
                .skip(1)
                .filter_map(SexpNode::as_str)
                .map(str::to_string)
                .collect(),
            None => zone
                .find_str("layer")
                .map(str::to_string)
                .into_iter()
                .collect(),
        };
        let points: Vec<(f64, f64)> = zone
            .find("polygon")
            .and_then(|p| p.find("pts"))
            .map(|pts| {
                pts.find_all("xy")
                    .into_iter()
                    .filter_map(|xy| Some((xy.get_f64(1)?, xy.get_f64(2)?)))
                    .collect()
            })
            .unwrap_or_default();
        if points.len() < 3 {
            continue;
        }
        out.push(Keepout {
            owner,
            layers: expand_layers(&layers, copper),
            points,
            no_tracks: banned("tracks"),
            no_vias: banned("vias"),
            no_pads: banned("pads"),
            no_footprints: banned("footprints"),
        });
    }
    out
}

impl Board {
    pub fn from_text(content: &str) -> anyhow::Result<Self> {
        let tree = voltaris_sexp::parse_sexp(content)?;
        Ok(Self::from_tree(&tree))
    }

    pub fn from_tree(tree: &SexpNode) -> Self {
        let renamed = canonical_layer_names(tree);
        let tree = renamed.as_ref().unwrap_or(tree);
        let index = PcbConnectivityIndex::build(tree);
        let courtyards: BTreeMap<String, Rect> = footprint_courtyards(tree)
            .items
            .into_iter()
            .filter_map(|c| c.reference.map(|r| (r, c.bbox)))
            .collect();
        let mut pads = Vec::new();
        let mut fps = Vec::new();
        let mut mounting_holes = 0;
        for node in footprints(tree) {
            let Some((fx, fy, frot)) = root_at(node) else {
                continue;
            };
            let reference = reference_of(node).unwrap_or_default();
            let library = node
                .get(1)
                .and_then(SexpNode::as_str)
                .unwrap_or("")
                .to_string();
            let back = node.find_str("layer") == Some("B.Cu");
            let mut own = Vec::new();
            for pad in node.find_all("pad") {
                let Some(p) = parse_pad(pad, &reference, (fx, fy, frot), &index) else {
                    continue;
                };
                own.push(pads.len());
                pads.push(p);
            }
            if library.starts_with("MountingHole") {
                mounting_holes += 1;
            }
            let courtyard = courtyards.get(&reference).copied().unwrap_or_else(|| {
                own.iter().fold((fx, fy, fx, fy), |acc, &i| {
                    let b: Rect = pads[i].bbox();
                    (
                        acc.0.min(b.0),
                        acc.1.min(b.1),
                        acc.2.max(b.2),
                        acc.3.max(b.3),
                    )
                })
            });
            let ref_text = field_text(node, "Reference", (fx, fy, frot));
            let silk = silk_boxes(node, (fx, fy, frot));
            fps.push(Footprint {
                reference,
                library,
                at: (fx, fy),
                rotation: frot,
                back,
                locked: is_locked(node),
                courtyard,
                pads: own,
                ref_text,
                silk,
            });
        }
        let segments: Vec<Segment> = tracks(tree)
            .items
            .into_iter()
            .map(|t| Segment {
                net: t.net,
                layer: t.layer,
                a: (t.x1, t.y1),
                b: (t.x2, t.y2),
                width: t.width,
                from_arc: false,
            })
            .collect();
        // Curved tracks join the model as two chords (start → mid → end):
        // close enough for connectivity, clearance and corner checks.
        let mut segments: Vec<Segment> = segments;
        for a in track_arcs(tree).items {
            for (p, q) in [(a.start, a.mid), (a.mid, a.end)] {
                segments.push(Segment {
                    net: a.net.clone(),
                    layer: a.layer.clone(),
                    a: p,
                    b: q,
                    width: a.width,
                    from_arc: true,
                });
            }
        }
        let vias = vias(tree)
            .items
            .into_iter()
            .map(|v| Via {
                net: v.net,
                at: (v.x, v.y),
                size: v.size,
            })
            .collect();
        let copper = copper_layers(tree);
        let zones = zones(tree)
            .items
            .into_iter()
            .map(|z| Zone {
                net: z.net,
                layers: expand_layers(&z.layers, &copper),
                points: z.points,
            })
            .collect();
        let texts = tree
            .children()
            .unwrap_or_default()
            .iter()
            .filter(|n| n.head() == Some("gr_text"))
            .filter_map(|n| {
                let text = n.get(1)?.as_str()?.to_string();
                let at = n.find("at")?;
                let (size, _) = font_of(n);
                let knockout = n.find("layer").is_some_and(|l| {
                    l.children()
                        .unwrap_or_default()
                        .iter()
                        .any(|c| c.as_str() == Some("knockout"))
                });
                Some(Text {
                    knockout,
                    layer: n.find_str("layer").unwrap_or("").to_string(),
                    at: (at.get_f64(1)?, at.get_f64(2)?),
                    text,
                    size,
                    rotation: at.get_f64(3).unwrap_or(0.0),
                })
            })
            .collect();
        let shape = board_outline_shape(tree);
        Board {
            outline: board_outline_bbox(tree),
            outline_rectangular: matches!(shape, OutlineShape::Rectangular { .. }),
            copper_layers: copper_layers(tree),
            footprints: fps,
            pads,
            segments,
            vias,
            zones,
            texts,
            mounting_holes,
            keepouts: keepouts(tree, &copper),
        }
    }

    /// True when a part whose courtyard is `c` on this side would sit in a
    /// keep-out that bans footprints or pads there.
    pub fn keepout_blocks(&self, c: Rect, back: bool) -> bool {
        let side = if back { "B.Cu" } else { "F.Cu" };
        self.keepouts
            .iter()
            .filter(|k| k.owner.is_none() && (k.no_footprints || k.no_pads) && k.on(side))
            .any(|k| {
                let corners = [
                    (c.0, c.1),
                    (c.2, c.1),
                    (c.2, c.3),
                    (c.0, c.3),
                    ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0),
                ];
                corners.into_iter().any(|p| k.contains(p))
                    || k.points
                        .iter()
                        .any(|p| p.0 > c.0 && p.0 < c.2 && p.1 > c.1 && p.1 < c.3)
            })
    }

    pub fn footprint(&self, reference: &str) -> Option<&Footprint> {
        self.footprints.iter().find(|f| f.reference == reference)
    }

    pub fn nets(&self) -> BTreeSet<String> {
        self.pads.iter().filter_map(|p| p.net.clone()).collect()
    }

    pub fn poured_layers(&self, net: &str) -> Vec<String> {
        let mut out: Vec<String> = self
            .zones
            .iter()
            .filter(|z| z.net.as_deref() == Some(net))
            .flat_map(|z| z.layers.iter().cloned())
            .filter(|l| l.ends_with(".Cu"))
            .collect();
        out.sort();
        out.dedup();
        out
    }
}

/// KiCad 5 let designers rename copper layers ("Top", "GND", "Bottom") and
/// then wrote the new names on every item. Map them back to F.Cu / In<n>.Cu /
/// B.Cu by layer number; `None` when the board already uses canonical names.
fn canonical_layer_names(tree: &SexpNode) -> Option<SexpNode> {
    let layers = tree.find("layers")?.children()?;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for layer in layers.iter().skip(1) {
        let (Some(id), Some(name), Some(kind)) = (
            layer.head().and_then(|h| h.parse::<u32>().ok()),
            layer.get(1).and_then(SexpNode::as_str),
            layer.get(2).and_then(SexpNode::as_str),
        ) else {
            continue;
        };
        if name.ends_with(".Cu") || !matches!(kind, "signal" | "power" | "mixed" | "jumper") {
            continue;
        }
        let canonical = match id {
            0 => "F.Cu".to_string(),
            31 => "B.Cu".to_string(),
            n @ 1..=30 => format!("In{n}.Cu"),
            _ => continue,
        };
        map.insert(name.to_string(), canonical);
    }
    if map.is_empty() {
        return None;
    }
    fn walk(node: &SexpNode, map: &BTreeMap<String, String>, in_layer: bool) -> SexpNode {
        match node {
            SexpNode::Atom(a) if in_layer => {
                SexpNode::Atom(map.get(a).cloned().unwrap_or_else(|| a.clone()))
            }
            SexpNode::Str(a) if in_layer => {
                SexpNode::Str(map.get(a).cloned().unwrap_or_else(|| a.clone()))
            }
            SexpNode::List(items) => {
                let here = matches!(node.head(), Some("layer" | "layers"));
                SexpNode::List(items.iter().map(|c| walk(c, map, here)).collect())
            }
            other => other.clone(),
        }
    }
    let mut out = walk(tree, &map, false);
    if let SexpNode::List(items) = &mut out {
        for item in items.iter_mut() {
            if item.head() == Some("layers") {
                *item = renamed_layer_table(item, &map);
            }
        }
    }
    Some(out)
}

/// The top-level layer table with the copper entries renamed too.
fn renamed_layer_table(table: &SexpNode, map: &BTreeMap<String, String>) -> SexpNode {
    let SexpNode::List(items) = table else {
        return table.clone();
    };
    SexpNode::List(
        items
            .iter()
            .map(|entry| match entry {
                SexpNode::List(parts) if parts.len() >= 2 => {
                    let mut parts = parts.clone();
                    if let Some(new) = parts[1].as_str().and_then(|n| map.get(n)) {
                        parts[1] = SexpNode::Str(new.clone());
                    }
                    SexpNode::List(parts)
                }
                other => other.clone(),
            })
            .collect(),
    )
}

/// KiCad's layer shorthands: "F&B.Cu" is both outer layers, "*.Cu" every
/// copper layer.
fn expand_layers(layers: &[String], copper: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for l in layers {
        match l.as_str() {
            "F&B.Cu" => out.extend(["F.Cu".to_string(), "B.Cu".to_string()]),
            "*.Cu" => out.extend(copper.iter().cloned()),
            _ => out.push(l.clone()),
        }
    }
    out.sort();
    out.dedup();
    out
}

fn copper_layers(tree: &SexpNode) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(layers) = tree.find("layers").and_then(SexpNode::children) {
        for layer in layers.iter().skip(1) {
            if let Some(name) = layer.get(1).and_then(SexpNode::as_str) {
                if name.ends_with(".Cu") {
                    out.push(name.to_string());
                }
            }
        }
    }
    if out.is_empty() {
        out = vec!["F.Cu".into(), "B.Cu".into()];
    }
    out
}

fn root_at(fp: &SexpNode) -> Option<(f64, f64, f64)> {
    let at = fp.find("at")?;
    Some((at.get_f64(1)?, at.get_f64(2)?, at.get_f64(3).unwrap_or(0.0)))
}

fn property(fp: &SexpNode, name: &str) -> Option<String> {
    fp.find_all("property")
        .into_iter()
        .find(|p| p.get(1).and_then(SexpNode::as_str) == Some(name))
        .and_then(|p| p.get(2).and_then(SexpNode::as_str).map(str::to_string))
        .or_else(|| {
            let legacy = name.to_ascii_lowercase();
            fp.find_all("fp_text")
                .into_iter()
                .find(|t| t.get(1).and_then(SexpNode::as_str) == Some(legacy.as_str()))
                .and_then(|t| t.get(2).and_then(SexpNode::as_str).map(str::to_string))
        })
}

/// Glyph height and stroke of a text node's `(effects (font (size h w) (thickness t)))`.
fn font_of(node: &SexpNode) -> (f64, f64) {
    let font = node.find("effects").and_then(|e| e.find("font"));
    let size = font
        .and_then(|f| f.find("size"))
        .and_then(|s| s.get_f64(1))
        .unwrap_or(1.0);
    let stroke = font
        .and_then(|f| f.find("thickness"))
        .and_then(|t| t.get_f64(1))
        .unwrap_or(size * 0.15);
    (size, stroke)
}

fn hidden(node: &SexpNode) -> bool {
    let flag = |n: &SexpNode| {
        n.children().unwrap_or_default().iter().any(|c| {
            c.as_str() == Some("hide")
                || (c.head() == Some("hide") && c.get(1).and_then(SexpNode::as_str) != Some("no"))
        })
    };
    flag(node) || node.find("effects").is_some_and(flag)
}

fn field_text(fp: &SexpNode, name: &str, (fx, fy, frot): (f64, f64, f64)) -> Option<FieldText> {
    let node = fp
        .find_all("property")
        .into_iter()
        .find(|p| p.get(1).and_then(SexpNode::as_str) == Some(name))
        .or_else(|| {
            let legacy = name.to_ascii_lowercase();
            fp.find_all("fp_text")
                .into_iter()
                .find(|t| t.get(1).and_then(SexpNode::as_str) == Some(legacy.as_str()))
        })?;
    // Designers often keep the Reference field on the fab layer and print a
    // `${REFERENCE}` text on the silkscreen instead; the printed one counts.
    let on_silk = |n: &SexpNode| n.find_str("layer").is_some_and(|l| l.ends_with(".SilkS"));
    let node = if on_silk(node) {
        node
    } else {
        fp.find_all("fp_text")
            .into_iter()
            .find(|t| t.get(2).and_then(SexpNode::as_str) == Some("${REFERENCE}") && on_silk(t))
            .unwrap_or(node)
    };
    let at = node.find("at")?;
    let (lx, ly) = (at.get_f64(1)?, at.get_f64(2)?);
    let (size, stroke) = font_of(node);
    let justify = node
        .find("effects")
        .and_then(|e| e.find("justify"))
        .map(|j| {
            let words: Vec<&str> = j
                .children()
                .unwrap_or_default()
                .iter()
                .filter_map(SexpNode::as_str)
                .collect();
            let h = if words.contains(&"left") {
                -1
            } else if words.contains(&"right") {
                1
            } else {
                0
            };
            let v = if words.contains(&"top") {
                -1
            } else if words.contains(&"bottom") {
                1
            } else {
                0
            };
            (h, v)
        })
        .unwrap_or((0, 0));
    Some(FieldText {
        at: transform_pad(lx, ly, fx, fy, frot),
        rotation: at.get_f64(3).unwrap_or(0.0),
        size,
        stroke,
        layer: node.find_str("layer").unwrap_or("F.SilkS").to_string(),
        visible: !hidden(node),
        justify,
    })
}

fn silk_boxes(fp: &SexpNode, (fx, fy, frot): (f64, f64, f64)) -> Vec<Rect> {
    let mut out = Vec::new();
    for g in fp.children().unwrap_or_default() {
        let Some(head) = g.head() else { continue };
        if !matches!(
            head,
            "fp_line" | "fp_rect" | "fp_circle" | "fp_arc" | "fp_poly"
        ) {
            continue;
        }
        if !g.find_str("layer").is_some_and(|l| l.ends_with(".SilkS")) {
            continue;
        }
        let mut pts: Vec<(f64, f64)> = ["start", "end", "mid", "center"]
            .iter()
            .filter_map(|k| g.find(k))
            .filter_map(|n| Some((n.get_f64(1)?, n.get_f64(2)?)))
            .collect();
        if let Some(poly) = g.find("pts") {
            pts.extend(
                poly.find_all("xy")
                    .into_iter()
                    .filter_map(|n| Some((n.get_f64(1)?, n.get_f64(2)?))),
            );
        }
        if head == "fp_circle" && pts.len() == 2 {
            let r = dist(pts[0], pts[1]);
            let c = pts[0];
            pts = vec![(c.0 - r, c.1 - r), (c.0 + r, c.1 + r)];
        }
        if pts.is_empty() {
            continue;
        }
        let width = g
            .find("stroke")
            .and_then(|s| s.find("width"))
            .and_then(|w| w.get_f64(1))
            .or_else(|| g.find("width").and_then(|w| w.get_f64(1)))
            .unwrap_or(0.12)
            / 2.0;
        let board: Vec<(f64, f64)> = pts
            .iter()
            .map(|&(x, y)| transform_pad(x, y, fx, fy, frot))
            .collect();
        let b = board
            .iter()
            .fold((f64::MAX, f64::MAX, f64::MIN, f64::MIN), |a, p| {
                (a.0.min(p.0), a.1.min(p.1), a.2.max(p.0), a.3.max(p.1))
            });
        out.push((b.0 - width, b.1 - width, b.2 + width, b.3 + width));
    }
    out
}

fn reference_of(fp: &SexpNode) -> Option<String> {
    property(fp, "Reference")
}

fn is_locked(fp: &SexpNode) -> bool {
    fp.children().unwrap_or_default().iter().any(|c| {
        c.as_str() == Some("locked")
            || (c.head() == Some("locked") && c.get(1).and_then(SexpNode::as_str) == Some("yes"))
    })
}

fn parse_pad(
    pad: &SexpNode,
    reference: &str,
    (fx, fy, frot): (f64, f64, f64),
    index: &PcbConnectivityIndex,
) -> Option<Pad> {
    let number = pad.get(1)?.as_str()?.to_string();
    let kind = pad.get(2).and_then(SexpNode::as_str).unwrap_or("smd");
    let shape = match pad.get(3).and_then(SexpNode::as_str) {
        Some("circle") => PadShape::Circle,
        _ => PadShape::Rect,
    };
    let at = pad.find("at")?;
    let local = (at.get_f64(1)?, at.get_f64(2)?);
    // The angle written on a pad is its absolute orientation (footprint turn
    // included); KiCad leaves it out only when that orientation is 0°.
    let rotation = at.get_f64(3).unwrap_or(0.0);
    let size = pad.find("size")?;
    let mut size = (size.get_f64(1)?, size.get_f64(2)?);
    if pad.get(3).and_then(SexpNode::as_str) == Some("custom") {
        if let Some((hx, hy)) = primitives_extent(pad) {
            size = (size.0.max(2.0 * hx), size.1.max(2.0 * hy));
        }
    }
    let mut front = false;
    let mut back = false;
    if let Some(layers) = pad.find("layers").and_then(SexpNode::children) {
        for l in layers.iter().skip(1).filter_map(SexpNode::as_str) {
            match l {
                "*.Cu" => {
                    front = true;
                    back = true;
                }
                "F&B.Cu" => {
                    front = true;
                    back = true;
                }
                "F.Cu" => front = true,
                "B.Cu" => back = true,
                _ => {}
            }
        }
    }
    if !front && !back {
        return None;
    }
    let through_hole = kind == "thru_hole" || kind == "np_thru_hole";
    let net = if kind == "np_thru_hole" {
        None
    } else {
        index.net_of_pad(reference, &number).map(str::to_string)
    };
    Some(Pad {
        reference: reference.to_string(),
        number,
        net,
        at: transform_pad(local.0, local.1, fx, fy, frot),
        local,
        size,
        rotation,
        shape,
        front,
        back,
        through_hole: through_hole && front && back,
    })
}

/// Half extents, around the anchor, of a custom pad's drawn shapes: the
/// anchor of such a pad is often a tiny dot and the copper is in its
/// primitives.
fn primitives_extent(pad: &SexpNode) -> Option<(f64, f64)> {
    let prims = pad.find("primitives")?;
    let (mut hx, mut hy) = (0.0f64, 0.0f64);
    let mut grow = |x: f64, y: f64, r: f64| {
        hx = hx.max(x.abs() + r);
        hy = hy.max(y.abs() + r);
    };
    for shape in prims.children()?.iter().skip(1) {
        let half = shape.find_f64("width").unwrap_or(0.0) / 2.0;
        if let Some(pts) = shape.find("pts") {
            for xy in pts.find_all("xy") {
                if let (Some(x), Some(y)) = (xy.get_f64(1), xy.get_f64(2)) {
                    grow(x, y, half);
                }
            }
        }
        for tag in ["start", "end", "mid"] {
            if let Some(p) = shape.find(tag) {
                if let (Some(x), Some(y)) = (p.get_f64(1), p.get_f64(2)) {
                    grow(x, y, half);
                }
            }
        }
        if let (Some(c), Some(e)) = (shape.find("center"), shape.find("end")) {
            if let (Some(cx), Some(cy), Some(ex), Some(ey)) =
                (c.get_f64(1), c.get_f64(2), e.get_f64(1), e.get_f64(2))
            {
                grow(cx, cy, (ex - cx).hypot(ey - cy) + half);
            }
        }
    }
    (hx > 0.0 || hy > 0.0).then_some((hx, hy))
}

pub fn dist(a: (f64, f64), b: (f64, f64)) -> f64 {
    ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2)).sqrt()
}

pub fn rect_distance(r: Rect, p: (f64, f64)) -> f64 {
    let dx = (r.0 - p.0).max(0.0).max(p.0 - r.2);
    let dy = (r.1 - p.1).max(0.0).max(p.1 - r.3);
    (dx * dx + dy * dy).sqrt()
}

pub fn segment_distance(a: (f64, f64), b: (f64, f64), p: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    if len2 < 1e-12 {
        return dist(a, p);
    }
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0);
    dist((a.0 + t * dx, a.1 + t * dy), p)
}

/// Shortest distance between two line segments.
pub fn segments_distance(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    let cross = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| {
        (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
    };
    let (d1, d2) = (cross(a, b, c), cross(a, b, d));
    let (d3, d4) = (cross(c, d, a), cross(c, d, b));
    if d1 * d2 < 0.0 && d3 * d4 < 0.0 {
        return 0.0;
    }
    segment_distance(a, b, c)
        .min(segment_distance(a, b, d))
        .min(segment_distance(c, d, a))
        .min(segment_distance(c, d, b))
}

impl Pad {
    /// Shortest distance from the pad's copper to a line segment.
    pub fn segment_distance(&self, a: (f64, f64), b: (f64, f64)) -> f64 {
        match self.shape {
            PadShape::Circle => {
                let r = self.size.0.min(self.size.1) / 2.0;
                (segment_distance(a, b, self.at) - r).max(0.0)
            }
            PadShape::Rect => {
                let r = self.bbox();
                if rect_distance(r, a) == 0.0 || rect_distance(r, b) == 0.0 {
                    return 0.0;
                }
                let corners = [(r.0, r.1), (r.2, r.1), (r.2, r.3), (r.0, r.3)];
                (0..4)
                    .map(|i| segments_distance(a, b, corners[i], corners[(i + 1) % 4]))
                    .fold(f64::INFINITY, f64::min)
            }
        }
    }
}

pub fn rects_overlap(a: Rect, b: Rect, gap: f64) -> bool {
    a.0 < b.2 + gap && b.0 < a.2 + gap && a.1 < b.3 + gap && b.1 < a.3 + gap
}

pub fn point_in_polygon(p: (f64, f64), poly: &[(f64, f64)]) -> bool {
    let mut inside = false;
    let n = poly.len();
    if n < 3 {
        return false;
    }
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > p.1) != (yj > p.1) && p.0 < (xj - xi) * (p.1 - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }
    inside
}

pub fn short_net(net: &str) -> &str {
    net.rsplit('/').next().unwrap_or(net)
}

pub fn is_ground(net: &str) -> bool {
    let n = short_net(net).to_ascii_uppercase();
    n.starts_with("GND") || n.ends_with("GND") || n == "VSS" || n == "0V"
}

pub fn is_unnamed(net: &str) -> bool {
    short_net(net).starts_with("Net-(") || short_net(net).starts_with("unconnected-")
}

const FAST_MARKERS: &[&str] = &[
    "USB", "D+", "D-", "DP", "DM", "CLK", "SCK", "XTAL", "XIN", "XOUT", "OSC", "SDIO", "QSPI",
    "MCLK", "BCLK", "ETH", "RF", "ANT", "LVDS", "MIPI", "HDMI", "SWCLK",
];

const POWER_WORDS: &[&str] = &[
    "VBUS", "VCC", "VDD", "VIN", "VOUT", "VSYS", "VBAT", "VDDIO", "VUSB",
];
/// Slow or static lines that share a name with a fast interface: HDMI CEC
/// and hot-plug, DDC and I2C, antenna bias, enables.
const SLOW_WORDS: &[&str] = &[
    "CEC", "HPD", "DDC", "BIAS", "I2C", "SCL", "SDA", "EN", "DET",
];

/// Fast or sensitive signal by name. Short markers (RF, DP, DM) must be a
/// whole word of the name so that power rails such as VDD_NRF or VDDPA do not
/// count; longer markers may appear inside a word (SWCLK, MCLK, USB1_DP).
pub fn is_fast(net: &str) -> bool {
    let n = short_net(net).to_ascii_uppercase();
    let words: Vec<&str> = n
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '+' && c != '-')
        .filter(|w| !w.is_empty())
        .collect();
    if words
        .iter()
        .any(|w| w.starts_with("USB") && (w.ends_with("D+") || w.ends_with("D-")))
    {
        return true;
    }
    if super::super::design_model::is_power_net(net) || is_ground(net) {
        return false;
    }
    if words
        .iter()
        .any(|w| POWER_WORDS.contains(w) || SLOW_WORDS.contains(w))
    {
        return false;
    }
    FAST_MARKERS.iter().any(|m| {
        if m.len() <= 2 {
            words.iter().any(|w| w == m)
        } else {
            n.contains(m)
        }
    })
}

pub fn round3(v: f64) -> f64 {
    (v * 1000.0).round() / 1000.0
}

#[cfg(test)]
pub(crate) mod fixture {
    pub fn board(body: &str) -> String {
        format!(
            r#"(kicad_pcb (version 20250114) (generator "test")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (net 0 "") (net 1 "GND") (net 2 "+5V") (net 3 "SIG") (net 4 "Net-(D1-A)")
  (gr_rect (start 100 100) (end 130 120) (stroke (width 0.05) (type default)) (fill none) (layer "Edge.Cuts"))
{body})"#
        )
    }

    pub fn smd(reference: &str, x: f64, y: f64, rot: f64, nets: (&str, &str)) -> String {
        format!(
            r#"  (footprint "Resistor_SMD:R_0805" (layer "F.Cu") (at {x} {y} {rot})
    (property "Reference" "{reference}" (at 0 -1.5 0) (layer "F.SilkS"))
    (property "Value" "10k" (at 0 1.5 0) (layer "F.Fab"))
    (fp_rect (start -1.7 -1) (end 1.7 1) (layer "F.CrtYd"))
    (pad "1" smd roundrect (at -0.9 0 {rot}) (size 1 1.4) (layers "F.Cu" "F.Paste" "F.Mask") (net {n1}))
    (pad "2" smd roundrect (at 0.9 0 {rot}) (size 1 1.4) (layers "F.Cu" "F.Paste" "F.Mask") (net {n2})))
"#,
            n1 = net_ref(nets.0),
            n2 = net_ref(nets.1),
        )
    }

    pub fn header(reference: &str, x: f64, y: f64, nets: &[&str]) -> String {
        let mut pads = String::new();
        for (i, n) in nets.iter().enumerate() {
            pads.push_str(&format!(
                "    (pad \"{}\" thru_hole circle (at 0 {}) (size 1.7 1.7) (drill 1) (layers \"*.Cu\" \"*.Mask\") (net {}))\n",
                i + 1,
                i as f64 * 2.54,
                net_ref(n)
            ));
        }
        format!(
            r#"  (footprint "Connector_PinHeader_2.54mm:PinHeader_1x0{}" (layer "F.Cu") (at {x} {y})
    (property "Reference" "{reference}" (at 0 -2.3 0) (layer "F.SilkS"))
    (property "Value" "Conn" (at 0 6 0) (layer "F.Fab"))
    (fp_rect (start -1.3 -1.3) (end 1.3 {}) (layer "F.CrtYd"))
{pads}  )
"#,
            nets.len(),
            (nets.len() as f64 - 1.0) * 2.54 + 1.3,
        )
    }

    fn net_ref(name: &str) -> String {
        let code = match name {
            "GND" => 1,
            "+5V" => 2,
            "SIG" => 3,
            "Net-(D1-A)" => 4,
            _ => 0,
        };
        format!("{code} \"{name}\"")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn justified_text_grows_away_from_its_anchor() {
        let t = FieldText {
            at: (10.0, 10.0),
            rotation: 0.0,
            size: 1.0,
            stroke: 0.15,
            layer: "F.SilkS".into(),
            visible: true,
            justify: (-1, 0),
        };
        let r = t.bbox("R1", false);
        assert!((r.0 - 10.0).abs() < 1e-9 && r.2 > 11.0);
        let upside_down = FieldText {
            rotation: 180.0,
            ..t
        };
        let r = upside_down.bbox("R1", false);
        assert!((r.2 - 10.0).abs() < 1e-9 && r.0 < 9.0);
    }

    #[test]
    fn names_fast_nets_by_whole_words() {
        assert!(is_fast("/USB1_D+"));
        assert!(is_fast("USB_DP"));
        assert!(!is_fast("LOAD+"));
        assert!(!is_fast("USB1_VBUS"));
        assert!(!is_fast("HDMI1_CEC"));
        assert!(!is_fast("ANT_BIAS"));
    }

    #[test]
    fn reads_kicad5_boards_and_pad_shapes() {
        let legacy = r#"(kicad_pcb (version 20171130) (host pcbnew 5.1.8)
  (layers (0 F.Cu signal) (31 B.Cu signal) (44 Edge.Cuts user))
  (net 0 "") (net 1 +3V3)
  (gr_line (start 103 100) (end 127 100) (layer Edge.Cuts) (width 0.05))
  (gr_arc (start 103 103) (end 103 100) (angle -90) (layer Edge.Cuts) (width 0.05))
  (gr_line (start 100 103) (end 100 120) (layer Edge.Cuts) (width 0.05))
  (segment (start 110 110) (end 115 110) (width 0.25) (layer F.Cu) (net 1))
)"#;
        let b = Board::from_text(legacy).unwrap();
        let o = b.outline.expect("outline with a legacy arc");
        assert!((o.0 - 100.0).abs() < 1e-6 && (o.1 - 100.0).abs() < 1e-6);
        assert_eq!(b.segments[0].net.as_deref(), Some("+3V3"));
        let renamed = legacy
            .replace(
                "(0 F.Cu signal) (31 B.Cu signal)",
                "(0 Top signal) (1 GND power) (31 Bottom signal)",
            )
            .replace("(layer F.Cu)", "(layer Top)");
        let b = Board::from_text(&renamed).unwrap();
        assert_eq!(b.segments[0].layer, "F.Cu");
        assert_eq!(b.copper_layers, vec!["F.Cu", "In1.Cu", "B.Cu"]);

        let modern = r#"(kicad_pcb (version 20260206) (generator "test")
  (layers (0 "F.Cu" signal) (2 "B.Cu" signal) (25 "Edge.Cuts" user))
  (gr_poly (pts (arc (start 100 110) (mid 110 100) (end 120 110)) (arc (start 120 110) (mid 110 120) (end 100 110))) (layer "Edge.Cuts"))
  (zone (net "GND") (layers "F&B.Cu") (polygon (pts (xy 100 100) (xy 120 100) (xy 120 120))))
  (footprint "X" (layer "F.Cu") (at 110 110 90)
    (property "Reference" "U1" (at 0 0 0) (layer "F.SilkS"))
    (pad "1" smd rect (at 0 2) (size 0.3 1.3) (layers "F.Cu") (net "GND"))
    (pad "2" smd custom (at 0 -2 90) (size 0.2 0.2) (layers "F.Cu") (net "GND")
      (primitives (gr_poly (pts (xy -1 -0.5) (xy 1 -0.5) (xy 1 0.5) (xy -1 0.5)) (width 0) (fill yes)))))
)"#;
        let b = Board::from_text(modern).unwrap();
        assert_eq!(b.outline, Some((100.0, 100.0, 120.0, 120.0)));
        assert_eq!(
            b.zones[0].layers,
            vec!["B.Cu".to_string(), "F.Cu".to_string()]
        );
        let pad = |n: &str| b.pads.iter().find(|p| p.number == n).unwrap();
        assert_eq!(pad("1").rotation, 0.0);
        assert_eq!(pad("2").size, (2.0, 1.0));
    }

    #[test]
    fn reads_pads_nets_and_outline() {
        let text = fixture::board(&format!(
            "{}{}",
            fixture::header("J1", 104.0, 106.0, &["+5V", "GND"]),
            fixture::smd("R1", 115.0, 106.0, 0.0, ("+5V", "SIG"))
        ));
        let b = Board::from_text(&text).unwrap();
        assert_eq!(b.outline, Some((100.0, 100.0, 130.0, 120.0)));
        assert_eq!(b.footprints.len(), 2);
        let j1 = b.footprint("J1").unwrap();
        assert!(j1.is_connector());
        let p2 = &b.pads[j1.pads[1]];
        assert_eq!(p2.net.as_deref(), Some("GND"));
        assert!((p2.at.1 - 108.54).abs() < 1e-9);
        assert!(p2.through_hole);
        let r1 = b.footprint("R1").unwrap();
        let p = &b.pads[r1.pads[0]];
        assert!((p.at.0 - 114.1).abs() < 1e-9 && p.front && !p.back);
    }

    #[test]
    fn rotated_footprint_moves_its_pads_and_courtyard() {
        let text = fixture::board(&fixture::smd("R1", 110.0, 110.0, 90.0, ("+5V", "SIG")));
        let b = Board::from_text(&text).unwrap();
        let r1 = b.footprint("R1").unwrap();
        let p1 = &b.pads[r1.pads[0]];
        assert!((p1.at.0 - 110.0).abs() < 1e-9);
        assert!((p1.at.1 - 110.9).abs() < 1e-9);
        let (hx, hy) = p1.half_extent();
        assert!((hx - 0.7).abs() < 1e-9 && (hy - 0.5).abs() < 1e-9);
        let c = r1.courtyard_at((120.0, 110.0), 0.0);
        assert!((c.0 - 118.3).abs() < 1e-6 && (c.3 - 111.0).abs() < 1e-6);
    }

    #[test]
    fn geometry_helpers() {
        assert!(point_in_polygon(
            (1.0, 1.0),
            &[(0.0, 0.0), (2.0, 0.0), (2.0, 2.0), (0.0, 2.0)]
        ));
        assert!((segment_distance((0.0, 0.0), (10.0, 0.0), (5.0, 3.0)) - 3.0).abs() < 1e-9);
        assert!(is_ground("/GND") && !is_ground("SIG"));
        assert!(is_fast("USB_D+") && !is_fast("LED"));
    }
}
