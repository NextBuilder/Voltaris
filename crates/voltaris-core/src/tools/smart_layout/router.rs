//! Grid A* router: routes every unrouted connection on the board in priority
//! order (differential pairs, power, fast, signal, ground), on any number of
//! copper layers with vias, keeps KiCad's clearance from foreign copper,
//! prefers octilinear paths with few bends, straightens them afterwards, and
//! retries failed nets first on the next pass.
//!
//! Differential pairs (D+/D-, _P/_N, DP/DM) are routed as a coupled pair: the
//! positive net first with room left beside it, the negative net hugging it at
//! the pair gap, then the shorter one is length-tuned with mitred bumps until
//! the skew is inside tolerance. Fine-pitch parts (pitch under 0.8 mm) get an
//! automatic finer grid aligned to their pins and a neck-down width near their
//! pads, so their pins escape without any manual settings.

use super::model::{dist, is_fast, is_ground, segment_distance, Board, PadShape};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};

#[derive(Debug, Clone)]
pub struct RouteOptions {
    pub layers: Vec<String>,
    /// Routing grid in mm; None picks one from the finest pin pitch.
    pub grid: Option<f64>,
    pub clearance: f64,
    pub signal_width: f64,
    pub power_width: f64,
    pub net_widths: BTreeMap<String, f64>,
    pub via_size: f64,
    pub via_drill: f64,
    pub via_cost: f64,
    pub edge_clearance: f64,
    pub prefer_layer: Option<String>,
    pub passes: usize,
    pub only_nets: Option<Vec<String>>,
    pub max_expansions: usize,
    /// Explicit differential pairs as (positive, negative) net names.
    pub pairs: Vec<(String, String)>,
    /// Detect pairs from net names (D+/D-, _P/_N, DP/DM).
    pub auto_pairs: bool,
    pub pair_width: Option<f64>,
    pub pair_gap: Option<f64>,
    /// Largest allowed length difference inside a pair, in mm.
    pub pair_skew: f64,
    /// Neck tracks down near fine-pitch pads and pick a fine grid automatically.
    pub fine_pitch: bool,
    /// Altium-style layer directions: outer layers alternate horizontal and
    /// vertical, so a 2-layer board routes like a woven grid.
    pub layer_directions: bool,
    /// Allow tracks to pass between the two pads of a small passive.
    pub between_pads: bool,
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            layers: vec!["F.Cu".into(), "B.Cu".into()],
            grid: None,
            clearance: 0.2,
            signal_width: 0.25,
            power_width: 0.5,
            net_widths: BTreeMap::new(),
            via_size: 0.8,
            via_drill: 0.4,
            via_cost: 3.0,
            edge_clearance: 0.5,
            prefer_layer: Some("F.Cu".into()),
            passes: 3,
            only_nets: None,
            max_expansions: 4_000_000,
            pairs: Vec::new(),
            auto_pairs: true,
            pair_width: None,
            pair_gap: None,
            pair_skew: 0.1,
            fine_pitch: true,
            layer_directions: false,
            between_pads: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoutedSegment {
    pub layer: String,
    pub a: (f64, f64),
    pub b: (f64, f64),
    pub width: f64,
}

#[derive(Debug, Clone, Default)]
pub struct NetResult {
    pub net: String,
    pub status: &'static str,
    pub segments: Vec<RoutedSegment>,
    pub vias: Vec<(f64, f64)>,
    pub connections_total: usize,
    pub connections_routed: usize,
    pub detail: Option<String>,
}

impl NetResult {
    pub fn length(&self) -> f64 {
        self.segments.iter().map(|s| dist(s.a, s.b)).sum()
    }
}

#[derive(Debug, Clone, Default)]
pub struct PairResult {
    pub positive: String,
    pub negative: String,
    pub width: f64,
    pub gap: f64,
    pub length_positive: f64,
    pub length_negative: f64,
    pub skew: f64,
    pub coupled_pct: f64,
    pub tuned_mm: f64,
    pub status: &'static str,
}

#[derive(Debug, Clone)]
pub struct FinePitch {
    pub reference: String,
    pub pitch: f64,
    pub pad_width: f64,
    pub neck: f64,
}

#[derive(Debug, Clone, Default)]
pub struct RouteResult {
    pub nets: Vec<NetResult>,
    pub pairs: Vec<PairResult>,
    pub fine_pitch: Vec<FinePitch>,
    pub grid: f64,
    pub pass_used: usize,
    pub warnings: Vec<String>,
}

impl RouteResult {
    pub fn routed_connections(&self) -> (usize, usize) {
        self.nets.iter().fold((0, 0), |(r, t), n| {
            (r + n.connections_routed, t + n.connections_total)
        })
    }
    pub fn vias(&self) -> usize {
        self.nets.iter().map(|n| n.vias.len()).sum()
    }
    pub fn length(&self) -> f64 {
        self.nets.iter().map(NetResult::length).sum()
    }
    fn better_than(&self, other: &RouteResult) -> bool {
        let (a, _) = self.routed_connections();
        let (b, _) = other.routed_connections();
        if a != b {
            return a > b;
        }
        if self.vias() != other.vias() {
            return self.vias() < other.vias();
        }
        self.length() < other.length()
    }
}

#[derive(Debug, Clone)]
enum Shape {
    Rect(f64, f64, f64, f64),
    Circle((f64, f64), f64),
    Capsule((f64, f64), (f64, f64), f64),
}

impl Shape {
    fn distance(&self, p: (f64, f64)) -> f64 {
        match *self {
            Shape::Rect(x0, y0, x1, y1) => super::model::rect_distance((x0, y0, x1, y1), p),
            Shape::Circle(c, r) => (dist(c, p) - r).max(0.0),
            Shape::Capsule(a, b, r) => (segment_distance(a, b, p) - r).max(0.0),
        }
    }
    fn bbox(&self) -> (f64, f64, f64, f64) {
        match *self {
            Shape::Rect(x0, y0, x1, y1) => (x0, y0, x1, y1),
            Shape::Circle(c, r) => (c.0 - r, c.1 - r, c.0 + r, c.1 + r),
            Shape::Capsule(a, b, r) => (
                a.0.min(b.0) - r,
                a.1.min(b.1) - r,
                a.0.max(b.0) + r,
                a.1.max(b.1) + r,
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct Obstacle {
    net: Option<usize>,
    layers: u32,
    shape: Shape,
}

struct Grid {
    x0: f64,
    y0: f64,
    pitch: f64,
    nx: usize,
    ny: usize,
    nl: usize,
}

impl Grid {
    fn idx(&self, l: usize, x: usize, y: usize) -> usize {
        (l * self.ny + y) * self.nx + x
    }
    fn coord(&self, i: usize) -> (usize, usize, usize) {
        let per = self.nx * self.ny;
        let l = i / per;
        let r = i % per;
        (l, r % self.nx, r / self.nx)
    }
    fn center(&self, x: usize, y: usize) -> (f64, f64) {
        (
            self.x0 + x as f64 * self.pitch,
            self.y0 + y as f64 * self.pitch,
        )
    }
    fn cells_near(&self, bbox: (f64, f64, f64, f64), pad: f64) -> (usize, usize, usize, usize) {
        let fx = |v: f64| ((v - self.x0) / self.pitch).floor().max(0.0) as usize;
        let fy = |v: f64| ((v - self.y0) / self.pitch).floor().max(0.0) as usize;
        let cx = |v: f64| (((v - self.x0) / self.pitch).ceil().max(0.0) as usize).min(self.nx - 1);
        let cy = |v: f64| (((v - self.y0) / self.pitch).ceil().max(0.0) as usize).min(self.ny - 1);
        (
            fx(bbox.0 - pad).min(self.nx - 1),
            fy(bbox.1 - pad).min(self.ny - 1),
            cx(bbox.2 + pad),
            cy(bbox.3 + pad),
        )
    }
}

#[derive(Copy, Clone, PartialEq)]
struct Node {
    f: f64,
    cell: usize,
}
impl Eq for Node {}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.partial_cmp(&self.f)
            .unwrap_or(Ordering::Equal)
            .then_with(|| o.cell.cmp(&self.cell))
    }
}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}

const DIRS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

struct NetPlan {
    name: String,
    id: usize,
    width: f64,
    tier: u8,
    span: f64,
    components: Vec<Vec<usize>>,
    terminal_points: Vec<Vec<(f64, f64)>>,
    /// (partner plan index, this net is the positive one)
    pair: Option<(usize, bool)>,
}

/// Name rules for differential pairs: (positive suffix, negative suffix).
const PAIR_SUFFIXES: &[(&str, &str)] = &[
    ("+", "-"),
    ("_P", "_N"),
    ("_DP", "_DN"),
    ("_DP", "_DM"),
    ("DP", "DM"),
    ("P", "N"),
];

const NOT_PAIR_BASES: &[&str] = &[
    "V", "VCC", "VDD", "VEE", "VSS", "VBAT", "VIN", "VOUT", "BAT", "PWR", "SUPPLY", "AC", "DC",
    "VBUS", "POWER",
];

/// Pairs nets by name: D+/D-, USB_DP/USB_DM, TX_P/TX_N, CLKP/CLKN and so on.
pub fn find_pairs(nets: &[String]) -> Vec<(String, String)> {
    let upper: Vec<String> = nets.iter().map(|n| n.to_ascii_uppercase()).collect();
    let mut used = vec![false; nets.len()];
    let mut out = Vec::new();
    for (i, u) in upper.iter().enumerate() {
        if used[i] || super::super::design_model::is_power_net(&nets[i]) || is_ground(&nets[i]) {
            continue;
        }
        for (sp, sn) in PAIR_SUFFIXES {
            let Some(base) = u.strip_suffix(sp) else {
                continue;
            };
            let short = super::model::short_net(base);
            if short.is_empty() || NOT_PAIR_BASES.contains(&short.trim_end_matches('_')) {
                continue;
            }
            if *sp == "P"
                && !(is_fast(base) || ["TX", "RX", "LVDS", "CLK"].iter().any(|m| base.contains(m)))
            {
                continue;
            }
            let want = format!("{base}{sn}");
            if let Some(j) = upper.iter().position(|v| *v == want) {
                if !used[j] && j != i {
                    used[i] = true;
                    used[j] = true;
                    out.push((nets[i].clone(), nets[j].clone()));
                    break;
                }
            }
        }
    }
    out
}

/// Footprints whose pins are closer than 0.8 mm, with the widest track that
/// still escapes between their pads at this clearance on the given grid
/// (pitch and origin): the further a part's pins sit off the grid lines, the
/// narrower the neck.
pub fn fine_pitch_parts(board: &Board, clearance: f64, grid: (f64, f64, f64)) -> Vec<FinePitch> {
    let (gx0, gy0, g) = grid;
    let mut out = Vec::new();
    for f in &board.footprints {
        let pads: Vec<&super::model::Pad> = f
            .pads
            .iter()
            .map(|&i| &board.pads[i])
            .filter(|p| !p.through_hole)
            .collect();
        if pads.len() < 3 {
            continue;
        }
        let mut pitch = f64::INFINITY;
        let mut pad_width = 0.0;
        let mut off: f64 = 0.0;
        for (i, a) in pads.iter().enumerate() {
            let mut near: Option<(f64, &super::model::Pad)> = None;
            for (j, b) in pads.iter().enumerate() {
                if i == j || a.front != b.front {
                    continue;
                }
                let d = dist(a.at, b.at);
                if d > 0.05 && near.is_none_or(|n| d < n.0) {
                    near = Some((d, b));
                }
            }
            let Some((d, b)) = near else { continue };
            if d < pitch - 1e-9 {
                pitch = d;
                pad_width = a.size.0.min(a.size.1);
            }
            if d < 0.8 {
                let (v, o) = if (a.at.0 - b.at.0).abs() > (a.at.1 - b.at.1).abs() {
                    (a.at.0, gx0)
                } else {
                    (a.at.1, gy0)
                };
                let k = ((v - o) / g).round();
                off = off.max((o + k * g - v).abs());
            }
        }
        if pitch >= 0.8 - 1e-9 {
            continue;
        }
        let margin = cell_margin(g, clearance + 0.05);
        let between = 2.0 * (pitch - pad_width / 2.0 - clearance - margin - off);
        let neck = (pitch - clearance).min(pad_width).min(between);
        out.push(FinePitch {
            reference: f.reference.clone(),
            pitch: round_to(pitch, 1000.0),
            pad_width: round_to(pad_width, 1000.0),
            neck: (neck * 100.0).floor() / 100.0,
        });
    }
    out
}

fn round_to(v: f64, k: f64) -> f64 {
    (v * k).round() / k
}

/// Extra clearance a grid cell needs so that the straight step between two
/// cell centres never comes closer to a convex obstacle than the centres do.
fn cell_margin(pitch: f64, keep: f64) -> f64 {
    pitch * pitch / (4.0 * keep.max(0.05)) + 0.002
}

/// The grid the router uses when none is given: 0.2 mm for ordinary boards; a
/// pitch-aligned grid of a quarter pitch or finer when fine-pitch parts exist.
fn auto_grid(fine: &[FinePitch]) -> f64 {
    let Some(min_pitch) = fine.iter().map(|f| f.pitch).reduce(f64::min) else {
        return 0.2;
    };
    let steps = (min_pitch / 0.125).ceil().max(4.0);
    (min_pitch / steps).clamp(0.08, 0.2)
}

/// Shifts the grid origin so the pins of the finest part sit on grid lines.
fn grid_origin(board: &Board, fine: &[FinePitch], outline: Rect, pitch: f64) -> (f64, f64) {
    let mut origin = (outline.0, outline.1);
    let Some(part) = fine
        .iter()
        .min_by(|a, b| a.pitch.partial_cmp(&b.pitch).unwrap_or(Ordering::Equal))
    else {
        return origin;
    };
    let Some(f) = board.footprint(&part.reference) else {
        return origin;
    };
    let pads: Vec<(f64, f64)> = f.pads.iter().map(|&i| board.pads[i].at).collect();
    let mut have = (false, false);
    for (i, a) in pads.iter().enumerate() {
        for b in &pads[i + 1..] {
            if (dist(*a, *b) - part.pitch).abs() > 0.01 {
                continue;
            }
            let along_x = (a.0 - b.0).abs() > (a.1 - b.1).abs();
            if along_x && !have.0 {
                origin.0 = a.0 - ((a.0 - outline.0) / pitch).floor() * pitch;
                have.0 = true;
            } else if !along_x && !have.1 {
                origin.1 = a.1 - ((a.1 - outline.1) / pitch).floor() * pitch;
                have.1 = true;
            }
        }
    }
    origin
}

type Rect = (f64, f64, f64, f64);

/// The gap between the two pads of each small two-terminal SMD part (R, C, L,
/// D, F, FB, LED): pro boards never run a track there, where it invites solder
/// bridges and traps flux.
fn between_pad_areas(board: &Board, layers: &[String]) -> Vec<(Rect, u32)> {
    let mut out = Vec::new();
    for f in &board.footprints {
        if f.pads.len() != 2
            || !matches!(
                f.prefix().as_str(),
                "R" | "C" | "L" | "D" | "F" | "FB" | "LED"
            )
        {
            continue;
        }
        let (a, b) = (&board.pads[f.pads[0]], &board.pads[f.pads[1]]);
        if a.through_hole || b.through_hole || a.front != b.front {
            continue;
        }
        let (ba, bb) = (a.bbox(), b.bbox());
        let r = if (a.at.0 - b.at.0).abs() >= (a.at.1 - b.at.1).abs() {
            let (x0, x1) = (ba.2.min(bb.2), ba.0.max(bb.0));
            (x0 + 0.05, ba.1.max(bb.1), x1 - 0.05, ba.3.min(bb.3))
        } else {
            let (y0, y1) = (ba.3.min(bb.3), ba.1.max(bb.1));
            (ba.0.max(bb.0), y0 + 0.05, ba.2.min(bb.2), y1 - 0.05)
        };
        if r.2 - r.0 <= 0.05 || r.3 - r.1 <= 0.05 {
            continue;
        }
        let side = if a.front { "F.Cu" } else { "B.Cu" };
        if let Some(i) = layers.iter().position(|l| l == side) {
            out.push((r, 1u32 << i));
        }
    }
    out
}

/// An octilinear track from `a` to `b`: straight, then 45°, when they are not
/// already on one line.
fn octilinear(a: (f64, f64), b: (f64, f64)) -> Vec<((f64, f64), (f64, f64))> {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let (ax, ay) = (dx.abs(), dy.abs());
    if ax < 1e-9 || ay < 1e-9 || (ax - ay).abs() < 1e-9 {
        return vec![(a, b)];
    }
    let m = ax.min(ay);
    let corner = (b.0 - dx.signum() * m, b.1 - dy.signum() * m);
    vec![(a, corner), (corner, b)]
}

/// Joins consecutive segments that continue in the same direction.
fn merge_collinear(segs: Vec<RoutedSegment>) -> Vec<RoutedSegment> {
    let mut out: Vec<RoutedSegment> = Vec::new();
    for s in segs {
        if let Some(last) = out.last_mut() {
            let d1 = (last.b.0 - last.a.0, last.b.1 - last.a.1);
            let d2 = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let cross = d1.0 * d2.1 - d1.1 * d2.0;
            let same_way = d1.0 * d2.0 + d1.1 * d2.1 > 0.0;
            if last.layer == s.layer
                && (last.width - s.width).abs() < 1e-9
                && dist(last.b, s.a) < 1e-6
                && cross.abs() < 1e-9
                && same_way
            {
                last.b = s.b;
                continue;
            }
        }
        out.push(s);
    }
    out
}

pub fn route(board: &Board, opts: &RouteOptions) -> anyhow::Result<RouteResult> {
    let outline = board
        .outline
        .ok_or_else(|| anyhow::anyhow!("the board has no Edge.Cuts outline to route inside"))?;
    let layers: Vec<String> = opts
        .layers
        .iter()
        .filter(|l| board.copper_layers.contains(l))
        .cloned()
        .collect();
    if layers.is_empty() {
        anyhow::bail!("none of the requested layers exist on this board");
    }
    let mut warnings = Vec::new();
    let probe = opts.grid.unwrap_or(0.1).clamp(0.05, 1.0);
    let mut fine = if opts.fine_pitch {
        fine_pitch_parts(board, opts.clearance, (outline.0, outline.1, probe))
    } else {
        Vec::new()
    };
    let mut pitch = match opts.grid {
        Some(g) => g.clamp(0.05, 1.0),
        None => auto_grid(&fine),
    };
    let cells = |p: f64| {
        (((outline.2 - outline.0) / p).ceil() as usize + 2)
            * (((outline.3 - outline.1) / p).ceil() as usize + 2)
            * layers.len()
    };
    if opts.grid.is_none() {
        while cells(pitch) > 8_000_000 && pitch < 0.5 {
            pitch *= 1.25;
        }
    }
    let (x0, y0) = grid_origin(board, &fine, outline, pitch);
    if opts.fine_pitch {
        fine = fine_pitch_parts(board, opts.clearance, (x0, y0, pitch));
        for f in &fine {
            if f.neck < 0.1 {
                warnings.push(format!(
                    "{} has a {:.2} mm pin pitch: at {:.2} mm clearance no track fits between its pads; lower 'clearance' (0.1-0.15 mm is typical for fine pitch)",
                    f.reference, f.pitch, opts.clearance
                ));
            }
        }
    }
    let grid = Grid {
        x0,
        y0,
        pitch,
        nx: ((outline.2 - x0) / pitch).ceil() as usize + 1,
        ny: ((outline.3 - y0) / pitch).ceil() as usize + 1,
        nl: layers.len(),
    };
    if grid.nx * grid.ny * grid.nl > 12_000_000 {
        anyhow::bail!("the routing grid would exceed 12 million cells; raise 'grid'");
    }
    let mut pad_neck: HashMap<usize, f64> = HashMap::new();
    for f in fine.iter().filter(|f| f.neck >= 0.1) {
        if let Some(fp) = board.footprint(&f.reference) {
            for &i in &fp.pads {
                pad_neck.insert(i, f.neck);
            }
        }
    }

    let nets: Vec<String> = board.nets().into_iter().collect();
    let net_id: HashMap<&str, usize> = nets
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let layer_bit = |name: &str| -> u32 {
        layers
            .iter()
            .position(|l| l == name)
            .map(|i| 1u32 << i)
            .unwrap_or(0)
    };
    let all_layers: u32 = (1u32 << layers.len()) - 1;

    let mut base: Vec<Obstacle> = Vec::new();
    for p in &board.pads {
        let mut mask = 0;
        for (i, l) in layers.iter().enumerate() {
            if p.on_layer(l) || (p.through_hole) {
                mask |= 1 << i;
            }
        }
        if mask == 0 {
            continue;
        }
        let shape = match p.shape {
            PadShape::Circle => Shape::Circle(p.at, p.size.0.min(p.size.1) / 2.0),
            PadShape::Rect => {
                let b = p.bbox();
                Shape::Rect(b.0, b.1, b.2, b.3)
            }
        };
        base.push(Obstacle {
            net: p.net.as_deref().and_then(|n| net_id.get(n).copied()),
            layers: mask,
            shape,
        });
    }
    for s in &board.segments {
        let bit = layer_bit(&s.layer);
        if bit == 0 {
            continue;
        }
        base.push(Obstacle {
            net: s.net.as_deref().and_then(|n| net_id.get(n).copied()),
            layers: bit,
            shape: Shape::Capsule(s.a, s.b, s.width / 2.0),
        });
    }
    for v in &board.vias {
        base.push(Obstacle {
            net: v.net.as_deref().and_then(|n| net_id.get(n).copied()),
            layers: all_layers,
            shape: Shape::Circle(v.at, v.size / 2.0),
        });
    }
    if !opts.between_pads {
        for (r, mask) in between_pad_areas(board, &layers) {
            base.push(Obstacle {
                net: None,
                layers: mask,
                shape: Shape::Rect(r.0, r.1, r.2, r.3),
            });
        }
    }

    let mut pair_list: Vec<(String, String)> = opts
        .pairs
        .iter()
        .filter(|(p, n)| net_id.contains_key(p.as_str()) && net_id.contains_key(n.as_str()))
        .cloned()
        .collect();
    if opts.auto_pairs {
        for (p, n) in find_pairs(&nets) {
            if !pair_list
                .iter()
                .any(|(a, b)| [a, b].contains(&&p) || [a, b].contains(&&n))
            {
                pair_list.push((p, n));
            }
        }
    }
    if let Some(only) = &opts.only_nets {
        pair_list.retain(|(p, n)| only.contains(p) && only.contains(n));
    }
    let pair_gap = opts.pair_gap.unwrap_or(opts.clearance);
    if pair_gap + 1e-9 < opts.clearance && !pair_list.is_empty() {
        warnings.push(format!(
            "pair_gap {pair_gap} mm is below the {} mm clearance KiCad's DRC checks between the two nets; the pairs use the clearance as their gap (add a diff-pair rule and lower 'clearance' for a tighter gap)",
            opts.clearance
        ));
    }
    let pair_gap = pair_gap.max(opts.clearance);

    let mut plans = Vec::new();
    let mut skipped = Vec::new();
    for (id, name) in nets.iter().enumerate() {
        if let Some(only) = &opts.only_nets {
            if !only.iter().any(|n| n == name) {
                continue;
            }
        }
        let pads: Vec<usize> = board
            .pads
            .iter()
            .enumerate()
            .filter(|(_, p)| p.net.as_deref() == Some(name))
            .map(|(i, _)| i)
            .collect();
        if pads.len() < 2 {
            continue;
        }
        let poured = board.poured_layers(name);
        if !poured.is_empty() && layers.iter().all(|l| poured.contains(l)) {
            skipped.push(NetResult {
                net: name.clone(),
                status: "pour",
                detail: Some("connected by copper pours on every routing layer".into()),
                ..Default::default()
            });
            continue;
        }
        let components = connectivity(board, name, &pads);
        if components.len() < 2 {
            skipped.push(NetResult {
                net: name.clone(),
                status: "already_connected",
                ..Default::default()
            });
            continue;
        }
        let in_pair = pair_list.iter().any(|(p, n)| p == name || n == name);
        let power = super::super::design_model::is_power_net(name) || is_ground(name);
        let explicit = opts.net_widths.get(name).copied().or_else(|| {
            opts.net_widths
                .iter()
                .find(|(k, _)| super::model::short_net(name) == k.as_str())
                .map(|(_, v)| *v)
        });
        let width = explicit.unwrap_or(if in_pair {
            opts.pair_width.unwrap_or(opts.signal_width)
        } else if power {
            opts.power_width
        } else {
            opts.signal_width
        });
        let tier = if in_pair {
            0
        } else if is_ground(name) && !poured.is_empty() {
            1
        } else if power && !is_ground(name) {
            2
        } else if is_fast(name) {
            3
        } else if is_ground(name) {
            5
        } else {
            4
        };
        let pts: Vec<(f64, f64)> = pads.iter().map(|&i| board.pads[i].at).collect();
        let span = {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            for p in &pts {
                x0 = x0.min(p.0);
                y0 = y0.min(p.1);
                x1 = x1.max(p.0);
                y1 = y1.max(p.1);
            }
            (x1 - x0) + (y1 - y0)
        };
        let terminal_points = components
            .iter()
            .map(|c| c.iter().map(|&i| board.pads[i].at).collect())
            .collect();
        plans.push(NetPlan {
            name: name.clone(),
            id,
            width,
            tier,
            span,
            components,
            terminal_points,
            pair: None,
        });
    }
    plans.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then(a.span.partial_cmp(&b.span).unwrap_or(Ordering::Equal))
            .then(a.name.cmp(&b.name))
    });
    let mut pairs_routed: Vec<(usize, usize)> = Vec::new();
    for (p, n) in &pair_list {
        let pi = plans.iter().position(|x| &x.name == p);
        let ni = plans.iter().position(|x| &x.name == n);
        match (pi, ni) {
            (Some(pi), Some(ni)) => {
                plans[pi].pair = Some((ni, true));
                plans[ni].pair = Some((pi, false));
                pairs_routed.push((pi, ni));
            }
            (None, None) => {}
            _ => warnings.push(format!(
                "{p}/{n}: one side is already connected, so the pair is not routed as a coupled pair; check its skew with analyze_net_lengths"
            )),
        }
    }
    for plan in &plans {
        if plan.pair.is_none() && is_fast(&plan.name) {
            warnings.push(format!(
                "{} looks like a fast signal; keep it short and check it with analyze_net_lengths",
                plan.name
            ));
        }
    }

