//! Placement optimizer: improves a placement by moving, turning and swapping
//! parts while keeping every courtyard clear, every part inside the board and
//! every decoupling capacitor beside its pin. It minimises the total ratsnest
//! length (a minimum spanning tree per net, the same measure KiCad's ratsnest
//! draws), so the routes it leaves behind are shorter and cross less.
//!
//! Two strengths: a gentle greedy pass that only takes improvements and moves
//! each part a few millimetres from where a person put it, and a full
//! simulated-annealing pass for a fresh placement. Both are deterministic.

use super::model::{dist, is_ground, rects_overlap, Board, Rect};
use std::collections::{BTreeMap, HashMap};
use voltaris_sexp::geometry::transform_pad;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freedom {
    /// Never moves.
    Fixed,
    /// Slides along one axis only (a connector on an edge): true = along x.
    Slide(bool),
    /// Moves, turns and swaps freely.
    Free,
}

#[derive(Debug, Clone)]
pub struct RefineOptions {
    pub anneal: bool,
    pub iterations: usize,
    /// Largest distance a part may end up from where it started, in mm.
    pub max_shift: f64,
    pub grid: f64,
    pub gap: f64,
    pub rotate: bool,
    pub seed: u64,
    /// Scales the calibrated starting temperature: 1 for a fresh placement,
    /// lower to anneal gently around a good existing one.
    pub heat: f64,
}

