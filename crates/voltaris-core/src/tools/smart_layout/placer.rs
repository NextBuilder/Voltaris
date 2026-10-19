//! Placement planner: connectors flush on an edge, main ICs centred, every
//! decoupling capacitor beside the power pin it serves, everything else next to
//! the part it talks to most, snapped to a grid and aligned into rows. Then the
//! optimizer (refine.rs) shortens the ratsnest: gently around a person's
//! layout in `Improve` mode, with full annealing in `Replace` mode.

use super::model::{dist, is_ground, is_unnamed, rects_overlap, Board, Footprint, Rect};
use crate::tools::design_model::is_power_net;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use voltaris_sexp::geometry::transform_pad;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceMode {
    /// Place only what is missing, off the board or overlapping; leave the rest.
    Respect,
    /// Keep the layout's structure and shorten its connections: parts move at
    /// most `max_shift`, turn and swap with identical parts; connectors stay.
    Improve,
    /// Re-place every movable part from scratch and anneal the result.
    Replace,
}

#[derive(Debug, Clone)]
pub struct PlaceOptions {
    pub grid: f64,
    pub gap: f64,
    pub edge_margin: f64,
    pub keep: BTreeSet<String>,
    pub rotate: bool,
    pub only: Option<BTreeSet<String>>,
    pub mode: PlaceMode,
    pub max_shift: f64,
    pub iterations: usize,
}