    let ctx = RouteCtx {
        board,
        grid: &grid,
        layers: &layers,
        opts,
        pad_neck: &pad_neck,
        layer_bit: &layer_bit,
        all_layers,
    };
    let mut order: Vec<usize> = (0..plans.len()).collect();
    let mut best: Option<RouteResult> = None;
    for pass in 0..opts.passes.max(1) {
        let mut obstacles = base.clone();
        let mut results: Vec<NetResult> = Vec::new();
        let mut pairs_out: Vec<PairResult> = Vec::new();
        let mut failed = Vec::new();
        for &pi in &order {
            let plan = &plans[pi];
            match plan.pair {
                Some((_, false)) => continue,
                Some((ni, true)) => {
                    let (rp, rn, pr) = route_pair(&ctx, plan, &plans[ni], &mut obstacles, pair_gap);
                    if rp.connections_routed < rp.connections_total
                        || rn.connections_routed < rn.connections_total
                    {
                        failed.push(pi);
                    }
                    results.push(rp);
                    results.push(rn);
                    pairs_out.push(pr);
                }
                None => {
                    let r = route_net(&ctx, plan, &obstacles, &Extras::default());
                    ctx.add_obstacles(&mut obstacles, plan.id, &r);
                    if r.connections_routed < r.connections_total {
                        failed.push(pi);
                    }
                    results.push(r);
                }
            }
        }
        let candidate = RouteResult {
            nets: results,
            pairs: pairs_out,
            pass_used: pass + 1,
            ..Default::default()
        };
        let done = failed.is_empty();
        if best.as_ref().is_none_or(|b| candidate.better_than(b)) {
            best = Some(candidate);
        }
        if done {
            break;
        }
        let mut next: Vec<usize> = failed.clone();
        next.extend(order.iter().copied().filter(|i| !failed.contains(i)));
        order = next;
    }
    let mut best = best.unwrap_or_default();
    best.nets.sort_by(|a, b| a.net.cmp(&b.net));
    best.nets.extend(skipped);
    for p in &best.pairs {
        if p.status == "routed" && p.skew > opts.pair_skew + 1e-6 {
            warnings.push(format!(
                "{}/{}: skew {:.2} mm is above the {:.2} mm target (no room left to tune); lengthen the shorter net by hand",
                p.positive, p.negative, p.skew, opts.pair_skew
            ));
        }
    }
    best.warnings = warnings;
    best.grid = round_to(pitch, 10000.0);
    best.fine_pitch = fine;
    Ok(best)
}