impl Default for RefineOptions {
    fn default() -> Self {
        Self {
            anneal: false,
            iterations: 0,
            max_shift: 5.0,
            grid: 0.25,
            gap: 0.4,
            rotate: true,
            seed: 0x9E37_79B9_7F4A_7C15,
            heat: 1.0,
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RefineStats {
    pub cost_before: f64,
    pub cost_after: f64,
    pub iterations: usize,
    pub accepted: usize,
}

/// A part the optimizer knows about.
#[derive(Debug, Clone)]
pub struct Part {
    pub reference: String,
    pub freedom: Freedom,
    /// Courtyard must stay inside this rectangle.
    pub inside: Rect,
    /// Through-hole parts block both sides.
    pub both_sides: bool,
    /// For a decoupling capacitor: its power pad and the IC pads it serves.
    pub decouples: Option<(usize, Vec<usize>)>,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

struct Model<'a> {
    board: &'a Board,
    parts: Vec<Part>,
    fp: Vec<usize>,
    at: Vec<(f64, f64)>,
    rot: Vec<f64>,
    origin: Vec<(f64, f64)>,
    boxes: Vec<Rect>,
    back: Vec<bool>,
    /// nets → pad indices; pad → (part index or none)
    nets: Vec<Vec<usize>>,
    pad_part: HashMap<usize, usize>,
    part_nets: Vec<Vec<usize>>,
    net_cost: Vec<f64>,
    static_obstacles: Vec<(Rect, bool, bool)>,
    opts: &'a RefineOptions,
}

const DECOUPLING_REACH: f64 = 2.5;
const DECOUPLING_WEIGHT: f64 = 8.0;

impl Model<'_> {
    fn pad_at(&self, pad: usize) -> (f64, f64) {
        match self.pad_part.get(&pad) {
            Some(&k) => {
                let p = &self.board.pads[pad];
                transform_pad(
                    p.local.0,
                    p.local.1,
                    self.at[k].0,
                    self.at[k].1,
                    self.rot[k],
                )
            }
            None => self.board.pads[pad].at,
        }
    }

    fn mst_of(&self, net: usize) -> f64 {
        let pts: Vec<(f64, f64)> = self.nets[net].iter().map(|&p| self.pad_at(p)).collect();
        mst(&pts)
    }

    fn decoupling_penalty(&self, k: usize) -> f64 {
        let Some((cap_pad, ic_pads)) = &self.parts[k].decouples else {
            return 0.0;
        };
        let here = self.pad_at(*cap_pad);
        let d = ic_pads
            .iter()
            .map(|&q| dist(self.pad_at(q), here))
            .fold(f64::INFINITY, f64::min);
        if d.is_finite() {
            DECOUPLING_WEIGHT * (d - DECOUPLING_REACH).max(0.0)
        } else {
            0.0
        }
    }

    /// Decoupling penalties of `k` and of every capacitor serving a pad of `k`.
    fn penalty_around(&self, ks: &[usize]) -> f64 {
        let mut total = 0.0;
        for (j, part) in self.parts.iter().enumerate() {
            let Some((cap_pad, ic_pads)) = &part.decouples else {
                continue;
            };
            let touches = ks.contains(&j)
                || ks.iter().any(|&k| {
                    ic_pads.iter().any(|q| self.pad_part.get(q) == Some(&k))
                        || self.pad_part.get(cap_pad) == Some(&k)
                });
            if touches {
                total += self.decoupling_penalty(j);
            }
        }
        total
    }

    fn total(&self) -> f64 {
        self.net_cost.iter().sum::<f64>()
            + (0..self.parts.len())
                .map(|k| self.decoupling_penalty(k))
                .sum::<f64>()
    }

    fn legal(&self, k: usize, at: (f64, f64), rot: f64) -> Option<Rect> {
        let f = &self.board.footprints[self.fp[k]];
        let c = f.courtyard_at(at, rot);
        let inside = self.parts[k].inside;
        if c.0 < inside.0 - 1e-6
            || c.1 < inside.1 - 1e-6
            || c.2 > inside.2 + 1e-6
            || c.3 > inside.3 + 1e-6
        {
            return None;
        }
        let centre = ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0);
        if !self.opts.max_shift.is_infinite()
            && dist(centre, self.origin[k]) > self.opts.max_shift + 1e-6
        {
            return None;
        }
        let both = self.parts[k].both_sides;
        let side = self.back[k];
        for (j, o) in self.boxes.iter().enumerate() {
            if j == k {
                continue;
            }
            let shares = both || self.parts[j].both_sides || self.back[j] == side;
            if shares && rects_overlap(c, *o, self.opts.gap) {
                return None;
            }
        }
        for (o, ob_back, ob_both) in &self.static_obstacles {
            if (both || *ob_both || *ob_back == side) && rects_overlap(c, *o, self.opts.gap) {
                return None;
            }
        }
        Some(c)
    }

    /// Applies new poses to parts `ks`, returning the cost change; `undo` restores.
    fn delta(
        &mut self,
        ks: &[usize],
        poses: &[((f64, f64), f64, Rect)],
    ) -> (f64, Vec<(usize, f64)>) {
        let mut affected: Vec<usize> = ks
            .iter()
            .flat_map(|&k| self.part_nets[k].iter().copied())
            .collect();
        affected.sort_unstable();
        affected.dedup();
        let pen_before = self.penalty_around(ks);
        for (i, &k) in ks.iter().enumerate() {
            self.at[k] = poses[i].0;
            self.rot[k] = poses[i].1;
            self.boxes[k] = poses[i].2;
        }
        let mut old = Vec::with_capacity(affected.len());
        let mut d = 0.0;
        for n in affected {
            let c = self.mst_of(n);
            d += c - self.net_cost[n];
            old.push((n, self.net_cost[n]));
            self.net_cost[n] = c;
        }
        d += self.penalty_around(ks) - pen_before;
        (d, old)
    }
}

fn mst(pts: &[(f64, f64)]) -> f64 {
    if pts.len() < 2 {
        return 0.0;
    }
    let mut in_tree = vec![false; pts.len()];
    let mut best = vec![f64::INFINITY; pts.len()];
    best[0] = 0.0;
    let mut total = 0.0;
    for _ in 0..pts.len() {
        let mut u = usize::MAX;
        for i in 0..pts.len() {
            if !in_tree[i] && (u == usize::MAX || best[i] < best[u]) {
                u = i;
            }
        }
        in_tree[u] = true;
        total += best[u];
        for v in 0..pts.len() {
            if !in_tree[v] {
                best[v] = best[v].min(dist(pts[u], pts[v]));
            }
        }
    }
    total
}

/// Ratsnest segments (minimum spanning tree edges) per net, for crossing counts.
/// One ratsnest line: (net index, from, to).
pub type Line = (usize, (f64, f64), (f64, f64));

pub fn ratsnest(board: &Board, positions: &HashMap<usize, (f64, f64)>) -> Vec<Line> {
    let mut out = Vec::new();
    for (ni, net) in board.nets().into_iter().enumerate() {
        if !board.poured_layers(&net).is_empty() || is_ground(&net) {
            continue;
        }
        let pts: Vec<(f64, f64)> = board
            .pads
            .iter()
            .enumerate()
            .filter(|(_, p)| p.net.as_deref() == Some(net.as_str()))
            .map(|(i, p)| positions.get(&i).copied().unwrap_or(p.at))
            .collect();
        if pts.len() < 2 {
            continue;
        }
        let mut in_tree = vec![false; pts.len()];
        let mut best = vec![(f64::INFINITY, 0usize); pts.len()];
        best[0].0 = 0.0;
        for step in 0..pts.len() {
            let mut u = usize::MAX;
            for i in 0..pts.len() {
                if !in_tree[i] && (u == usize::MAX || best[i].0 < best[u].0) {
                    u = i;
                }
            }
            in_tree[u] = true;
            if step > 0 {
                out.push((ni, pts[best[u].1], pts[u]));
            }
            for v in 0..pts.len() {
                if !in_tree[v] {
                    let d = dist(pts[u], pts[v]);
                    if d < best[v].0 {
                        best[v] = (d, u);
                    }
                }
            }
        }
    }
    out
}

/// Number of places where ratsnest lines of different nets cross (ground
/// excluded, since it is poured).
pub fn crossings(board: &Board, positions: &HashMap<usize, (f64, f64)>) -> usize {
    let lines = ratsnest(board, positions);
    let cross = |o: (f64, f64), p: (f64, f64), q: (f64, f64)| {
        (p.0 - o.0) * (q.1 - o.1) - (p.1 - o.1) * (q.0 - o.0)
    };
    let mut n = 0;
    for i in 0..lines.len() {
        for j in i + 1..lines.len() {
            let (na, a, b) = lines[i];
            let (nb, c, d) = lines[j];
            if na == nb {
                continue;
            }
            let (d1, d2, d3, d4) = (
                cross(c, d, a),
                cross(c, d, b),
                cross(a, b, c),
                cross(a, b, d),
            );
            if d1 * d2 < -1e-12 && d3 * d4 < -1e-12 {
                n += 1;
            }
        }
    }
    n
}

/// Improves `placed` in place. `parts` lists every footprint the optimizer may
/// see; footprints on the board but not in `parts` are fixed obstacles.
pub fn refine(
    board: &Board,
    placed: &mut BTreeMap<String, ((f64, f64), f64)>,
    parts: Vec<Part>,
    opts: &RefineOptions,
) -> RefineStats {
    let fp: Vec<usize> = parts
        .iter()
        .map(|p| {
            board
                .footprints
                .iter()
                .position(|f| f.reference == p.reference)
                .expect("part is a footprint")
        })
        .collect();
    let pose =
        |r: &str, f: &super::model::Footprint| placed.get(r).copied().unwrap_or((f.at, f.rotation));
    let at: Vec<(f64, f64)> = parts
        .iter()
        .zip(&fp)
        .map(|(p, &i)| pose(&p.reference, &board.footprints[i]).0)
        .collect();
    let rot: Vec<f64> = parts
        .iter()
        .zip(&fp)
        .map(|(p, &i)| pose(&p.reference, &board.footprints[i]).1)
        .collect();
    let boxes: Vec<Rect> = fp
        .iter()
        .enumerate()
        .map(|(k, &i)| board.footprints[i].courtyard_at(at[k], rot[k]))
        .collect();
    let origin = boxes
        .iter()
        .map(|c| ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0))
        .collect();
    let back = fp.iter().map(|&i| board.footprints[i].back).collect();
    let mut pad_part = HashMap::new();
    for (k, &i) in fp.iter().enumerate() {
        for &p in &board.footprints[i].pads {
            pad_part.insert(p, k);
        }
    }
    let outline = board.outline.unwrap_or_default();
    let static_obstacles = board
        .footprints
        .iter()
        .filter(|f| !parts.iter().any(|p| p.reference == f.reference))
        .map(|f| {
            let (a, r) = pose(&f.reference, f);
            (
                f.courtyard_at(a, r),
                f.back,
                f.pads.iter().any(|&i| board.pads[i].through_hole),
            )
        })
        .filter(|(c, _, _)| rects_overlap(*c, outline, 0.0))
        .collect();
    let mut nets = Vec::new();
    let mut part_nets = vec![Vec::new(); parts.len()];
    for net in board.nets() {
        if !board.poured_layers(&net).is_empty() {
            continue;
        }
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
        let id = nets.len();
        for p in &pads {
            if let Some(&k) = pad_part.get(p) {
                if !part_nets[k].contains(&id) {
                    part_nets[k].push(id);
                }
            }
        }
        nets.push(pads);
    }
    let mut m = Model {
        board,
        parts,
        fp,
        at,
        rot,
        origin,
        boxes,
        back,
        nets,
        pad_part,
        part_nets,
        net_cost: Vec::new(),
        static_obstacles,
        opts,
    };
    m.net_cost = (0..m.nets.len()).map(|n| m.mst_of(n)).collect();
    let before = m.total();
    let movable: Vec<usize> = (0..m.parts.len())
        .filter(|&k| m.parts[k].freedom != Freedom::Fixed)
        .collect();
    let mut stats = RefineStats {
        cost_before: before,
        cost_after: before,
        ..Default::default()
    };
    if movable.is_empty() || m.nets.is_empty() {
        return stats;
    }
    let iterations = if opts.iterations > 0 {
        opts.iterations
    } else if opts.anneal {
        (movable.len() * 2_000).clamp(20_000, 300_000)
    } else {
        (movable.len() * 200).clamp(2_000, 40_000)
    };
    let mut rng = Rng(opts.seed | 1);
    let span = (outline.2 - outline.0).max(outline.3 - outline.1);
    let groups: Vec<Vec<usize>> = {
        let mut by: BTreeMap<(String, bool), Vec<usize>> = BTreeMap::new();
        for &k in &movable {
            if m.parts[k].freedom == Freedom::Free {
                by.entry((board.footprints[m.fp[k]].library.clone(), m.back[k]))
                    .or_default()
                    .push(k);
            }
        }
        by.into_values().filter(|v| v.len() > 1).collect()
    };
    let mover = Mover {
        movable: &movable,
        groups: &groups,
        grid: opts.grid.max(0.05),
    };