impl Default for PlaceOptions {
    fn default() -> Self {
        Self {
            grid: 0.25,
            gap: 0.4,
            edge_margin: 0.5,
            keep: BTreeSet::new(),
            rotate: true,
            only: None,
            mode: PlaceMode::Improve,
            max_shift: 5.0,
            iterations: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Move {
    pub reference: String,
    pub from: (f64, f64, f64),
    pub to: (f64, f64, f64),
    pub role: &'static str,
    pub reason: String,
}

#[derive(Debug, Clone, Default)]
pub struct PlacePlan {
    pub moves: Vec<Move>,
    pub kept: Vec<(String, &'static str)>,
    pub unplaced: Vec<(String, String)>,
    pub airwire_before: f64,
    pub airwire_after: f64,
    pub crossings_before: usize,
    pub crossings_after: usize,
    pub refine: Option<super::refine::RefineStats>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Role {
    Fixed,
    Connector,
    Main,
    Decoupling,
    Part,
}

impl Role {
    fn name(self) -> &'static str {
        match self {
            Role::Fixed => "fixed",
            Role::Connector => "connector",
            Role::Main => "main_ic",
            Role::Decoupling => "decoupling",
            Role::Part => "support",
        }
    }
}

struct State<'a> {
    board: &'a Board,
    opts: &'a PlaceOptions,
    region: Rect,
    outline: Rect,
    placed: BTreeMap<String, ((f64, f64), f64)>,
    occupied: Vec<(String, Rect, bool)>,
}

impl State<'_> {
    fn fp(&self, r: &str) -> &Footprint {
        self.board.footprint(r).expect("footprint exists")
    }

    fn fits(&self, fp: &Footprint, at: (f64, f64), rot: f64, inside: Rect) -> bool {
        let c = fp.courtyard_at(at, rot);
        if c.0 < inside.0 - 1e-6
            || c.1 < inside.1 - 1e-6
            || c.2 > inside.2 + 1e-6
            || c.3 > inside.3 + 1e-6
        {
            return false;
        }
        if self.board.keepout_blocks(c, fp.back) {
            return false;
        }
        !self.occupied.iter().any(|(r, o, back)| {
            r != &fp.reference && *back == fp.back && rects_overlap(c, *o, self.opts.gap)
        })
    }

    fn commit(&mut self, fp: &Footprint, at: (f64, f64), rot: f64) {
        self.occupied.retain(|(r, _, _)| r != &fp.reference);
        self.occupied
            .push((fp.reference.clone(), fp.courtyard_at(at, rot), fp.back));
        self.placed.insert(fp.reference.clone(), (at, rot));
    }

    fn snap(&self, v: f64) -> f64 {
        (v / self.opts.grid).round() * self.opts.grid
    }

    fn search(
        &self,
        fp: &Footprint,
        target: (f64, f64),
        rotations: &[f64],
        inside: Rect,
    ) -> Option<((f64, f64), f64)> {
        let g = self.opts.grid;
        let max_ring = (((inside.2 - inside.0).max(inside.3 - inside.1)) / g).ceil() as i32 + 2;
        for ring in 0..max_ring {
            let mut best: Option<(f64, (f64, f64), f64)> = None;
            for dx in -ring..=ring {
                for dy in -ring..=ring {
                    if dx.abs() != ring && dy.abs() != ring {
                        continue;
                    }
                    let at = (
                        self.snap(target.0) + dx as f64 * g,
                        self.snap(target.1) + dy as f64 * g,
                    );
                    for &rot in rotations {
                        let anchor = anchor_for(fp, at, rot);
                        if self.fits(fp, anchor, rot, inside) {
                            let d = dist(at, target);
                            if best.is_none_or(|b| d < b.0) {
                                best = Some((d, anchor, rot));
                            }
                        }
                    }
                }
            }
            if let Some((_, a, r)) = best {
                return Some((a, r));
            }
        }
        None
    }

    fn pad_position(&self, pad: usize, at: (f64, f64), rot: f64) -> (f64, f64) {
        let p = &self.board.pads[pad];
        transform_pad(p.local.0, p.local.1, at.0, at.1, rot)
    }

    fn current_pad(&self, pad: usize) -> Option<(f64, f64)> {
        let p = &self.board.pads[pad];
        self.placed
            .get(&p.reference)
            .map(|&(at, rot)| self.pad_position(pad, at, rot))
    }
}

fn anchor_for(fp: &Footprint, center: (f64, f64), rot: f64) -> (f64, f64) {
    let c = fp.courtyard_at((0.0, 0.0), rot);
    (center.0 - (c.0 + c.2) / 2.0, center.1 - (c.1 + c.3) / 2.0)
}

fn center_of(r: Rect) -> (f64, f64) {
    ((r.0 + r.2) / 2.0, (r.1 + r.3) / 2.0)
}

fn role(board: &Board, fp: &Footprint, opts: &PlaceOptions, ic_power: &BTreeSet<String>) -> Role {
    if fp.locked || fp.is_mechanical() || opts.keep.contains(&fp.reference) {
        return Role::Fixed;
    }
    if fp.is_connector() {
        return Role::Connector;
    }
    let p = fp.prefix();
    if fp.pads.len() >= 6 || matches!(p.as_str(), "U" | "IC" | "MOD" | "M" | "A") {
        return Role::Main;
    }
    if p == "C" && fp.pads.len() == 2 {
        let nets: Vec<&str> = fp
            .pads
            .iter()
            .filter_map(|&i| board.pads[i].net.as_deref())
            .collect();
        if nets.len() == 2
            && nets.iter().any(|n| is_ground(n))
            && nets.iter().any(|n| !is_ground(n) && ic_power.contains(*n))
        {
            return Role::Decoupling;
        }
    }
    Role::Part
}

pub fn airwire_length(board: &Board, positions: &HashMap<usize, (f64, f64)>) -> f64 {
    let mut total = 0.0;
    for net in board.nets() {
        if !board.poured_layers(&net).is_empty() {
            continue;
        }
        let pts: Vec<(f64, f64)> = board
            .pads
            .iter()
            .enumerate()
            .filter(|(_, p)| p.net.as_deref() == Some(net.as_str()))
            .map(|(i, p)| positions.get(&i).copied().unwrap_or(p.at))
            .collect();
        total += mst(&pts);
    }
    total
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

pub fn plan(board: &Board, opts: &PlaceOptions) -> anyhow::Result<PlacePlan> {
    let mut pinned: BTreeSet<String> = BTreeSet::new();
    let mut last = None;
    for _ in 0..6 {
        let p = plan_once(board, opts, &pinned)?;
        let moved: BTreeMap<&str, Rect> = p
            .moves
            .iter()
            .filter_map(|m| {
                board.footprint(&m.reference).map(|f| {
                    (
                        m.reference.as_str(),
                        f.courtyard_at((m.to.0, m.to.1), m.to.2),
                    )
                })
            })
            .collect();
        let outline = board.outline.unwrap_or_default();
        let mut conflicts = Vec::new();
        for f in &board.footprints {
            if moved.contains_key(f.reference.as_str()) || pinned.contains(&f.reference) {
                continue;
            }
            let c = f.courtyard;
            if !rects_overlap(c, outline, 0.0) {
                continue;
            }
            let hit = p.moves.iter().any(|m| {
                let other = board.footprint(&m.reference).map(|o| o.back) == Some(f.back);
                other && rects_overlap(c, moved[m.reference.as_str()], 0.0)
            });
            if hit {
                conflicts.push(f.reference.clone());
            }
        }
        if conflicts.is_empty() {
            return Ok(p);
        }
        pinned.extend(conflicts);
        last = Some(p);
    }
    Ok(last.unwrap_or_default())
}

fn plan_once(
    board: &Board,
    opts: &PlaceOptions,
    pinned: &BTreeSet<String>,
) -> anyhow::Result<PlacePlan> {
    let outline = board
        .outline
        .ok_or_else(|| anyhow::anyhow!("the board has no Edge.Cuts outline to place inside"))?;
    let region = (
        outline.0 + opts.edge_margin,
        outline.1 + opts.edge_margin,
        outline.2 - opts.edge_margin,
        outline.3 - opts.edge_margin,
    );
    let ic_power: BTreeSet<String> = board
        .footprints
        .iter()
        .filter(|f| f.pads.len() >= 6 || matches!(f.prefix().as_str(), "U" | "IC" | "MOD" | "M"))
        .flat_map(|f| f.pads.iter().filter_map(|&i| board.pads[i].net.clone()))
        .filter(|n| is_power_net(n) && !is_ground(n))
        .collect();
    let respect_existing = opts.mode != PlaceMode::Replace;
    let mut roles: BTreeMap<String, Role> = board
        .footprints
        .iter()
        .map(|f| {
            let mut r = role(board, f, opts, &ic_power);
            if pinned.contains(&f.reference) {
                r = Role::Fixed;
            }
            if let Some(only) = &opts.only {
                if !only.contains(&f.reference) {
                    r = Role::Fixed;
                }
            }
            (f.reference.clone(), r)
        })
        .collect();

    let mut st = State {
        board,
        opts,
        region,
        outline,
        placed: BTreeMap::new(),
        occupied: Vec::new(),
    };
    let base_roles = roles.clone();
    let mut out = PlacePlan::default();
    for f in &board.footprints {
        if roles[&f.reference] == Role::Fixed {
            st.commit(f, f.at, f.rotation);
            out.kept.push((f.reference.clone(), "fixed"));
        }
    }
    if respect_existing {
        let refs: Vec<String> = board
            .footprints
            .iter()
            .filter(|f| {
                roles[&f.reference] != Role::Fixed && rects_overlap(f.courtyard, outline, 0.0)
            })
            .map(|f| f.reference.clone())
            .collect();
        for r in refs {
            let f = board.footprint(&r).expect("footprint exists");
            let keep = match roles[&r] {
                Role::Connector => true,
                Role::Decoupling => f.pads.iter().any(|&i| {
                    let p = &board.pads[i];
                    p.net.as_deref().is_some_and(|n| !is_ground(n))
                        && board.pads.iter().any(|q| {
                            q.net == p.net
                                && q.reference != r
                                && board
                                    .footprint(&q.reference)
                                    .is_some_and(|o| o.pads.len() >= 6 && !o.is_connector())
                                && dist(q.at, p.at) <= 3.0
                        })
                }),
                _ => true,
            };
            if keep {
                roles.insert(r.clone(), Role::Fixed);
                st.commit(f, f.at, f.rotation);
                out.kept.push((r, "already placed"));
            }
        }
    }
    let rots_all: Vec<f64> = if opts.rotate {
        vec![0.0, 90.0, 180.0, 270.0]
    } else {
        vec![]
    };
    let inside_board = |f: &Footprint| -> bool {
        let c = f.courtyard;
        c.0 >= outline.0 - 1e-6
            && c.1 >= outline.1 - 1e-6
            && c.2 <= outline.2 + 1e-6
            && c.3 <= outline.3 + 1e-6
    };

    let mut connectors: Vec<&Footprint> = board
        .footprints
        .iter()
        .filter(|f| roles[&f.reference] == Role::Connector)
        .collect();
    connectors.sort_by_key(|f| std::cmp::Reverse(f.pads.len()));
    for (k, f) in connectors.iter().enumerate() {
        let c = f.courtyard;
        let (cx, cy) = center_of(c);
        let touching = rects_overlap(c, outline, 0.0)
            && [
                c.0 - outline.0,
                outline.2 - c.2,
                c.1 - outline.1,
                outline.3 - c.3,
            ]
            .iter()
            .any(|d| *d <= 1.0);
        let free_here = !st
            .occupied
            .iter()
            .any(|(r, o, back)| r != &f.reference && *back == f.back && rects_overlap(c, *o, 0.0));
        if touching && free_here {
            st.commit(f, f.at, f.rotation);
            out.kept
                .push((f.reference.clone(), "connector already on an edge"));
            continue;
        }
        let powered = f
            .pads
            .iter()
            .filter_map(|&i| board.pads[i].net.as_deref())
            .all(|n| is_power_net(n) || is_ground(n));
        let edge = if inside_board(f) {
            let d = [
                (cx - outline.0, 0),
                (outline.2 - cx, 1),
                (cy - outline.1, 2),
                (outline.3 - cy, 3),
            ];
            d.iter()
                .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
                .unwrap()
                .1
        } else if powered {
            0
        } else {
            [1, 2, 3, 0][k % 4]
        };
        let mut placed = None;
        let rot = f.rotation;
        let local = f.courtyard_at((0.0, 0.0), rot);
        let (w, h) = (local.2 - local.0, local.3 - local.1);
        let span = match edge {
            0 | 1 => outline.3 - outline.1,
            _ => outline.2 - outline.0,
        };
        let mid = match edge {
            0 | 1 => (outline.1 + outline.3) / 2.0,
            _ => (outline.0 + outline.2) / 2.0,
        };
        let steps = (span / opts.grid) as i32;
        'scan: for s in 0..steps {
            let off = if s % 2 == 0 { s / 2 } else { -(s + 1) / 2 } as f64 * opts.grid;
            let center = match edge {
                0 => (outline.0 + w / 2.0, mid + off),
                1 => (outline.2 - w / 2.0, mid + off),
                2 => (mid + off, outline.1 + h / 2.0),
                _ => (mid + off, outline.3 - h / 2.0),
            };
            let anchor = anchor_for(f, center, rot);
            if st.fits(f, anchor, rot, outline) {
                placed = Some(anchor);
                break 'scan;
            }
        }
        match placed {
            Some(a) => {
                st.commit(f, a, rot);
                out.moves.push(Move {
                    reference: f.reference.clone(),
                    from: (f.at.0, f.at.1, f.rotation),
                    to: (a.0, a.1, rot),
                    role: Role::Connector.name(),
                    reason: format!(
                        "flush on the {} edge",
                        ["left", "right", "top", "bottom"][edge]
                    ),
                });
            }
            None => out
                .unplaced
                .push((f.reference.clone(), "no room on any board edge".into())),
        }
    }

    let mut mains: Vec<&Footprint> = board
        .footprints
        .iter()
        .filter(|f| roles[&f.reference] == Role::Main)
        .collect();
    mains.sort_by_key(|f| std::cmp::Reverse(f.pads.len()));
    let centre = center_of(region);
    for (k, f) in mains.iter().enumerate() {
        if inside_board(f) && st.fits(f, f.at, f.rotation, region) {
            st.commit(f, f.at, f.rotation);
            out.kept
                .push((f.reference.clone(), "main part already placed on the board"));
            continue;
        }
        let w = region.2 - region.0;
        let offset = if k == 0 {
            0.0
        } else if k % 2 == 1 {
            w * 0.25 * k.div_ceil(2) as f64
        } else {
            -w * 0.25 * (k / 2) as f64
        };
        let target = (centre.0 + offset, centre.1);
        match st.search(f, target, &[f.rotation], region) {
            Some((a, r)) => {
                st.commit(f, a, r);
                out.moves.push(Move {
                    reference: f.reference.clone(),
                    from: (f.at.0, f.at.1, f.rotation),
                    to: (a.0, a.1, r),
                    role: Role::Main.name(),
                    reason: "main part in the middle of the board".into(),
                });
            }
            None => out
                .unplaced
                .push((f.reference.clone(), "no free area large enough".into())),
        }
    }

    let decoupling: Vec<&Footprint> = board
        .footprints
        .iter()
        .filter(|f| roles[&f.reference] == Role::Decoupling)
        .collect();
    let mut used_pins: BTreeMap<(String, String), usize> = BTreeMap::new();
    for f in decoupling {
        let power_pad = f
            .pads
            .iter()
            .copied()
            .find(|&i| board.pads[i].net.as_deref().is_some_and(|n| !is_ground(n)));
        let Some(cap_pad) = power_pad else { continue };
        let net = board.pads[cap_pad].net.clone().unwrap_or_default();
        let mut candidates: Vec<(usize, (f64, f64))> = board
            .pads
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                p.net.as_deref() == Some(net.as_str())
                    && roles.get(&p.reference) == Some(&Role::Main)
            })
            .filter_map(|(i, _)| st.current_pad(i).map(|at| (i, at)))
            .collect();
        candidates.sort_by_key(|(i, _)| {
            used_pins
                .get(&(
                    board.pads[*i].reference.clone(),
                    board.pads[*i].number.clone(),
                ))
                .copied()
                .unwrap_or(0)
        });
        let Some(&(ic_pad, pin_at)) = candidates.first() else {
            out.unplaced
                .push((f.reference.clone(), format!("no placed IC pin on {net}")));
            continue;
        };
        let ic_ref = board.pads[ic_pad].reference.clone();
        *used_pins
            .entry((ic_ref.clone(), board.pads[ic_pad].number.clone()))
            .or_default() += 1;
        let ic = st.fp(&ic_ref);
        let (at, rot) = st.placed[&ic_ref];
        let body = ic.courtyard_at(at, rot);
        let sides = [
            (pin_at.0 - body.0, 0),
            (body.2 - pin_at.0, 1),
            (pin_at.1 - body.1, 2),
            (body.3 - pin_at.1, 3),
        ];
        let side = sides
            .iter()
            .min_by(|a, b| a.0.partial_cmp(&b.0).unwrap())
            .unwrap()
            .1;
        let local0 = f.courtyard_at((0.0, 0.0), 0.0);
        let long_x = (local0.2 - local0.0) >= (local0.3 - local0.1);
        let rotations: Vec<f64> = match (side, long_x) {
            (0 | 1, true) | (2 | 3, false) => vec![90.0, 270.0],
            _ => vec![0.0, 180.0],
        };
        let mut best: Option<(f64, (f64, f64), f64)> = None;
        for &r in &rotations {
            let c = f.courtyard_at((0.0, 0.0), r);
            let (hw, hh) = ((c.2 - c.0) / 2.0, (c.3 - c.1) / 2.0);
            let center = match side {
                0 => (body.0 - opts.gap - hw, pin_at.1),
                1 => (body.2 + opts.gap + hw, pin_at.1),
                2 => (pin_at.0, body.1 - opts.gap - hh),
                _ => (pin_at.0, body.3 + opts.gap + hh),
            };
            if let Some((a, rr)) = st.search(f, center, &[r], region) {
                let pp = st.pad_position(cap_pad, a, rr);
                let d = dist(pp, pin_at);
                if best.is_none_or(|b| d < b.0) {
                    best = Some((d, a, rr));
                }
            }
        }
        if inside_board(f) && st.fits(f, f.at, f.rotation, region) {
            let current = dist(st.pad_position(cap_pad, f.at, f.rotation), pin_at);
            if current <= 3.0 || best.is_none_or(|b| current <= b.0 + 0.3) {
                st.commit(f, f.at, f.rotation);
                out.kept
                    .push((f.reference.clone(), "decoupling already beside its pin"));
                continue;
            }
        }
        match best {
            Some((d, a, r)) => {
                st.commit(f, a, r);
                out.moves.push(Move {
                    reference: f.reference.clone(),
                    from: (f.at.0, f.at.1, f.rotation),
                    to: (a.0, a.1, r),
                    role: Role::Decoupling.name(),
                    reason: format!(
                        "beside {ic_ref} pin {} ({net}), {:.1} mm from the pin",
                        board.pads[ic_pad].number, d
                    ),
                });
            }
            None => out
                .unplaced
                .push((f.reference.clone(), "no room beside its IC".into())),
        }
    }