struct RouteCtx<'a> {
    board: &'a Board,
    grid: &'a Grid,
    layers: &'a [String],
    opts: &'a RouteOptions,
    pad_neck: &'a HashMap<usize, f64>,
    layer_bit: &'a dyn Fn(&str) -> u32,
    all_layers: u32,
}

impl RouteCtx<'_> {
    fn add_obstacles(&self, obstacles: &mut Vec<Obstacle>, net: usize, r: &NetResult) {
        for s in &r.segments {
            obstacles.push(Obstacle {
                net: Some(net),
                layers: (self.layer_bit)(&s.layer),
                shape: Shape::Capsule(s.a, s.b, s.width / 2.0),
            });
        }
        for v in &r.vias {
            obstacles.push(Obstacle {
                net: Some(net),
                layers: self.all_layers,
                shape: Shape::Circle(*v, self.opts.via_size / 2.0),
            });
        }
    }
}

/// Per-net adjustments for pair routing.
#[derive(Default)]
struct Extras {
    /// Keep this much further from foreign copper (room for the partner),
    /// except from the partner net itself and near the pair's own pads.
    inflate: Option<(f64, usize, Vec<usize>)>,
    /// Cells at the pair spacing from the partner: cheaper, so the path hugs it.
    couple: Option<Vec<bool>>,
}

fn route_pair(
    ctx: &RouteCtx<'_>,
    p: &NetPlan,
    n: &NetPlan,
    obstacles: &mut Vec<Obstacle>,
    gap: f64,
) -> (NetResult, NetResult, PairResult) {
    let spacing = p.width / 2.0 + gap + n.width / 2.0;
    let pair_pads: Vec<usize> = p
        .components
        .iter()
        .chain(n.components.iter())
        .flatten()
        .copied()
        .collect();
    let inflated = route_net(
        ctx,
        p,
        obstacles,
        &Extras {
            inflate: Some((gap + n.width, n.id, pair_pads)),
            couple: None,
        },
    );
    let mut rp = if inflated.connections_routed == inflated.connections_total {
        inflated
    } else {
        let plain = route_net(ctx, p, obstacles, &Extras::default());
        if plain.connections_routed > inflated.connections_routed {
            plain
        } else {
            inflated
        }
    };
    let p_start = obstacles.len();
    ctx.add_obstacles(obstacles, p.id, &rp);
    let band = coupling_band(ctx.grid, ctx.layers, &rp.segments, spacing);
    let mut rn = route_net(
        ctx,
        n,
        obstacles,
        &Extras {
            inflate: None,
            couple: Some(band),
        },
    );
    let both = rp.connections_routed == rp.connections_total
        && rn.connections_routed == rn.connections_total;
    let mut tuned = 0.0;
    let skew0 = (rp.length() - rn.length()).abs();
    if both && skew0 > ctx.opts.pair_skew {
        if rn.length() < rp.length() {
            tuned = tune_length(ctx, &mut rn, n.id, obstacles, skew0, &rp.segments);
            ctx.add_obstacles(obstacles, n.id, &rn);
        } else {
            obstacles.truncate(p_start);
            ctx.add_obstacles(obstacles, n.id, &rn);
            tuned = tune_length(ctx, &mut rp, p.id, obstacles, skew0, &rn.segments);
            ctx.add_obstacles(obstacles, p.id, &rp);
        }
    } else {
        ctx.add_obstacles(obstacles, n.id, &rn);
    }
    let coupled = coupled_fraction(&rn.segments, &rp.segments, spacing + ctx.grid.pitch * 1.5);
    let pr = PairResult {
        positive: p.name.clone(),
        negative: n.name.clone(),
        width: p.width,
        gap,
        length_positive: rp.length(),
        length_negative: rn.length(),
        skew: (rp.length() - rn.length()).abs(),
        coupled_pct: coupled * 100.0,
        tuned_mm: tuned,
        status: if both { "routed" } else { "partial" },
    };
    if both {
        let note = format!(
            "differential pair with {}: {:.0}% coupled, skew {:.3} mm",
            if p.name == pr.positive {
                &n.name
            } else {
                &p.name
            },
            pr.coupled_pct,
            pr.skew
        );
        rp.detail = Some(note.clone());
        rn.detail = Some(note.replacen(&n.name, &p.name, 1));
    }
    (rp, rn, pr)
}