    // Temperature from the moves themselves: the average uphill step of a
    // sample of wide moves, so roughly a third of uphill moves pass at first.
    let t0 = if opts.anneal {
        let mut ups = Vec::new();
        for _ in 0..400 {
            if let Some((ks, poses)) = mover.propose(&mut m, &mut rng, span * 0.25) {
                let saved = m.save(&ks);
                let (d, old) = m.delta(&ks, &poses);
                m.restore(&ks, &saved, old);
                if d > 1e-6 {
                    ups.push(d);
                }
            }
        }
        opts.heat.max(1e-3)
            * if ups.is_empty() {
                1.0
            } else {
                ups.iter().sum::<f64>() / ups.len() as f64
            }
    } else {
        0.0
    };
    let t_end = (t0 * 1e-3).max(1e-3);
    let polish_from = if opts.anneal { iterations * 4 / 5 } else { 0 };

    let mut current = before;
    let mut best_cost = before;
    let mut best_pose: (Vec<(f64, f64)>, Vec<f64>) = (m.at.clone(), m.rot.clone());
    for it in 0..iterations {
        if opts.anneal && it == polish_from {
            m.set_pose(&best_pose.0, &best_pose.1);
            current = best_cost;
        }
        let annealing = opts.anneal && it < polish_from;
        let temp = if annealing {
            t0 * (t_end / t0).powf(it as f64 / polish_from as f64)
        } else {
            0.0
        };
        let reach = if annealing {
            (span * 0.5 * (temp / t0).sqrt()).max(mover.grid * 2.0)
        } else {
            (opts.max_shift.min(span * 0.1)).max(mover.grid * 2.0) * (0.15 + 0.85 * rng.unit())
        };
        let Some((ks, poses)) = mover.propose(&mut m, &mut rng, reach) else {
            continue;
        };
        let saved = m.save(&ks);
        let (d, old) = m.delta(&ks, &poses);
        let accept = d < -1e-6 || (temp > 0.0 && rng.unit() < (-d / temp).exp());
        if accept {
            current += d;
            stats.accepted += 1;
            if current < best_cost - 1e-9 {
                best_cost = current;
                best_pose = (m.at.clone(), m.rot.clone());
            }
        } else {
            m.restore(&ks, &saved, old);
        }
    }
    m.at = best_pose.0;
    m.rot = best_pose.1;
    for (k, part) in m.parts.iter().enumerate() {
        if part.freedom == Freedom::Fixed {
            continue;
        }
        placed.insert(
            part.reference.clone(),
            (m.at[k], m.rot[k].rem_euclid(360.0)),
        );
    }
    stats.cost_after = best_cost;
    stats.iterations = iterations;
    stats
}