    let mut parts: Vec<&Footprint> = board
        .footprints
        .iter()
        .filter(|f| roles[&f.reference] == Role::Part)
        .collect();
    parts.sort_by_key(|f| std::cmp::Reverse(f.pads.len()));
    for f in parts {
        let mut weights: Vec<((f64, f64), f64)> = Vec::new();
        for &pi in &f.pads {
            let Some(net) = board.pads[pi].net.as_deref() else {
                continue;
            };
            let w = if is_ground(net) {
                0.0
            } else if is_power_net(net) {
                0.3
            } else if is_unnamed(net) {
                1.5
            } else {
                1.0
            };
            if w == 0.0 {
                continue;
            }
            for (qi, q) in board.pads.iter().enumerate() {
                if q.reference != f.reference && q.net.as_deref() == Some(net) {
                    if let Some(at) = st.current_pad(qi) {
                        weights.push((at, w));
                    }
                }
            }
        }
        let target = if weights.is_empty() {
            centre
        } else {
            let tw: f64 = weights.iter().map(|w| w.1).sum();
            (
                weights.iter().map(|(p, w)| p.0 * w).sum::<f64>() / tw,
                weights.iter().map(|(p, w)| p.1 * w).sum::<f64>() / tw,
            )
        };
        let rotations = if rots_all.is_empty() {
            vec![f.rotation]
        } else {
            rots_all.clone()
        };
        let cost_at = |st: &State<'_>, a: (f64, f64), rr: f64| -> f64 {
            let mut cost = dist(center_of(f.courtyard_at(a, rr)), target) * 0.2;
            for &pi in &f.pads {
                let Some(net) = board.pads[pi].net.as_deref() else {
                    continue;
                };
                if is_ground(net) {
                    continue;
                }
                let pp = st.pad_position(pi, a, rr);
                let nearest = board
                    .pads
                    .iter()
                    .enumerate()
                    .filter(|(_, q)| q.reference != f.reference && q.net.as_deref() == Some(net))
                    .filter_map(|(qi, _)| st.current_pad(qi))
                    .map(|q| dist(q, pp))
                    .fold(f64::INFINITY, f64::min);
                if nearest.is_finite() {
                    cost += nearest;
                }
            }
            cost
        };
        let mut best: Option<(f64, (f64, f64), f64)> = None;
        for &r in &rotations {
            if let Some((a, rr)) = st.search(f, target, &[r], region) {
                let cost = cost_at(&st, a, rr);
                if best.is_none_or(|b| cost < b.0 - 1e-9) {
                    best = Some((cost, a, rr));
                }
            }
        }
        if inside_board(f) && st.fits(f, f.at, f.rotation, region) {
            let current = cost_at(&st, f.at, f.rotation);
            if respect_existing || best.is_none_or(|b| current <= b.0 + 0.5) {
                st.commit(f, f.at, f.rotation);
                out.kept.push((f.reference.clone(), "already well placed"));
                continue;
            }
        }
        match best {
            Some((_, a, r)) => {
                st.commit(f, a, r);
                out.moves.push(Move {
                    reference: f.reference.clone(),
                    from: (f.at.0, f.at.1, f.rotation),
                    to: (a.0, a.1, r),
                    role: Role::Part.name(),
                    reason: "beside the parts it connects to".into(),
                });
            }
            None => out
                .unplaced
                .push((f.reference.clone(), "no free spot".into())),
        }
    }

    if opts.mode != PlaceMode::Respect {
        let constructive: Poses = st.placed.clone();
        let refine_opts = super::refine::RefineOptions {
            anneal: opts.mode == PlaceMode::Replace,
            iterations: opts.iterations,
            max_shift: if opts.mode == PlaceMode::Replace {
                f64::INFINITY
            } else {
                opts.max_shift
            },
            grid: opts.grid,
            gap: opts.gap,
            rotate: opts.rotate,
            ..Default::default()
        };
        let mut seeds = vec![constructive.clone()];
        if opts.mode == PlaceMode::Replace {
            // Also anneal from the arrangement already on the board: a fresh
            // start and the existing layout often settle in different valleys.
            let mut existing = constructive.clone();
            let mut on_board = 0;
            for (r, pose) in existing.iter_mut() {
                let f = st.fp(r);
                if rects_overlap(f.courtyard, outline, 0.0) {
                    *pose = (f.at, f.rotation);
                    on_board += 1;
                }
            }
            if on_board * 2 >= existing.len() {
                seeds.push(existing);
            }
        }
        let mut best: Option<(Poses, super::refine::RefineStats)> = None;
        for (i, seed) in seeds.into_iter().enumerate() {
            st.placed = seed;
            let parts = refine_parts(board, &st, &base_roles, opts);
            let mut placed = st.placed.clone();
            let stats = super::refine::refine(
                board,
                &mut placed,
                parts,
                &super::refine::RefineOptions {
                    seed: refine_opts.seed.wrapping_add(i as u64 * 0x2545_F491),
                    heat: if i == 0 { 1.0 } else { 0.05 },
                    ..refine_opts.clone()
                },
            );
            if best
                .as_ref()
                .is_none_or(|(_, b)| stats.cost_after < b.cost_after)
            {
                best = Some((placed, stats));
            }
        }
        let (placed, stats) = best.expect("at least one seed");
        st.placed = placed;
        out.refine = Some(stats);
        let placed: Vec<(String, Pose)> = st.placed.iter().map(|(r, p)| (r.clone(), *p)).collect();
        st.occupied.clear();
        for (r, (at, rot)) in &placed {
            let f = st.fp(r).clone();
            st.commit(&f, *at, *rot);
        }
        for (r, (at, rot)) in placed {
            let f = st.fp(&r);
            let (from_at, from_rot) = (f.at, f.rotation);
            let moved = (at.0 - from_at.0).abs() > 1e-6
                || (at.1 - from_at.1).abs() > 1e-6
                || (rot - from_rot).rem_euclid(360.0).abs() > 1e-6;
            let refined = constructive.get(&r).is_some_and(|c| {
                (c.0 .0 - at.0).abs() > 1e-6
                    || (c.0 .1 - at.1).abs() > 1e-6
                    || (c.1 - rot).abs() > 1e-6
            });
            if !moved {
                continue;
            }
            match out.moves.iter_mut().find(|m| m.reference == r) {
                Some(m) => {
                    m.to = (at.0, at.1, rot);
                    if refined {
                        m.reason.push_str(", then refined for shorter connections");
                    }
                }
                None => {
                    out.kept.retain(|(k, _)| k != &r);
                    out.moves.push(Move {
                        reference: r.clone(),
                        from: (from_at.0, from_at.1, from_rot),
                        to: (at.0, at.1, rot),
                        role: base_roles[&r].name(),
                        reason: "moved, turned or swapped for shorter connections".into(),
                    });
                }
            }
        }
    }

    align_rows(&mut st, &mut out);

    out.moves.retain(|m| {
        (m.from.0 - m.to.0).abs() > 1e-6
            || (m.from.1 - m.to.1).abs() > 1e-6
            || (m.from.2 - m.to.2).rem_euclid(360.0).abs() > 1e-6
    });
    let before = airwire_length(board, &HashMap::new());
    let mut positions = HashMap::new();
    for (i, p) in board.pads.iter().enumerate() {
        if let Some(&(at, rot)) = st.placed.get(&p.reference) {
            positions.insert(i, st.pad_position(i, at, rot));
        }
    }
    out.airwire_before = before;
    out.airwire_after = airwire_length(board, &positions);
    out.crossings_before = super::refine::crossings(board, &HashMap::new());
    out.crossings_after = super::refine::crossings(board, &positions);
    let _ = st.outline;
    Ok(out)
}