/// Grid cells whose centre lies at the pair spacing (plus up to 1.5 cells) from
/// the partner's tracks on the same layer.
fn coupling_band(
    grid: &Grid,
    layers: &[String],
    partner: &[RoutedSegment],
    spacing: f64,
) -> Vec<bool> {
    let mut band = vec![false; grid.nx * grid.ny * grid.nl];
    let mut nearest: HashMap<usize, f64> = HashMap::new();
    let hi = spacing + grid.pitch * 1.5;
    for s in partner {
        let Some(l) = layers.iter().position(|x| x == &s.layer) else {
            continue;
        };
        let bbox = (
            s.a.0.min(s.b.0),
            s.a.1.min(s.b.1),
            s.a.0.max(s.b.0),
            s.a.1.max(s.b.1),
        );
        let (x0, y0, x1, y1) = grid.cells_near(bbox, hi);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let d = segment_distance(s.a, s.b, grid.center(x, y));
                let e = nearest.entry(grid.idx(l, x, y)).or_insert(f64::INFINITY);
                *e = e.min(d);
            }
        }
    }
    for (cell, d) in nearest {
        if d >= spacing - 0.02 && d <= hi {
            band[cell] = true;
        }
    }
    band
}

fn coupled_fraction(net: &[RoutedSegment], partner: &[RoutedSegment], within: f64) -> f64 {
    let (mut total, mut near) = (0.0, 0.0);
    for s in net {
        let len = dist(s.a, s.b);
        let steps = (len / 0.1).ceil().max(1.0) as usize;
        for k in 0..steps {
            let t = (k as f64 + 0.5) / steps as f64;
            let q = (s.a.0 + t * (s.b.0 - s.a.0), s.a.1 + t * (s.b.1 - s.a.1));
            let d = partner
                .iter()
                .filter(|o| o.layer == s.layer)
                .map(|o| segment_distance(o.a, o.b, q))
                .fold(f64::INFINITY, f64::min);
            total += len / steps as f64;
            if d <= within {
                near += len / steps as f64;
            }
        }
    }
    if total > 0.0 {
        near / total
    } else {
        0.0
    }
}

/// Adds mitred (45°) bumps to the straight runs of a routed net until it is
/// `extra` mm longer, on the side away from its partner, keeping clearance.
/// Returns the length added.
fn tune_length(
    ctx: &RouteCtx<'_>,
    r: &mut NetResult,
    net: usize,
    obstacles: &[Obstacle],
    extra: f64,
    partner: &[RoutedSegment],
) -> f64 {
    let opts = ctx.opts;
    let outline = ctx.board.outline.unwrap_or_default();
    let mut remaining = extra;
    let mut added = 0.0;
    let mut order: Vec<usize> = (0..r.segments.len()).collect();
    order.sort_by(|&a, &b| {
        let la = dist(r.segments[a].a, r.segments[a].b);
        let lb = dist(r.segments[b].a, r.segments[b].b);
        lb.partial_cmp(&la).unwrap_or(Ordering::Equal)
    });
    let mut replace: Vec<(usize, Vec<RoutedSegment>)> = Vec::new();
    for si in order {
        if remaining < 0.005 {
            break;
        }
        let s = r.segments[si].clone();
        let len = dist(s.a, s.b);
        let w = s.width;
        let top = (2.0 * w).max(0.4);
        let amax = (4.0 * w).max(1.0);
        let end_margin = (2.0 * w).max(0.8);
        let per_bump = |a: f64| a * (2.0 * std::f64::consts::SQRT_2 - 2.0);
        let room = len - 2.0 * end_margin;
        if room < 2.0 * 0.2 + top {
            continue;
        }
        let u = ((s.b.0 - s.a.0) / len, (s.b.1 - s.a.1) / len);
        let fits =
            |a: f64, k: usize| k as f64 * (2.0 * a + top) + (k as f64 - 1.0).max(0.0) * top <= room;
        let c = per_bump(1.0);
        let mut k = 1usize;
        while remaining / (c * k as f64) > amax && fits(amax, k + 1) {
            k += 1;
        }
        let a = (remaining / (c * k as f64)).clamp(0.05, amax);
        if !fits(a, k) {
            continue;
        }
        let normal_a = (-u.1, u.0);
        let side_score = |nv: (f64, f64)| -> f64 {
            let mid = (
                (s.a.0 + s.b.0) / 2.0 + nv.0 * a,
                (s.a.1 + s.b.1) / 2.0 + nv.1 * a,
            );
            partner
                .iter()
                .filter(|o| o.layer == s.layer)
                .map(|o| segment_distance(o.a, o.b, mid))
                .fold(f64::INFINITY, f64::min)
        };
        let mut sides = [normal_a, (-normal_a.0, -normal_a.1)];
        if side_score(sides[1]) > side_score(sides[0]) {
            sides.swap(0, 1);
        }
        let used = k as f64 * (2.0 * a + top) + (k as f64 - 1.0) * top;
        let start = (len - used) / 2.0;
        for nv in sides {
            let at = |along: f64, out: f64| {
                (
                    s.a.0 + u.0 * along + nv.0 * out,
                    s.a.1 + u.1 * along + nv.1 * out,
                )
            };
            let mut pts = vec![s.a];
            let mut t = start;
            for _ in 0..k {
                pts.push(at(t, 0.0));
                pts.push(at(t + a, a));
                pts.push(at(t + a + top, a));
                pts.push(at(t + 2.0 * a + top, 0.0));
                t += 2.0 * a + 2.0 * top;
            }
            pts.push(s.b);
            let segs: Vec<RoutedSegment> = pts
                .windows(2)
                .filter(|p| dist(p[0], p[1]) > 1e-6)
                .map(|p| RoutedSegment {
                    layer: s.layer.clone(),
                    a: p[0],
                    b: p[1],
                    width: w,
                })
                .collect();
            let edge = opts.edge_clearance + w / 2.0;
            let inside = pts.iter().all(|p| {
                p.0 >= outline.0 + edge
                    && p.0 <= outline.2 - edge
                    && p.1 >= outline.1 + edge
                    && p.1 <= outline.3 - edge
            });
            let bit = (ctx.layer_bit)(&s.layer);
            let clear = inside
                && segs.iter().all(|g| {
                    obstacles.iter().all(|o| {
                        o.net == Some(net)
                            || o.layers & bit == 0
                            || shape_segment_distance(&o.shape, g.a, g.b)
                                >= opts.clearance + w / 2.0 + 0.005
                    })
                })
                && segs.iter().all(|g| {
                    partner.iter().filter(|o| o.layer == g.layer).all(|o| {
                        shape_segment_distance(&Shape::Capsule(o.a, o.b, o.width / 2.0), g.a, g.b)
                            >= opts.clearance + w / 2.0 + 0.005
                    })
                })
                && r.vias.iter().all(|v| {
                    dist(*v, s.a) < 1e-6
                        || dist(*v, s.b) < 1e-6
                        || segs.iter().all(|g| {
                            segment_distance(g.a, g.b, *v) >= opts.via_size / 2.0 + w / 2.0
                        })
                });
            if clear {
                let gain = per_bump(a) * k as f64;
                remaining -= gain;
                added += gain;
                replace.push((si, segs));
                break;
            }
        }
    }
    replace.sort_by_key(|r| std::cmp::Reverse(r.0));
    for (si, segs) in replace {
        r.segments.splice(si..=si, segs);
    }
    added
}

fn shape_segment_distance(shape: &Shape, a: (f64, f64), b: (f64, f64)) -> f64 {
    let len = dist(a, b);
    let steps = (len / 0.02).ceil().max(1.0) as usize;
    (0..=steps)
        .map(|k| {
            let t = k as f64 / steps as f64;
            shape.distance((a.0 + t * (b.0 - a.0), a.1 + t * (b.1 - a.1)))
        })
        .fold(f64::INFINITY, f64::min)
}

pub(crate) fn component_count(board: &Board, net: &str, pads: &[usize]) -> usize {
    connectivity(board, net, pads).len()
}