type Pose = ((f64, f64), f64, Rect);

impl Model<'_> {
    fn save(&self, ks: &[usize]) -> Vec<Pose> {
        ks.iter()
            .map(|&k| (self.at[k], self.rot[k], self.boxes[k]))
            .collect()
    }

    fn restore(&mut self, ks: &[usize], saved: &[Pose], old: Vec<(usize, f64)>) {
        for (i, &k) in ks.iter().enumerate() {
            self.at[k] = saved[i].0;
            self.rot[k] = saved[i].1;
            self.boxes[k] = saved[i].2;
        }
        for (n, c) in old {
            self.net_cost[n] = c;
        }
    }

    fn set_pose(&mut self, at: &[(f64, f64)], rot: &[f64]) {
        self.at = at.to_vec();
        self.rot = rot.to_vec();
        for k in 0..self.at.len() {
            self.boxes[k] = self.board.footprints[self.fp[k]].courtyard_at(self.at[k], self.rot[k]);
        }
        self.net_cost = (0..self.nets.len()).map(|n| self.mst_of(n)).collect();
    }
}

struct Mover<'a> {
    movable: &'a [usize],
    groups: &'a [Vec<usize>],
    grid: f64,
}

impl Mover<'_> {
    fn snap(&self, v: f64) -> f64 {
        (v / self.grid).round() * self.grid
    }

    /// A random legal move: swap two identical parts, turn a part, or shift it
    /// (connectors on an edge only slide along it).
    fn propose(
        &self,
        m: &mut Model<'_>,
        rng: &mut Rng,
        reach: f64,
    ) -> Option<(Vec<usize>, Vec<Pose>)> {
        let board = m.board;
        let roll = rng.unit();
        if roll < 0.12 && !self.groups.is_empty() {
            let grp = &self.groups[rng.below(self.groups.len())];
            let a = grp[rng.below(grp.len())];
            let b = grp[rng.below(grp.len())];
            if a == b {
                return None;
            }
            let (pa, ra, pb, rb) = (m.at[a], m.rot[a], m.at[b], m.rot[b]);
            let fa = &board.footprints[m.fp[a]];
            let fb = &board.footprints[m.fp[b]];
            let centre = |c: Rect| ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0);
            let na = anchor(fa, centre(fb.courtyard_at(pb, rb)), rb);
            let nb = anchor(fb, centre(fa.courtyard_at(pa, ra)), ra);
            let (sa, sb) = (m.boxes[a], m.boxes[b]);
            let gone = (f64::MAX, f64::MAX, f64::MAX, f64::MAX);
            m.boxes[a] = gone;
            m.boxes[b] = gone;
            let la = m.legal(a, na, rb);
            let lb = m.legal(b, nb, ra);
            m.boxes[a] = sa;
            m.boxes[b] = sb;
            let (ba, bb) = (la?, lb?);
            if rects_overlap(ba, bb, m.opts.gap) {
                return None;
            }
            return Some((vec![a, b], vec![(na, rb, ba), (nb, ra, bb)]));
        }
        let k = self.movable[rng.below(self.movable.len())];
        let f = &board.footprints[m.fp[k]];
        let mut rot = m.rot[k];
        let c = m.boxes[k];
        let mut centre = ((c.0 + c.2) / 2.0, (c.1 + c.3) / 2.0);
        let slide = matches!(m.parts[k].freedom, Freedom::Slide(_));
        match m.parts[k].freedom {
            Freedom::Free => {
                if m.opts.rotate && roll < 0.27 {
                    rot = (rot + [90.0, 180.0, 270.0][rng.below(3)]).rem_euclid(360.0);
                } else {
                    centre.0 += (rng.unit() * 2.0 - 1.0) * reach;
                    centre.1 += (rng.unit() * 2.0 - 1.0) * reach;
                }
            }
            Freedom::Slide(along_x) => {
                let d = self.snap((rng.unit() * 2.0 - 1.0) * reach);
                if along_x {
                    centre.0 += d;
                } else {
                    centre.1 += d;
                }
            }
            Freedom::Fixed => return None,
        }
        let a = anchor(f, centre, rot);
        let a = if slide {
            a
        } else {
            (self.snap(a.0), self.snap(a.1))
        };
        let bx = m.legal(k, a, rot)?;
        Some((vec![k], vec![(a, rot, bx)]))
    }
}

fn anchor(f: &super::model::Footprint, centre: (f64, f64), rot: f64) -> (f64, f64) {
    let c = f.courtyard_at((0.0, 0.0), rot);
    (centre.0 - (c.0 + c.2) / 2.0, centre.1 - (c.1 + c.3) / 2.0)
}