type Pose = ((f64, f64), f64);
type Poses = BTreeMap<String, Pose>;

/// What the optimizer may do with each placed part.
fn refine_parts(
    board: &Board,
    st: &State<'_>,
    roles: &BTreeMap<String, Role>,
    opts: &PlaceOptions,
) -> Vec<super::refine::Part> {
    use super::refine::{Freedom, Part};
    let outline = st.outline;
    let region = st.region;
    let mut parts = Vec::new();
    for (r, &(at, rot)) in &st.placed {
        let f = st.fp(r);
        let c = f.courtyard_at(at, rot);
        let within = |o: Rect| {
            c.0 >= o.0 - 1e-6 && c.1 >= o.1 - 1e-6 && c.2 <= o.2 + 1e-6 && c.3 <= o.3 + 1e-6
        };
        let (freedom, inside) = match roles[r] {
            Role::Fixed => (Freedom::Fixed, outline),
            Role::Connector if opts.mode == PlaceMode::Replace => {
                let d = [
                    c.0 - outline.0,
                    outline.2 - c.2,
                    c.1 - outline.1,
                    outline.3 - c.3,
                ];
                let edge = (0..4)
                    .min_by(|&a, &b| d[a].partial_cmp(&d[b]).unwrap_or(std::cmp::Ordering::Equal))
                    .unwrap_or(0);
                if edge < 2 {
                    (
                        Freedom::Slide(false),
                        (f64::MIN / 4.0, outline.1, f64::MAX / 4.0, outline.3),
                    )
                } else {
                    (
                        Freedom::Slide(true),
                        (outline.0, f64::MIN / 4.0, outline.2, f64::MAX / 4.0),
                    )
                }
            }
            Role::Connector => (Freedom::Fixed, outline),
            _ if within(region) => (Freedom::Free, region),
            _ if within(outline) => (Freedom::Free, outline),
            _ => (Freedom::Fixed, outline),
        };
        let decouples = if roles[r] == Role::Decoupling {
            f.pads
                .iter()
                .copied()
                .find(|&i| board.pads[i].net.as_deref().is_some_and(|n| !is_ground(n)))
                .map(|cap_pad| {
                    let net = board.pads[cap_pad].net.clone();
                    let ic: Vec<usize> = board
                        .pads
                        .iter()
                        .enumerate()
                        .filter(|(_, q)| {
                            q.net == net && roles.get(&q.reference) == Some(&Role::Main)
                        })
                        .map(|(i, _)| i)
                        .collect();
                    (cap_pad, ic)
                })
                .filter(|(_, ic)| !ic.is_empty())
        } else {
            None
        };
        parts.push(Part {
            reference: r.clone(),
            freedom,
            inside,
            both_sides: f.pads.iter().any(|&i| board.pads[i].through_hole),
            decouples,
        });
    }
    parts
}