fn connectivity(board: &Board, net: &str, pads: &[usize]) -> Vec<Vec<usize>> {
    let segs: Vec<&super::model::Segment> = board
        .segments
        .iter()
        .filter(|s| s.net.as_deref() == Some(net))
        .collect();
    let vias: Vec<&super::model::Via> = board
        .vias
        .iter()
        .filter(|v| v.net.as_deref() == Some(net))
        .collect();
    let n = pads.len() + segs.len() + vias.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(p: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while p[r] != r {
            r = p[r];
        }
        let mut c = i;
        while p[c] != r {
            let next = p[c];
            p[c] = r;
            c = next;
        }
        r
    }
    let union = |p: &mut Vec<usize>, a: usize, b: usize| {
        let (ra, rb) = (find(p, a), find(p, b));
        if ra != rb {
            p[ra] = rb;
        }
    };
    let tol = 0.02;
    for (si, s) in segs.iter().enumerate() {
        for (pi, &pad) in pads.iter().enumerate() {
            let p = &board.pads[pad];
            if p.on_layer(&s.layer)
                && (p.distance_to(s.a) <= tol + s.width / 2.0
                    || p.distance_to(s.b) <= tol + s.width / 2.0)
            {
                union(&mut parent, pi, pads.len() + si);
            }
        }
        for (sj, t) in segs.iter().enumerate().skip(si + 1) {
            if s.layer == t.layer {
                let touch = segment_distance(s.a, s.b, t.a) <= tol
                    || segment_distance(s.a, s.b, t.b) <= tol
                    || segment_distance(t.a, t.b, s.a) <= tol
                    || segment_distance(t.a, t.b, s.b) <= tol;
                if touch {
                    union(&mut parent, pads.len() + si, pads.len() + sj);
                }
            }
        }
        for (vi, v) in vias.iter().enumerate() {
            if dist(v.at, s.a) <= v.size / 2.0 || dist(v.at, s.b) <= v.size / 2.0 {
                union(&mut parent, pads.len() + si, pads.len() + segs.len() + vi);
            }
        }
    }
    for (vi, v) in vias.iter().enumerate() {
        for (pi, &pad) in pads.iter().enumerate() {
            if board.pads[pad].distance_to(v.at) <= v.size / 2.0 {
                union(&mut parent, pi, pads.len() + segs.len() + vi);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for (pi, &pad) in pads.iter().enumerate() {
        let r = find(&mut parent, pi);
        groups.entry(r).or_default().push(pad);
    }
    let mut out: Vec<Vec<usize>> = groups.into_values().collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.len()));
    out
}

fn route_net(
    ctx: &RouteCtx<'_>,
    plan: &NetPlan,
    obstacles: &[Obstacle],
    extras: &Extras,
) -> NetResult {
    let (board, grid, layers, opts) = (ctx.board, ctx.grid, ctx.layers, ctx.opts);
    let total = plan.components.len() - 1;
    let mut result = NetResult {
        net: plan.name.clone(),
        status: "failed",
        connections_total: total,
        ..Default::default()
    };
    let own_pads: Vec<usize> = plan.components.iter().flatten().copied().collect();
    let neck_width = own_pads
        .iter()
        .filter_map(|i| ctx.pad_neck.get(i).copied())
        .reduce(f64::min)
        .filter(|w| *w + 1e-9 < plan.width);
    let per = grid.nx * grid.ny;
    let mut neck_zone = vec![false; per];
    if neck_width.is_some() {
        for &i in &own_pads {
            if !ctx.pad_neck.contains_key(&i) {
                continue;
            }
            let p = &board.pads[i];
            let (hx, hy) = p.half_extent();
            let reach = hx.max(hy) + 0.8;
            let (x0, y0, x1, y1) = grid.cells_near(p.bbox(), reach);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if dist(p.at, grid.center(x, y)) <= reach {
                        neck_zone[y * grid.nx + x] = true;
                    }
                }
            }
        }
    }
    let half_of = |xy: usize| -> f64 {
        match neck_width {
            Some(w) if neck_zone[xy] => w / 2.0,
            _ => plan.width / 2.0,
        }
    };
    let wide_keep = opts.clearance + plan.width / 2.0;
    let margin = cell_margin(
        grid.pitch,
        opts.clearance + neck_width.unwrap_or(plan.width) / 2.0,
    );
    let n = per * grid.nl;
    let mut blocked = vec![false; n];
    let mut via_blocked = vec![false; per];
    let via_keep = opts.clearance + opts.via_size / 2.0;
    let mut exempt = vec![false; per];
    if let Some((_, _, pads)) = &extras.inflate {
        for &i in pads {
            let p = &board.pads[i];
            let (x0, y0, x1, y1) = grid.cells_near(p.bbox(), 1.5);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if p.distance_to(grid.center(x, y)) <= 1.5 {
                        exempt[y * grid.nx + x] = true;
                    }
                }
            }
        }
    }

    let outline = board.outline.unwrap_or_default();
    for y in 0..grid.ny {
        for x in 0..grid.nx {
            let (cx, cy) = grid.center(x, y);
            let edge = opts.edge_clearance + half_of(y * grid.nx + x);
            let inside = cx >= outline.0 + edge
                && cx <= outline.2 - edge
                && cy >= outline.1 + edge
                && cy <= outline.3 - edge;
            if !inside {
                for l in 0..grid.nl {
                    blocked[grid.idx(l, x, y)] = true;
                }
            }
            let via_edge = opts.edge_clearance + opts.via_size / 2.0;
            let via_inside = cx >= outline.0 + via_edge
                && cx <= outline.2 - via_edge
                && cy >= outline.1 + via_edge
                && cy <= outline.3 - via_edge;
            if !via_inside {
                via_blocked[y * grid.nx + x] = true;
            }
        }
    }
    for o in obstacles {
        if o.net == Some(plan.id) {
            continue;
        }
        let extra = match &extras.inflate {
            Some((e, partner, _)) if o.net != Some(*partner) => *e,
            _ => 0.0,
        };
        let bbox = o.shape.bbox();
        let reach = (wide_keep + extra).max(via_keep) + margin;
        let (x0, y0, x1, y1) = grid.cells_near(bbox, reach);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let xy = y * grid.nx + x;
                let d = o.shape.distance(grid.center(x, y));
                let grow = if exempt[xy] { 0.0 } else { extra };
                if d < opts.clearance + half_of(xy) + margin + grow {
                    for l in 0..grid.nl {
                        if o.layers & (1 << l) != 0 {
                            blocked[grid.idx(l, x, y)] = true;
                        }
                    }
                }
                if d < via_keep + margin {
                    via_blocked[xy] = true;
                }
            }
        }
    }
    for p in &board.pads {
        if p.net.as_deref() == Some(plan.name.as_str()) && !p.through_hole {
            let (x0, y0, x1, y1) = grid.cells_near(p.bbox(), opts.via_size / 2.0);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    if p.distance_to(grid.center(x, y)) < opts.via_size / 2.0 {
                        via_blocked[y * grid.nx + x] = true;
                    }
                }
            }
        }
    }

    // Terminal cells: grid cells inside the pad that keep clearance; when none
    // does, the inside cell nearest the pad centre (or, for a pad smaller than
    // the grid, the nearest cell plus a stub track back to the pad centre).
    let mut stubs: HashMap<usize, (f64, f64)> = HashMap::new();
    let mut pad_of: HashMap<usize, usize> = HashMap::new();
    let mut terminal_cells = |pads: &[usize], blocked: &[bool]| -> Vec<usize> {
        let mut cells = Vec::new();
        for &pi in pads {
            let p = &board.pads[pi];
            let (x0, y0, x1, y1) = grid.cells_near(p.bbox(), 0.0);
            for (l, name) in layers.iter().enumerate() {
                if !p.on_layer(name) && !p.through_hole {
                    continue;
                }
                let mut inside = Vec::new();
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        if p.distance_to(grid.center(x, y)) <= 1e-9 {
                            inside.push(grid.idx(l, x, y));
                        }
                    }
                }
                let free: Vec<usize> = inside.iter().copied().filter(|&c| !blocked[c]).collect();
                for &c in &inside {
                    pad_of.insert(c, pi);
                }
                if !free.is_empty() {
                    cells.extend(free);
                } else if let Some(&c) = inside.iter().min_by(|&&a, &&b| {
                    let (_, ax, ay) = grid.coord(a);
                    let (_, bx, by) = grid.coord(b);
                    dist(grid.center(ax, ay), p.at)
                        .partial_cmp(&dist(grid.center(bx, by), p.at))
                        .unwrap_or(Ordering::Equal)
                }) {
                    cells.push(c);
                } else {
                    let gx = (((p.at.0 - grid.x0) / grid.pitch).round().max(0.0) as usize)
                        .min(grid.nx - 1);
                    let gy = (((p.at.1 - grid.y0) / grid.pitch).round().max(0.0) as usize)
                        .min(grid.ny - 1);
                    let c = grid.idx(l, gx, gy);
                    stubs.insert(c, p.at);
                    cells.push(c);
                }
            }
        }
        cells
    };

    let cell_owner: Vec<Vec<usize>> = plan
        .components
        .iter()
        .map(|c| terminal_cells(c, &blocked))
        .collect();
    let mut connected = vec![false; plan.components.len()];
    connected[0] = true;
    let mut tree: Vec<usize> = cell_owner[0].clone();
    for cells in &cell_owner {
        for &c in cells {
            blocked[c] = false;
        }
    }
    let bonus = extras.couple.as_deref();

    let mut routed = 0;
    loop {
        let remaining: Vec<usize> = (0..plan.components.len())
            .filter(|&i| !connected[i])
            .collect();
        if remaining.is_empty() {
            break;
        }
        let mut target_of: HashMap<usize, usize> = HashMap::new();
        let mut target_pts: Vec<(f64, f64)> = Vec::new();
        for &ci in &remaining {
            for &c in &cell_owner[ci] {
                target_of.insert(c, ci);
            }
            target_pts.extend(plan.terminal_points[ci].iter().copied());
        }
        let Some((path, reached)) = astar(
            grid,
            layers,
            &blocked,
            &via_blocked,
            &tree,
            &target_of,
            &target_pts,
            opts,
            &plan.name,
            bonus,
        ) else {
            result.detail = Some(format!(
                "no path found for {} of {} connections",
                remaining.len(),
                total
            ));
            break;
        };
        connected[reached] = true;
        routed += 1;
        let widths = Widths {
            wide: plan.width,
            neck: neck_width,
            zone: &neck_zone,
        };
        let (segs, vias) = emit(grid, layers, &path, &widths, &blocked);
        // Tracks start and end at the pad centre, the way KiCad's interactive
        // router lands them, joined to the grid octilinearly inside the pad.
        let mut joined: Vec<RoutedSegment> = Vec::new();
        let mut tail: Vec<RoutedSegment> = Vec::new();
        for (k, end) in [path.first(), path.last()].into_iter().enumerate() {
            let Some(end) = end else { continue };
            let (l, x, y) = grid.coord(*end);
            let c = grid.center(x, y);
            let w = match neck_width {
                Some(nw) if neck_zone[y * grid.nx + x] => nw,
                _ => plan.width,
            };
            let centre = if let Some(&pad_at) = stubs.get(end) {
                Some((pad_at, vec![(pad_at, c)]))
            } else {
                pad_of.get(end).and_then(|&pi| {
                    let p = &board.pads[pi];
                    let fits = p.size.0.min(p.size.1) + 1e-6 >= w;
                    (fits && dist(c, p.at) > 1e-6).then(|| (p.at, octilinear(p.at, c)))
                })
            };
            let Some((_, pieces)) = centre else { continue };
            let pieces: Vec<RoutedSegment> = pieces
                .into_iter()
                .map(|(a, b)| RoutedSegment {
                    layer: layers[l].clone(),
                    a,
                    b,
                    width: w,
                })
                .collect();
            if k == 0 {
                joined.extend(pieces);
            } else {
                tail.extend(pieces.into_iter().rev().map(|s| RoutedSegment {
                    a: s.b,
                    b: s.a,
                    ..s
                }));
            }
        }
        joined.extend(segs);
        joined.extend(tail);
        let segs = merge_collinear(joined);
        result.segments.extend(segs);
        result.vias.extend(vias);
        tree.extend(path.iter().copied());
        tree.extend(cell_owner[reached].iter().copied());
    }
    result.connections_routed = routed;
    result.status = if routed == total {
        "routed"
    } else if routed > 0 {
        "partial"
    } else {
        "failed"
    };
    if let (Some(w), true) = (neck_width, routed > 0) {
        let note = format!("necked to {w} mm at fine-pitch pads");
        result.detail = Some(match result.detail.take() {
            Some(d) => format!("{d}; {note}"),
            None => note,
        });
    }
    result
}

