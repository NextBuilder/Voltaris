//! Grid A* router: routes every unrouted connection on the board in priority
//! order (power, fast, signal, ground), on any number of copper layers with
//! vias, keeps KiCad's clearance from foreign copper, prefers octilinear paths
//! with few bends, straightens them afterwards, and retries failed nets first
//! on the next pass.

use super::model::{dist, is_fast, is_ground, segment_distance, Board, PadShape};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BinaryHeap, HashMap};

#[derive(Debug, Clone)]
pub struct RouteOptions {
    pub layers: Vec<String>,
    pub grid: f64,
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
}

impl Default for RouteOptions {
    fn default() -> Self {
        Self {
            layers: vec!["F.Cu".into(), "B.Cu".into()],
            grid: 0.2,
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
pub struct RouteResult {
    pub nets: Vec<NetResult>,
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
    let pitch = opts.grid.clamp(0.05, 1.0);
    let grid = Grid {
        x0: outline.0,
        y0: outline.1,
        pitch,
        nx: ((outline.2 - outline.0) / pitch).ceil() as usize + 1,
        ny: ((outline.3 - outline.1) / pitch).ceil() as usize + 1,
        nl: layers.len(),
    };
    if grid.nx * grid.ny * grid.nl > 12_000_000 {
        anyhow::bail!("the routing grid would exceed 12 million cells; raise 'grid'");
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

    let mut plans = Vec::new();
    let mut warnings = Vec::new();
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
        let power = super::super::design_model::is_power_net(name) || is_ground(name);
        let width = opts
            .net_widths
            .get(name)
            .copied()
            .or_else(|| {
                opts.net_widths
                    .iter()
                    .find(|(k, _)| super::model::short_net(name) == k.as_str())
                    .map(|(_, v)| *v)
            })
            .unwrap_or(if power {
                opts.power_width
            } else {
                opts.signal_width
            });
        let tier = if is_ground(name) && !poured.is_empty() {
            0
        } else if power && !is_ground(name) {
            1
        } else if is_fast(name) {
            2
        } else if is_ground(name) {
            4
        } else {
            3
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
        if is_fast(name) {
            warnings.push(format!(
                "{name} looks like a fast signal; route matched pairs with route_differential_pair and check skew with analyze_net_lengths"
            ));
        }
        plans.push(NetPlan {
            name: name.clone(),
            id,
            width,
            tier,
            span,
            components,
            terminal_points,
        });
    }
    plans.sort_by(|a, b| {
        a.tier
            .cmp(&b.tier)
            .then(a.span.partial_cmp(&b.span).unwrap_or(Ordering::Equal))
            .then(a.name.cmp(&b.name))
    });

    let mut order: Vec<usize> = (0..plans.len()).collect();
    let mut best: Option<RouteResult> = None;
    for pass in 0..opts.passes.max(1) {
        let mut obstacles = base.clone();
        let mut results: Vec<NetResult> = Vec::new();
        let mut failed = Vec::new();
        for &pi in &order {
            let plan = &plans[pi];
            let r = route_net(board, plan, &grid, &layers, &obstacles, opts);
            for s in &r.segments {
                obstacles.push(Obstacle {
                    net: Some(plan.id),
                    layers: layer_bit(&s.layer),
                    shape: Shape::Capsule(s.a, s.b, s.width / 2.0),
                });
            }
            for v in &r.vias {
                obstacles.push(Obstacle {
                    net: Some(plan.id),
                    layers: all_layers,
                    shape: Shape::Circle(*v, opts.via_size / 2.0),
                });
            }
            if r.connections_routed < r.connections_total {
                failed.push(pi);
            }
            results.push(r);
        }
        let candidate = RouteResult {
            nets: results,
            pass_used: pass + 1,
            warnings: Vec::new(),
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
    best.warnings = warnings;
    Ok(best)
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
    board: &Board,
    plan: &NetPlan,
    grid: &Grid,
    layers: &[String],
    obstacles: &[Obstacle],
    opts: &RouteOptions,
) -> NetResult {
    let total = plan.components.len() - 1;
    let mut result = NetResult {
        net: plan.name.clone(),
        status: "failed",
        connections_total: total,
        ..Default::default()
    };
    let half = plan.width / 2.0;
    let track_keep = opts.clearance + half;
    let margin = grid.pitch * 0.25;
    let n = grid.nx * grid.ny * grid.nl;
    let mut blocked = vec![false; n];
    let mut via_blocked = vec![false; grid.nx * grid.ny];
    let via_keep = opts.clearance + opts.via_size / 2.0;

    let edge = opts.edge_clearance + half;
    let outline = board.outline.unwrap_or_default();
    for y in 0..grid.ny {
        for x in 0..grid.nx {
            let (cx, cy) = grid.center(x, y);
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
        let foreign = o.net != Some(plan.id);
        let bbox = o.shape.bbox();
        if foreign {
            let reach = track_keep.max(via_keep) + margin;
            let (x0, y0, x1, y1) = grid.cells_near(bbox, reach);
            for y in y0..=y1 {
                for x in x0..=x1 {
                    let d = o.shape.distance(grid.center(x, y));
                    if d < track_keep + margin {
                        for l in 0..grid.nl {
                            if o.layers & (1 << l) != 0 {
                                blocked[grid.idx(l, x, y)] = true;
                            }
                        }
                    }
                    if d < via_keep + margin {
                        via_blocked[y * grid.nx + x] = true;
                    }
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

    let terminal_cells = |pads: &[usize]| -> Vec<usize> {
        let mut cells = Vec::new();
        for &pi in pads {
            let p = &board.pads[pi];
            let (x0, y0, x1, y1) = grid.cells_near(p.bbox(), 0.0);
            for (l, name) in layers.iter().enumerate() {
                if !p.on_layer(name) && !p.through_hole {
                    continue;
                }
                let mut found = false;
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let c = grid.center(x, y);
                        if p.distance_to(c) <= 1e-9 {
                            cells.push(grid.idx(l, x, y));
                            found = true;
                        }
                    }
                }
                if !found {
                    let gx = (((p.at.0 - grid.x0) / grid.pitch).round().max(0.0) as usize)
                        .min(grid.nx - 1);
                    let gy = (((p.at.1 - grid.y0) / grid.pitch).round().max(0.0) as usize)
                        .min(grid.ny - 1);
                    cells.push(grid.idx(l, gx, gy));
                }
            }
        }
        cells
    };

    let mut connected = vec![false; plan.components.len()];
    connected[0] = true;
    let mut tree: Vec<usize> = terminal_cells(&plan.components[0]);
    let cell_owner: Vec<Vec<usize>> = plan.components.iter().map(|c| terminal_cells(c)).collect();
    for cells in &cell_owner {
        for &c in cells {
            blocked[c] = false;
        }
    }

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
        let (segs, vias) = emit(grid, layers, &path, plan.width, &blocked);
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
    result
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
) -> Option<(Vec<usize>, usize)> {
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
    width: f64,
    blocked: &[bool],
) -> (Vec<RoutedSegment>, Vec<(f64, f64)>) {
    let mut segments = Vec::new();
    let mut vias = Vec::new();
    let mut run: Vec<(usize, usize)> = Vec::new();
    let mut run_layer = None;
    let flush = |run: &mut Vec<(usize, usize)>, layer: usize, segments: &mut Vec<RoutedSegment>| {
        if run.len() >= 2 {
            let pts = straighten(grid, layer, run, blocked);
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
mod corpus {
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

    fn violations(b: &Board, r: &RouteResult, clearance: f64, via_size: f64) -> Vec<String> {
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
        let opts = RouteOptions {
            grid: 0.25,
            ..Default::default()
        };
        let (done, total, vias, len, ms) = reroute("pic_programmer.kicad_pcb", &opts);
        eprintln!("pic_programmer: {done}/{total} connections, {vias} vias, {len:.0} mm, {ms} ms");
        assert!(done as f64 >= total as f64 * 0.8, "{done}/{total}");
    }
}