fn align_rows(st: &mut State<'_>, out: &mut PlacePlan) {
    let movable: Vec<String> = out
        .moves
        .iter()
        .filter(|m| m.role == "support" || m.role == "decoupling")
        .map(|m| m.reference.clone())
        .collect();
    for i in 0..movable.len() {
        for j in 0..movable.len() {
            if i == j {
                continue;
            }
            let (a, b) = (&movable[i], &movable[j]);
            let (Some(&(pa, ra)), Some(&(pb, rb))) = (st.placed.get(a), st.placed.get(b)) else {
                continue;
            };
            let ca = center_of(st.fp(a).courtyard_at(pa, ra));
            let cb = center_of(st.fp(b).courtyard_at(pb, rb));
            let dy = ca.1 - cb.1;
            let dx = ca.0 - cb.0;
            let fb = st.fp(b).clone();
            if dy.abs() > 1e-6 && dy.abs() < 0.8 && dx.abs() < 10.0 {
                let nb = (pb.0, pb.1 + dy);
                if st.fits(&fb, nb, rb, st.region) {
                    st.commit(&fb, nb, rb);
                }
            } else if dx.abs() > 1e-6 && dx.abs() < 0.8 && dy.abs() < 10.0 {
                let nb = (pb.0 + dx, pb.1);
                if st.fits(&fb, nb, rb, st.region) {
                    st.commit(&fb, nb, rb);
                }
            }
        }
    }
    for m in out.moves.iter_mut() {
        if let Some(&(at, rot)) = st.placed.get(&m.reference) {
            m.to = (
                super::model::round3(at.0),
                super::model::round3(at.1),
                rot.rem_euclid(360.0),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{fixture, Board};
    use super::*;

    fn ic(reference: &str, x: f64, y: f64) -> String {
        let nets = ["+5V", "SIG", "GND", "Net-(D1-A)", "+5V", "GND"];
        let mut pads = String::new();
        for (i, n) in nets.iter().enumerate() {
            let code = match *n {
                "GND" => 1,
                "+5V" => 2,
                "SIG" => 3,
                _ => 4,
            };
            let (px, py) = if i < 3 {
                (-2.0, -1.27 + i as f64 * 1.27)
            } else {
                (2.0, 1.27 - (i - 3) as f64 * 1.27)
            };
            pads.push_str(&format!(
                "    (pad \"{}\" smd rect (at {px} {py}) (size 1.2 0.6) (layers \"F.Cu\") (net {code} \"{n}\"))\n",
                i + 1
            ));
        }
        format!(
            "  (footprint \"Package_SO:SOIC-6\" (layer \"F.Cu\") (at {x} {y})\n    (property \"Reference\" \"{reference}\" (at 0 -3 0) (layer \"F.SilkS\"))\n    (property \"Value\" \"MCU\" (at 0 3 0) (layer \"F.Fab\"))\n    (fp_rect (start -2.8 -2) (end 2.8 2) (layer \"F.CrtYd\"))\n{pads}  )\n"
        )
    }

    fn staged_board() -> Board {
        let mut body = String::new();
        body.push_str(&ic("U1", 160.0, 160.0));
        body.push_str(&fixture::smd("C1", 170.0, 160.0, 0.0, ("+5V", "GND")));
        body.push_str(&fixture::smd(
            "R1",
            175.0,
            160.0,
            0.0,
            ("SIG", "Net-(D1-A)"),
        ));
        body.push_str(&fixture::header("J1", 180.0, 160.0, &["+5V", "GND"]));
        Board::from_text(&fixture::board(&body)).unwrap()
    }

    #[test]
    fn places_every_part_inside_without_overlap() {
        let b = staged_board();
        let p = plan(&b, &PlaceOptions::default()).unwrap();
        assert!(p.unplaced.is_empty(), "{:?}", p.unplaced);
        assert_eq!(p.moves.len(), 4);
        let outline = b.outline.unwrap();
        let mut boxes = Vec::new();
        for m in &p.moves {
            let f = b.footprint(&m.reference).unwrap();
            let c = f.courtyard_at((m.to.0, m.to.1), m.to.2);
            assert!(
                c.0 >= outline.0 - 1e-6 && c.2 <= outline.2 + 1e-6,
                "{} {:?}",
                m.reference,
                c
            );
            assert!(
                c.1 >= outline.1 - 1e-6 && c.3 <= outline.3 + 1e-6,
                "{} {:?}",
                m.reference,
                c
            );
            boxes.push(c);
        }
        for i in 0..boxes.len() {
            for j in i + 1..boxes.len() {
                assert!(!rects_overlap(boxes[i], boxes[j], 0.0), "{:?}", p.moves);
            }
        }
    }

    #[test]
    fn connector_goes_flush_to_an_edge_and_cap_beside_its_pin() {
        let b = staged_board();
        let p = plan(&b, &PlaceOptions::default()).unwrap();
        let j1 = p.moves.iter().find(|m| m.reference == "J1").unwrap();
        let f = b.footprint("J1").unwrap();
        let c = f.courtyard_at((j1.to.0, j1.to.1), j1.to.2);
        let o = b.outline.unwrap();
        let edge_gap = [c.0 - o.0, o.2 - c.2, c.1 - o.1, o.3 - c.3]
            .into_iter()
            .fold(f64::INFINITY, f64::min);
        assert!(edge_gap < 0.3, "J1 is {edge_gap} mm from the nearest edge");
        let c1 = p.moves.iter().find(|m| m.reference == "C1").unwrap();
        assert_eq!(c1.role, "decoupling");
        let u1 = p.moves.iter().find(|m| m.reference == "U1").unwrap();
        let cap = b.footprint("C1").unwrap();
        let cap_pad = &b.pads[cap.pads[0]];
        let cap_at = transform_pad(cap_pad.local.0, cap_pad.local.1, c1.to.0, c1.to.1, c1.to.2);
        let ic = b.footprint("U1").unwrap();
        let nearest = ic
            .pads
            .iter()
            .filter(|&&i| b.pads[i].net.as_deref() == Some("+5V"))
            .map(|&i| {
                let q = &b.pads[i];
                dist(
                    transform_pad(q.local.0, q.local.1, u1.to.0, u1.to.1, u1.to.2),
                    cap_at,
                )
            })
            .fold(f64::INFINITY, f64::min);
        assert!(nearest < 3.5, "decoupling cap is {nearest} mm from its pin");
        assert!(p.airwire_after < p.airwire_before);
    }
}

#[cfg(test)]
mod corpus {
    use super::super::model::{rects_overlap, Board};
    use super::*;

    fn check(name: &str, mode: PlaceMode) -> PlacePlan {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../voltaris-sexp/tests/fixtures")
            .join(name);
        let b = Board::from_text(&std::fs::read_to_string(path).unwrap()).unwrap();
        let opts = PlaceOptions {
            mode,
            ..Default::default()
        };
        let t = std::time::Instant::now();
        let p = plan(&b, &opts).unwrap();
        let ms = t.elapsed().as_millis();
        let o = b.outline.unwrap();
        let mut boxes: Vec<(String, super::super::model::Rect, bool)> = Vec::new();
        for f in &b.footprints {
            let (at, rot) = p
                .moves
                .iter()
                .find(|m| m.reference == f.reference)
                .map(|m| ((m.to.0, m.to.1), m.to.2))
                .unwrap_or((f.at, f.rotation));
            boxes.push((f.reference.clone(), f.courtyard_at(at, rot), f.back));
        }
        for m in &p.moves {
            let (_, c, _) = boxes.iter().find(|(r, _, _)| r == &m.reference).unwrap();
            let connector = b.footprint(&m.reference).unwrap().is_connector();
            if !connector {
                assert!(
                    c.0 >= o.0 - 1e-6
                        && c.2 <= o.2 + 1e-6
                        && c.1 >= o.1 - 1e-6
                        && c.3 <= o.3 + 1e-6,
                    "{name} {mode:?}: {} outside",
                    m.reference
                );
            }
            for (r, other, back) in &boxes {
                let mine = b.footprint(&m.reference).unwrap().back;
                if r != &m.reference && *back == mine {
                    assert!(
                        !rects_overlap(*c, *other, 0.0),
                        "{name} {mode:?}: {} overlaps {r}",
                        m.reference
                    );
                }
            }
        }
        eprintln!(
            "{name} {mode:?}: {} moves, {} unplaced, connections {:.0} -> {:.0} mm, crossings {} -> {}, {ms} ms, {:?}",
            p.moves.len(),
            p.unplaced.len(),
            p.airwire_before,
            p.airwire_after,
            p.crossings_before,
            p.crossings_after,
            p.refine
        );
        p
    }

    #[test]
    fn respects_a_finished_layout() {
        for name in ["ecc83-pp.kicad_pcb", "pic_programmer.kicad_pcb"] {
            let p = check(name, PlaceMode::Respect);
            assert!(p.moves.is_empty(), "{name}: {} moves", p.moves.len());
        }
    }

    #[test]
    fn improves_a_human_layout_without_rearranging_it() {
        for name in ["ecc83-pp.kicad_pcb", "pic_programmer.kicad_pcb"] {
            let p = check(name, PlaceMode::Improve);
            assert!(
                p.airwire_after < p.airwire_before * 0.97,
                "{name}: {:.0} -> {:.0}",
                p.airwire_before,
                p.airwire_after
            );
        }
    }

    #[test]
    fn full_replacement_beats_the_human_layout() {
        for name in ["ecc83-pp.kicad_pcb", "pic_programmer.kicad_pcb"] {
            let p = check(name, PlaceMode::Replace);
            assert!(
                p.airwire_after < p.airwire_before * 0.9,
                "{name}: {:.0} -> {:.0}",
                p.airwire_before,
                p.airwire_after
            );
        }
    }
}

#[cfg(test)]
mod pipeline {
    use super::super::model::Board;
    use super::super::router::{route, RouteOptions};
    use super::*;

    /// The board with the plan's moves applied.
    fn moved(b: &Board, p: &PlacePlan) -> Board {
        let mut out = b.clone();
        for m in &p.moves {
            let Some(fi) = out
                .footprints
                .iter()
                .position(|f| f.reference == m.reference)
            else {
                continue;
            };
            let old_rot = out.footprints[fi].rotation;
            let f = &mut out.footprints[fi];
            f.courtyard = f.courtyard_at((m.to.0, m.to.1), m.to.2);
            f.at = (m.to.0, m.to.1);
            f.rotation = m.to.2;
            for &pi in &f.pads.clone() {
                let pad = &mut out.pads[pi];
                pad.at = transform_pad(pad.local.0, pad.local.1, m.to.0, m.to.1, m.to.2);
                pad.rotation += m.to.2 - old_rot;
            }
        }
        out.segments.clear();
        out.vias.clear();
        out.zones.clear();
        out
    }

    #[test]
    #[ignore = "places and routes a whole board; run with --ignored for a benchmark"]
    fn optimized_placement_still_routes_completely() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../voltaris-sexp/tests/fixtures/pic_programmer.kicad_pcb");
        let b = Board::from_text(&std::fs::read_to_string(path).unwrap()).unwrap();
        let ropts = RouteOptions::default();
        let human = moved(&b, &PlacePlan::default());
        let base = route(&human, &ropts).unwrap();
        for mode in [PlaceMode::Improve, PlaceMode::Replace] {
            let p = plan(
                &b,
                &PlaceOptions {
                    mode,
                    ..Default::default()
                },
            )
            .unwrap();
            let r = route(&moved(&b, &p), &ropts).unwrap();
            let (done, total) = r.routed_connections();
            let (hd, ht) = base.routed_connections();
            eprintln!(
                "{mode:?}: ratsnest {:.0} -> {:.0} mm; routed {done}/{total} with {} vias, {:.0} mm of track (human placement: {hd}/{ht}, {} vias, {:.0} mm)",
                p.airwire_before,
                p.airwire_after,
                r.vias(),
                r.length(),
                base.vias(),
                base.length()
            );
            assert!(
                done as f64 >= total as f64 * 0.95,
                "{mode:?}: {done}/{total}"
            );
        }
    }
}