struct Widths<'a> {
    wide: f64,
    neck: Option<f64>,
    zone: &'a [bool],
}

#[allow(clippy::too_many_arguments)]
fn astar(
    grid: &Grid,
    layers: &[String],
    blocked: &[bool],
    via_blocked: &[bool],
    sources: &[usize],
    targets: &HashMap<usize, usize>,
    target_pts: &[(f64, f64)],
    opts: &RouteOptions,
    net: &str,
    bonus: Option<&[bool]>,
) -> Option<(Vec<usize>, usize)> {
    const COUPLED: f64 = 0.45;
    let h_scale = if bonus.is_some() { COUPLED } else { 1.0 };
    let n = blocked.len();
    let mut g = vec![f64::INFINITY; n];
    let mut parent = vec![usize::MAX; n];
    let mut dir_of = vec![8u8; n];
    let mut heap = BinaryHeap::new();
    let p = grid.pitch;
    let heuristic = |cell: usize| -> f64 {
        let (_, x, y) = grid.coord(cell);
        let c = grid.center(x, y);
        target_pts
            .iter()
            .map(|t| {
                let dx = (t.0 - c.0).abs();
                let dy = (t.1 - c.1).abs();
                dx.max(dy) + (std::f64::consts::SQRT_2 - 1.0) * dx.min(dy)
            })
            .fold(f64::INFINITY, f64::min)
            * h_scale
    };
    let layer_cost: Vec<f64> = layers
        .iter()
        .map(|l| match &opts.prefer_layer {
            Some(pref) if pref != l && !is_ground(net) => 1.6,
            _ => 1.0,
        })
        .collect();
    for &s in sources {
        if s < n && g[s] > 0.0 {
            g[s] = 0.0;
            heap.push(Node {
                f: heuristic(s),
                cell: s,
            });
        }
    }
    let mut expansions = 0usize;
    while let Some(Node { cell, .. }) = heap.pop() {
        if let Some(&component) = targets.get(&cell) {
            let mut path = vec![cell];
            let mut c = cell;
            while parent[c] != usize::MAX {
                c = parent[c];
                path.push(c);
            }
            path.reverse();
            return Some((path, component));
        }
        expansions += 1;
        if expansions > opts.max_expansions {
            return None;
        }
        let (l, x, y) = grid.coord(cell);
        let gc = g[cell];
        for (d, (dx, dy)) in DIRS.iter().enumerate() {
            let nx = x as i32 + dx;
            let ny = y as i32 + dy;
            if nx < 0 || ny < 0 || nx >= grid.nx as i32 || ny >= grid.ny as i32 {
                continue;
            }
            let (nx, ny) = (nx as usize, ny as usize);
            let next = grid.idx(l, nx, ny);
            if blocked[next] {
                continue;
            }
            let diagonal = dx != &0 && dy != &0;
            if diagonal && (blocked[grid.idx(l, nx, y)] || blocked[grid.idx(l, x, ny)]) {
                continue;
            }
            let mut step = if diagonal {
                p * std::f64::consts::SQRT_2
            } else {
                p
            } * layer_cost[l];
            if opts.layer_directions && grid.nl > 1 {
                let horizontal_layer = l % 2 == 0;
                step *= if diagonal {
                    1.15
                } else if (*dy == 0) == horizontal_layer {
                    1.0
                } else {
                    1.6
                };
            }
            if bonus.is_some_and(|b| b[next]) {
                step *= COUPLED;
            }
            let prev = dir_of[cell];
            if prev != 8 {
                let turn = ((d as i32 - prev as i32).rem_euclid(8))
                    .min((prev as i32 - d as i32).rem_euclid(8));
                step += match turn {
                    0 => 0.0,
                    1 => p * 0.4,
                    2 => p * 1.5,
                    _ => p * 20.0,
                };
            }
            let cost = gc + step;
            if cost < g[next] {
                g[next] = cost;
                parent[next] = cell;
                dir_of[next] = d as u8;
                heap.push(Node {
                    f: cost + heuristic(next),
                    cell: next,
                });
            }
        }
        if grid.nl > 1 && !via_blocked[y * grid.nx + x] {
            for ol in 0..grid.nl {
                if ol == l {
                    continue;
                }
                let next = grid.idx(ol, x, y);
                if blocked[next] {
                    continue;
                }
                let cost = gc + opts.via_cost;
                if cost < g[next] {
                    g[next] = cost;
                    parent[next] = cell;
                    dir_of[next] = 8;
                    heap.push(Node {
                        f: cost + heuristic(next),
                        cell: next,
                    });
                }
            }
        }
    }
    None
}

fn emit(
    grid: &Grid,
    layers: &[String],
    path: &[usize],
    widths: &Widths<'_>,
    blocked: &[bool],
) -> (Vec<RoutedSegment>, Vec<(f64, f64)>) {
    let mut segments = Vec::new();
    let mut vias = Vec::new();
    let mut run: Vec<(usize, usize)> = Vec::new();
    let mut run_layer = None;
    let flush = |run: &mut Vec<(usize, usize)>, layer: usize, segments: &mut Vec<RoutedSegment>| {
        if run.len() >= 2 {
            for (piece, width) in split_by_width(grid, run, widths) {
                if piece.len() < 2 {
                    continue;
                }
                let pts = straighten(grid, layer, &piece, blocked);
                for w in pts.windows(2) {
                    let a = grid.center(w[0].0, w[0].1);
                    let b = grid.center(w[1].0, w[1].1);
                    if dist(a, b) > 1e-9 {
                        segments.push(RoutedSegment {
                            layer: layers[layer].clone(),
                            a,
                            b,
                            width,
                        });
                    }
                }
            }
        }
        run.clear();
    };
    for &cell in path {
        let (l, x, y) = grid.coord(cell);
        match run_layer {
            Some(rl) if rl != l => {
                let last = *run.last().expect("run is non-empty");
                flush(&mut run, rl, &mut segments);
                vias.push(grid.center(last.0, last.1));
                run.push(last);
            }
            _ => {}
        }
        run_layer = Some(l);
        if run.last() != Some(&(x, y)) {
            run.push((x, y));
        }
    }
    if let Some(rl) = run_layer {
        flush(&mut run, rl, &mut segments);
    }
    (segments, vias)
}

/// Splits a run of cells where the track changes between its full width and
/// the neck-down width. The cell shared by two pieces is always a full-width
/// cell, whose clearance was checked for the wider track.
fn split_by_width(
    grid: &Grid,
    run: &[(usize, usize)],
    widths: &Widths<'_>,
) -> Vec<(Vec<(usize, usize)>, f64)> {
    let Some(neck) = widths.neck else {
        return vec![(run.to_vec(), widths.wide)];
    };
    let is_neck = |c: &(usize, usize)| widths.zone[c.1 * grid.nx + c.0];
    let mut pieces: Vec<(Vec<(usize, usize)>, bool)> = Vec::new();
    for c in run {
        let flag = is_neck(c);
        match pieces.last_mut() {
            Some((cells, f)) if *f == flag => cells.push(*c),
            _ => pieces.push((vec![*c], flag)),
        }
    }
    for i in 1..pieces.len() {
        if pieces[i - 1].1 {
            let first = pieces[i].0[0];
            pieces[i - 1].0.push(first);
        } else {
            let last = *pieces[i - 1].0.last().expect("piece is non-empty");
            pieces[i].0.insert(0, last);
        }
    }
    pieces
        .into_iter()
        .map(|(cells, f)| (cells, if f { neck } else { widths.wide }))
        .collect()
}

fn straighten(
    grid: &Grid,
    layer: usize,
    run: &[(usize, usize)],
    blocked: &[bool],
) -> Vec<(usize, usize)> {
    let mut corners = vec![run[0]];
    for i in 1..run.len() - 1 {
        let d1 = (
            run[i].0 as i32 - run[i - 1].0 as i32,
            run[i].1 as i32 - run[i - 1].1 as i32,
        );
        let d2 = (
            run[i + 1].0 as i32 - run[i].0 as i32,
            run[i + 1].1 as i32 - run[i].1 as i32,
        );
        if d1 != d2 {
            corners.push(run[i]);
        }
    }
    corners.push(*run.last().expect("run has two points"));
    let clear = |a: (usize, usize), b: (usize, usize)| -> bool {
        let dx = b.0 as i32 - a.0 as i32;
        let dy = b.1 as i32 - a.1 as i32;
        if !(dx == 0 || dy == 0 || dx.abs() == dy.abs()) {
            return false;
        }
        let steps = dx.abs().max(dy.abs());
        let (sx, sy) = (dx.signum(), dy.signum());
        for k in 0..=steps {
            let x = (a.0 as i32 + sx * k) as usize;
            let y = (a.1 as i32 + sy * k) as usize;
            if blocked[grid.idx(layer, x, y)] {
                return false;
            }
            if sx != 0 && sy != 0 && k < steps {
                let ax = (a.0 as i32 + sx * (k + 1)) as usize;
                let ay = (a.1 as i32 + sy * (k + 1)) as usize;
                if blocked[grid.idx(layer, ax, y)] || blocked[grid.idx(layer, x, ay)] {
                    return false;
                }
            }
        }
        true
    };
    let mut out = vec![corners[0]];
    let mut i = 0;
    while i < corners.len() - 1 {
        let mut j = corners.len() - 1;
        while j > i + 1 && !clear(corners[i], corners[j]) {
            j -= 1;
        }
        out.push(corners[j]);
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::model::{fixture, Board};
    use super::*;

    fn board(body: &str) -> Board {
        Board::from_text(&fixture::board(body)).unwrap()
    }

    fn octilinear(s: &RoutedSegment) -> bool {
        let dx = (s.b.0 - s.a.0).abs();
        let dy = (s.b.1 - s.a.1).abs();
        dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6
    }

    /// A board with its own net list; pads are (number, x, y, w, h, net).
    type TestPad<'a> = (String, f64, f64, f64, f64, &'a str);
    type TestPart<'a> = (&'a str, &'a str, f64, f64, Vec<TestPad<'a>>);

    fn custom(outline: (f64, f64), nets: &[&str], parts: &[TestPart<'_>]) -> Board {
        let code = |n: &str| {
            nets.iter()
                .position(|x| *x == n)
                .map(|i| i + 1)
                .unwrap_or(0)
        };
        let mut text = String::from(
            "(kicad_pcb (version 20250114) (generator \"test\")\n  (layers (0 \"F.Cu\" signal) (2 \"B.Cu\" signal) (25 \"Edge.Cuts\" user))\n  (net 0 \"\")",
        );
        for (i, n) in nets.iter().enumerate() {
            text.push_str(&format!(" (net {} \"{n}\")", i + 1));
        }
        text.push_str(&format!(
            "\n  (gr_rect (start 100 100) (end {} {}) (stroke (width 0.05) (type default)) (fill none) (layer \"Edge.Cuts\"))\n",
            outline.0, outline.1
        ));
        for (reference, lib, x, y, pads) in parts {
            let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
            let mut body = String::new();
            for (num, px, py, w, h, net) in pads {
                x0 = x0.min(px - w / 2.0);
                y0 = y0.min(py - h / 2.0);
                x1 = x1.max(px + w / 2.0);
                y1 = y1.max(py + h / 2.0);
                let net = if net.is_empty() {
                    String::new()
                } else {
                    format!(" (net {} \"{net}\")", code(net))
                };
                if lib.starts_with("Connector_PinHeader") {
                    body.push_str(&format!("    (pad \"{num}\" thru_hole circle (at {px} {py}) (size {w} {h}) (drill 1) (layers \"*.Cu\" \"*.Mask\"){net})\n"));
                } else {
                    body.push_str(&format!("    (pad \"{num}\" smd rect (at {px} {py}) (size {w} {h}) (layers \"F.Cu\" \"F.Paste\" \"F.Mask\"){net})\n"));
                }
            }
            text.push_str(&format!(
                "  (footprint \"{lib}\" (layer \"F.Cu\") (at {x} {y})\n    (property \"Reference\" \"{reference}\" (at 0 -3 0) (layer \"F.SilkS\"))\n    (fp_rect (start {} {}) (end {} {}) (layer \"F.CrtYd\"))\n{body}  )\n",
                x0 - 0.25, y0 - 0.25, x1 + 0.25, y1 + 0.25
            ));
        }
        text.push(')');
        Board::from_text(&text).unwrap()
    }

    fn pad(num: usize, x: f64, y: f64, w: f64, h: f64, net: &str) -> TestPad<'_> {
        (num.to_string(), x, y, w, h, net)
    }

    /// Writes the board and its routes as an SVG when VOLTARIS_SVG_DIR is set.
    fn dump_svg(name: &str, b: &Board, r: &RouteResult) {
        let Ok(dir) = std::env::var("VOLTARIS_SVG_DIR") else {
            return;
        };
        let o = b.outline.unwrap();
        let mut svg = format!(
            "<svg xmlns='http://www.w3.org/2000/svg' viewBox='{} {} {} {}' width='1200' style='background:#0b2a17'>",
            o.0, o.1, o.2 - o.0, o.3 - o.1
        );
        for p in &b.pads {
            let bb = p.bbox();
            svg.push_str(&format!(
                "<rect x='{}' y='{}' width='{}' height='{}' fill='#c8a85a'/>",
                bb.0,
                bb.1,
                bb.2 - bb.0,
                bb.3 - bb.1
            ));
        }
        for n in &r.nets {
            for s in &n.segments {
                let c = if s.layer == "F.Cu" {
                    "#d04040"
                } else {
                    "#4060d0"
                };
                svg.push_str(&format!("<line x1='{}' y1='{}' x2='{}' y2='{}' stroke='{c}' stroke-width='{}' stroke-linecap='round' opacity='0.85'/>", s.a.0, s.a.1, s.b.0, s.b.1, s.width));
            }
            for v in &n.vias {
                svg.push_str(&format!(
                    "<circle cx='{}' cy='{}' r='0.4' fill='#ddd'/>",
                    v.0, v.1
                ));
            }
        }
        svg.push_str("</svg>");
        std::fs::write(format!("{dir}/{name}.svg"), svg).unwrap();
    }

    #[test]
    fn detects_differential_pairs_by_name() {
        let nets: Vec<String> = [
            "/D+", "/D-", "USB_DP", "USB_DM", "TX_P", "TX_N", "V+", "V-", "+5V", "CLKP", "CLKN",
            "SIG",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let pairs = find_pairs(&nets);
        let has = |p: &str, n: &str| pairs.iter().any(|(a, b)| a == p && b == n);
        assert!(has("/D+", "/D-"), "{pairs:?}");
        assert!(has("USB_DP", "USB_DM"), "{pairs:?}");
        assert!(has("TX_P", "TX_N"), "{pairs:?}");
        assert!(has("CLKP", "CLKN"), "{pairs:?}");
        assert!(!pairs.iter().any(|(a, _)| a == "V+"), "{pairs:?}");
        assert_eq!(pairs.len(), 4, "{pairs:?}");
    }

    fn usb_board(stagger: f64) -> Board {
        custom(
            (130.0, 120.0),
            &["GND", "D+", "D-"],
            &[
                (
                    "J1",
                    "Connector_USB:USB_C_Receptacle",
                    103.0,
                    110.0,
                    vec![
                        pad(1, 0.0, -0.25, 1.0, 0.3, "D+"),
                        pad(2, 0.0, 0.25, 1.0, 0.3, "D-"),
                        pad(3, 0.0, -1.5, 1.0, 0.6, "GND"),
                    ],
                ),
                (
                    "U1",
                    "Package_SO:SOIC-8",
                    125.0,
                    114.0,
                    vec![
                        pad(1, -1.0, 0.0, 1.0, 0.35, "D+"),
                        pad(2, -1.0 + stagger, 0.65, 1.0, 0.35, "D-"),
                        pad(3, 1.0, 0.0, 1.0, 0.35, "GND"),
                    ],
                ),
            ],
        )
    }

    #[test]
    fn routes_a_usb_pair_coupled_and_length_matched() {
        let b = usb_board(1.2);
        let opts = RouteOptions {
            layers: vec!["F.Cu".into()],
            only_nets: Some(vec!["D+".into(), "D-".into()]),
            ..Default::default()
        };
        let r = route(&b, &opts).unwrap();
        assert_eq!(r.pairs.len(), 1, "{:?}", r.warnings);
        let p = &r.pairs[0];
        eprintln!("{p:?}");
        dump_svg("usb_pair", &b, &r);
        assert_eq!(p.status, "routed");
        assert!(p.coupled_pct >= 50.0, "only {:.0}% coupled", p.coupled_pct);
        assert!(p.tuned_mm > 0.3, "the stagger should need tuning: {p:?}");
        assert!(p.skew <= opts.pair_skew + 0.02, "skew {:.3} mm", p.skew);
        let v = super::check::violations(&b, &r, opts.clearance, opts.via_size);
        assert!(v.is_empty(), "{v:?}");
        for n in &r.nets {
            assert!(
                n.segments.iter().all(|s| {
                    let dx = (s.b.0 - s.a.0).abs();
                    let dy = (s.b.1 - s.a.1).abs();
                    dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6
                }),
                "{} has off-angle segments",
                n.net
            );
        }
    }

    #[test]
    fn escapes_a_half_millimetre_pitch_part_without_settings() {
        let mut u1 = Vec::new();
        let nets: Vec<String> = (0..8).map(|i| format!("S{i}")).collect();
        let net_refs: Vec<&str> = nets.iter().map(String::as_str).collect();
        for (i, net) in net_refs.iter().enumerate() {
            u1.push(pad(i + 1, -1.5, -1.75 + i as f64 * 0.5, 0.8, 0.25, net));
            u1.push(pad(i + 9, 1.5, -1.75 + i as f64 * 0.5, 0.8, 0.25, ""));
        }
        let header: Vec<_> = (0..8)
            .map(|i| pad(i + 1, 0.0, i as f64 * 2.54, 1.7, 1.7, net_refs[i]))
            .collect();
        let b = custom(
            (140.0, 125.0),
            &net_refs,
            &[
                ("U1", "Package_DFN_QFN:QFN-16", 125.0, 112.5, u1),
                (
                    "J1",
                    "Connector_PinHeader_2.54mm:PinHeader_1x08",
                    104.0,
                    103.0,
                    header,
                ),
            ],
        );
        let opts = RouteOptions {
            layers: vec!["F.Cu".into()],
            ..Default::default()
        };
        let r = route(&b, &opts).unwrap();
        eprintln!(
            "grid {} fine {:?} warnings {:?}",
            r.grid, r.fine_pitch, r.warnings
        );
        dump_svg("fine_pitch", &b, &r);
        let (done, total) = r.routed_connections();
        assert_eq!(
            (done, total),
            (8, 8),
            "{:?}",
            r.nets
                .iter()
                .map(|n| (&n.net, n.status, &n.detail))
                .collect::<Vec<_>>()
        );
        assert!(r.grid < 0.2, "grid {}", r.grid);
        let u1 = r
            .fine_pitch
            .iter()
            .find(|f| f.reference == "U1")
            .expect("U1 is fine pitch");
        assert!((u1.pitch - 0.5).abs() < 1e-6);
        let v = super::check::violations(&b, &r, opts.clearance, opts.via_size);
        assert!(v.is_empty(), "{v:?}");

        let coarse = route(
            &b,
            &RouteOptions {
                grid: Some(0.2),
                fine_pitch: false,
                ..opts.clone()
            },
        )
        .unwrap();
        let v = super::check::violations(&b, &coarse, opts.clearance, opts.via_size);
        eprintln!(
            "coarse 0.2 mm grid: {:?} routed, {} violations",
            coarse.routed_connections(),
            v.len()
        );
    }

    #[test]
    fn routes_a_two_pin_net_with_octilinear_segments() {
        let b = board(&format!(
            "{}{}",
            fixture::header("J1", 104.0, 106.0, &["SIG", "GND"]),
            fixture::smd("R1", 120.0, 112.0, 0.0, ("SIG", "+5V"))
        ));
        let r = route(
            &b,
            &RouteOptions {
                only_nets: Some(vec!["SIG".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let sig = r.nets.iter().find(|n| n.net == "SIG").unwrap();
        assert_eq!(sig.status, "routed");
        assert!(!sig.segments.is_empty());
        assert!(sig.segments.iter().all(octilinear));
        assert!(sig.segments.len() <= 4, "{:?}", sig.segments);
    }

    #[test]
    fn lands_tracks_on_pad_centres_and_never_between_passive_pads() {
        let b = board(&format!(
            "{}{}{}",
            fixture::header("J1", 104.0, 108.0, &["SIG", "GND"]),
            fixture::smd("R1", 125.0, 110.0, 0.0, ("SIG", "+5V")),
            fixture::smd("R2", 114.0, 109.0, 0.0, ("GND", "Net-(D1-A)"))
        ));
        let r = route(
            &b,
            &RouteOptions {
                only_nets: Some(vec!["SIG".into()]),
                layers: vec!["F.Cu".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let sig = r.nets.iter().find(|n| n.net == "SIG").unwrap();
        assert_eq!(sig.status, "routed");
        let ends: Vec<(f64, f64)> = sig.segments.iter().flat_map(|s| [s.a, s.b]).collect();
        for p in b.pads.iter().filter(|p| p.net.as_deref() == Some("SIG")) {
            assert!(
                ends.iter().any(|e| dist(*e, p.at) < 1e-6),
                "no track lands on the centre of {}.{}",
                p.reference,
                p.number
            );
        }
        let gap = between_pad_areas(&b, &["F.Cu".to_string()]);
        assert!(!gap.is_empty());
        for s in &sig.segments {
            for (g, _) in &gap {
                for k in 0..=50 {
                    let t = k as f64 / 50.0;
                    let q = (s.a.0 + t * (s.b.0 - s.a.0), s.a.1 + t * (s.b.1 - s.a.1));
                    assert!(
                        super::super::model::rect_distance(*g, q) > s.width / 2.0,
                        "track passes between R2's pads"
                    );
                }
            }
        }
        assert!(sig.segments.iter().all(|s| {
            let dx = (s.b.0 - s.a.0).abs();
            let dy = (s.b.1 - s.a.1).abs();
            dx < 1e-6 || dy < 1e-6 || (dx - dy).abs() < 1e-6
        }));
    }

    #[test]
    fn keeps_clearance_from_foreign_pads() {
        let b = board(&format!(
            "{}{}{}",
            fixture::smd("R1", 105.0, 110.0, 0.0, ("SIG", "+5V")),
            fixture::smd("R2", 125.0, 110.0, 0.0, ("+5V", "SIG")),
            fixture::smd("R3", 115.0, 110.0, 90.0, ("GND", "Net-(D1-A)"))
        ));
        let r = route(
            &b,
            &RouteOptions {
                only_nets: Some(vec!["SIG".into()]),
                layers: vec!["F.Cu".into()],
                ..Default::default()
            },
        )
        .unwrap();
        let sig = r.nets.iter().find(|n| n.net == "SIG").unwrap();
        assert_eq!(sig.status, "routed");
        for s in &sig.segments {
            for p in b.pads.iter().filter(|p| p.net.as_deref() != Some("SIG")) {
                let bb = p.bbox();
                for k in 0..=20 {
                    let t = k as f64 / 20.0;
                    let q = (s.a.0 + t * (s.b.0 - s.a.0), s.a.1 + t * (s.b.1 - s.a.1));
                    let d = super::super::model::rect_distance(bb, q) - s.width / 2.0;
                    assert!(
                        d >= 0.2 - 1e-6,
                        "too close to {}.{}: {d}",
                        p.reference,
                        p.number
                    );
                }
            }
        }
    }

    #[test]
    fn uses_a_via_when_the_top_is_walled_off() {
        let mut body = String::new();
        body.push_str(&fixture::smd("R1", 104.0, 110.0, 0.0, ("SIG", "+5V")));
        body.push_str(&fixture::smd("R2", 126.0, 110.0, 0.0, ("SIG", "+5V")));
        body.push_str(
            r#"  (segment (start 115 100.6) (end 115 119.4) (width 1) (layer "F.Cu") (net 1 "GND"))
"#,
        );
        let b = board(&body);
        let r = route(
            &b,
            &RouteOptions {
                only_nets: Some(vec!["SIG".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let sig = r.nets.iter().find(|n| n.net == "SIG").unwrap();
        assert_eq!(sig.status, "routed");
        assert_eq!(sig.vias.len(), 2);
        assert!(sig.segments.iter().any(|s| s.layer == "B.Cu"));
    }

    #[test]
    fn skips_nets_already_connected_and_pours() {
        let mut body = String::new();
        body.push_str(&fixture::smd("R1", 104.0, 110.0, 0.0, ("GND", "SIG")));
        body.push_str(&fixture::smd("R2", 110.0, 110.0, 0.0, ("SIG", "GND")));
        body.push_str(
            r#"  (segment (start 104.9 110) (end 109.1 110) (width 0.25) (layer "F.Cu") (net 3 "SIG"))
  (zone (net 1) (net_name "GND") (layers "F.Cu" "B.Cu") (polygon (pts (xy 100 100) (xy 130 100) (xy 130 120) (xy 100 120))))
"#,
        );
        let b = board(&body);
        let r = route(&b, &RouteOptions::default()).unwrap();
        let sig = r.nets.iter().find(|n| n.net == "SIG").unwrap();
        assert_eq!(sig.status, "already_connected");
        let gnd = r.nets.iter().find(|n| n.net == "GND").unwrap();
        assert_eq!(gnd.status, "pour");
    }

    #[test]
    fn routes_multi_pin_nets_as_a_tree() {
        let mut body = String::new();
        body.push_str(&fixture::smd("R1", 105.0, 104.0, 0.0, ("+5V", "SIG")));
        body.push_str(&fixture::smd("R2", 125.0, 104.0, 0.0, ("+5V", "SIG")));
        body.push_str(&fixture::smd("R3", 115.0, 116.0, 0.0, ("+5V", "SIG")));
        let b = board(&body);
        let r = route(
            &b,
            &RouteOptions {
                only_nets: Some(vec!["+5V".into()]),
                ..Default::default()
            },
        )
        .unwrap();
        let p = r.nets.iter().find(|n| n.net == "+5V").unwrap();
        assert_eq!(p.connections_total, 2);
        assert_eq!(p.status, "routed");
        assert!(p.segments.iter().all(|s| (s.width - 0.5).abs() < 1e-9));
    }
}

#[cfg(test)]
pub(super) mod check {
    use super::super::model::Board;
    use super::*;

    fn seg_seg(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
        let cross = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| {
            (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
        };
        let (d1, d2, d3, d4) = (
            cross(c, d, a),
            cross(c, d, b),
            cross(a, b, c),
            cross(a, b, d),
        );
        if ((d1 > 0.0) != (d2 > 0.0)) && ((d3 > 0.0) != (d4 > 0.0)) {
            return 0.0;
        }
        segment_distance(a, b, c)
            .min(segment_distance(a, b, d))
            .min(segment_distance(c, d, a))
            .min(segment_distance(c, d, b))
    }

    pub fn violations(b: &Board, r: &RouteResult, clearance: f64, via_size: f64) -> Vec<String> {
        let mut out = Vec::new();
        let tol = 1e-3;
        let copper: Vec<(&str, &RoutedSegment)> = r
            .nets
            .iter()
            .flat_map(|n| n.segments.iter().map(move |s| (n.net.as_str(), s)))
            .collect();
        let vias: Vec<(&str, (f64, f64))> = r
            .nets
            .iter()
            .flat_map(|n| n.vias.iter().map(move |v| (n.net.as_str(), *v)))
            .collect();
        for (net, s) in &copper {
            for p in &b.pads {
                if p.net.as_deref() == Some(*net) || !(p.on_layer(&s.layer) || p.through_hole) {
                    continue;
                }
                for k in 0..=40 {
                    let t = k as f64 / 40.0;
                    let q = (s.a.0 + t * (s.b.0 - s.a.0), s.a.1 + t * (s.b.1 - s.a.1));
                    let d = p.distance_to(q) - s.width / 2.0;
                    if d < clearance - tol {
                        out.push(format!(
                            "{net} track vs pad {}.{} ({}) at {d:.3}",
                            p.reference,
                            p.number,
                            p.net.clone().unwrap_or_default()
                        ));
                        break;
                    }
                }
            }
            for (other, t) in &copper {
                if other == net || t.layer != s.layer {
                    continue;
                }
                let d = seg_seg(s.a, s.b, t.a, t.b) - s.width / 2.0 - t.width / 2.0;
                if d < clearance - tol {
                    out.push(format!("{net} vs {other} tracks on {} at {d:.3}", s.layer));
                }
            }
            for (other, v) in &vias {
                if other == net {
                    continue;
                }
                let d = segment_distance(s.a, s.b, *v) - s.width / 2.0 - via_size / 2.0;
                if d < clearance - tol {
                    out.push(format!("{net} track vs {other} via at {d:.3}"));
                }
            }
        }
        for (net, v) in &vias {
            for p in &b.pads {
                if p.net.as_deref() == Some(*net) {
                    continue;
                }
                let d = p.distance_to(*v) - via_size / 2.0;
                if d < clearance - tol {
                    out.push(format!(
                        "{net} via vs pad {}.{} at {d:.3}",
                        p.reference, p.number
                    ));
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod corpus {
    use super::super::model::Board;
    use super::check::violations;
    use super::*;

    fn reroute(name: &str, opts: &RouteOptions) -> (usize, usize, usize, f64, u128) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../voltaris-sexp/tests/fixtures")
            .join(name);
        let text = std::fs::read_to_string(path).unwrap();
        let mut b = Board::from_text(&text).unwrap();
        b.segments.clear();
        b.vias.clear();
        b.zones.clear();
        let t = std::time::Instant::now();
        let r = route(&b, opts).unwrap();
        let (done, total) = r.routed_connections();
        let v = violations(&b, &r, opts.clearance, opts.via_size);
        assert!(
            v.is_empty(),
            "{} clearance violations, first: {:?}",
            v.len(),
            &v[..v.len().min(5)]
        );
        (done, total, r.vias(), r.length(), t.elapsed().as_millis())
    }

    #[test]
    fn reroutes_the_ecc83_demo_board() {
        let (done, total, vias, len, ms) = reroute("ecc83-pp.kicad_pcb", &RouteOptions::default());
        eprintln!("ecc83: {done}/{total} connections, {vias} vias, {len:.0} mm, {ms} ms");
        assert!(total > 10);
        assert!(done as f64 >= total as f64 * 0.9, "{done}/{total}");
    }

    #[test]
    #[ignore = "large board; run with --ignored for a benchmark"]
    fn reroutes_the_pic_programmer_demo_board() {
        for (directions, between) in [(false, false), (true, false), (false, true)] {
            let opts = RouteOptions {
                layer_directions: directions,
                between_pads: between,
                ..Default::default()
            };
            let (done, total, vias, len, ms) = reroute("pic_programmer.kicad_pcb", &opts);
            eprintln!("pic_programmer (layer directions {directions}, between pads {between}): {done}/{total} connections, {vias} vias, {len:.0} mm, {ms} ms");
            assert!(done as f64 >= total as f64 * 0.8, "{done}/{total}");
        }
    }
}
