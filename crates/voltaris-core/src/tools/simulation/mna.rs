//! Linear modified-nodal-analysis circuit solver: DC operating point, AC
//! small-signal sweep and trapezoidal transient for R, C, L, independent V/I
//! sources and controlled sources E (VCVS) and G (VCCS).

use std::collections::BTreeMap;
use std::f64::consts::PI;
use std::ops::{Add, Div, Mul, Neg, Sub};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cx {
    pub re: f64,
    pub im: f64,
}

impl Cx {
    pub const ZERO: Cx = Cx { re: 0.0, im: 0.0 };
    pub fn new(re: f64, im: f64) -> Self {
        Cx { re, im }
    }
    pub fn real(re: f64) -> Self {
        Cx { re, im: 0.0 }
    }
    pub fn abs(self) -> f64 {
        self.re.hypot(self.im)
    }
    pub fn arg_deg(self) -> f64 {
        self.im.atan2(self.re).to_degrees()
    }
}
impl Add for Cx {
    type Output = Cx;
    fn add(self, o: Cx) -> Cx {
        Cx::new(self.re + o.re, self.im + o.im)
    }
}
impl Sub for Cx {
    type Output = Cx;
    fn sub(self, o: Cx) -> Cx {
        Cx::new(self.re - o.re, self.im - o.im)
    }
}
impl Mul for Cx {
    type Output = Cx;
    fn mul(self, o: Cx) -> Cx {
        Cx::new(
            self.re * o.re - self.im * o.im,
            self.re * o.im + self.im * o.re,
        )
    }
}
impl Div for Cx {
    type Output = Cx;
    fn div(self, o: Cx) -> Cx {
        let d = o.re * o.re + o.im * o.im;
        Cx::new(
            (self.re * o.re + self.im * o.im) / d,
            (self.im * o.re - self.re * o.im) / d,
        )
    }
}
impl Neg for Cx {
    type Output = Cx;
    fn neg(self) -> Cx {
        Cx::new(-self.re, -self.im)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Wave {
    Dc,
    Pulse {
        v1: f64,
        v2: f64,
        td: f64,
        tr: f64,
        tf: f64,
        pw: f64,
        per: f64,
    },
    Sin {
        vo: f64,
        va: f64,
        freq: f64,
        td: f64,
        theta: f64,
    },
    Pwl(Vec<(f64, f64)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub dc: f64,
    pub ac_mag: f64,
    pub ac_phase_deg: f64,
    pub wave: Wave,
}

impl Source {
    pub fn at(&self, t: f64) -> f64 {
        match &self.wave {
            Wave::Dc => self.dc,
            Wave::Pulse {
                v1,
                v2,
                td,
                tr,
                tf,
                pw,
                per,
            } => {
                if t < *td {
                    return *v1;
                }
                let mut tt = t - td;
                if *per > 0.0 {
                    tt %= per;
                }
                let tr = tr.max(1e-15);
                let tf = tf.max(1e-15);
                if tt < tr {
                    v1 + (v2 - v1) * tt / tr
                } else if tt < tr + pw {
                    *v2
                } else if tt < tr + pw + tf {
                    v2 + (v1 - v2) * (tt - tr - pw) / tf
                } else {
                    *v1
                }
            }
            Wave::Sin {
                vo,
                va,
                freq,
                td,
                theta,
            } => {
                if t < *td {
                    *vo
                } else {
                    vo + va * (-(t - td) * theta).exp() * (2.0 * PI * freq * (t - td)).sin()
                }
            }
            Wave::Pwl(points) => {
                if points.is_empty() {
                    return 0.0;
                }
                if t <= points[0].0 {
                    return points[0].1;
                }
                for w in points.windows(2) {
                    let ((t0, v0), (t1, v1)) = (w[0], w[1]);
                    if t <= t1 {
                        return if t1 > t0 {
                            v0 + (v1 - v0) * (t - t0) / (t1 - t0)
                        } else {
                            v1
                        };
                    }
                }
                points.last().unwrap().1
            }
        }
    }
    pub fn t0(&self) -> f64 {
        self.at(0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    R(f64),
    C { c: f64, ic: Option<f64> },
    L { l: f64, ic: Option<f64> },
    V(Source),
    I(Source),
    E { gain: f64, cp: usize, cn: usize },
    G { gm: f64, cp: usize, cn: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Element {
    pub name: String,
    pub p: usize,
    pub n: usize,
    pub kind: Kind,
}

/// Node 0 is ground.
#[derive(Clone, Debug, Default)]
pub struct Circuit {
    pub nodes: Vec<String>,
    pub elements: Vec<Element>,
}

impl Circuit {
    pub fn node(&mut self, name: &str) -> usize {
        let key = name.to_ascii_lowercase();
        if key == "0" || key == "gnd" {
            return 0;
        }
        if self.nodes.is_empty() {
            self.nodes.push("0".into());
        }
        if let Some(i) = self.nodes.iter().position(|n| n.eq_ignore_ascii_case(name)) {
            return i;
        }
        self.nodes.push(name.to_string());
        self.nodes.len() - 1
    }
    fn n_nodes(&self) -> usize {
        self.nodes.len().max(1)
    }
    fn has_branch(k: &Kind) -> bool {
        matches!(k, Kind::V(_) | Kind::L { .. } | Kind::E { .. })
    }
    fn branch_index(&self) -> BTreeMap<usize, usize> {
        let base = self.n_nodes() - 1;
        let mut m = BTreeMap::new();
        let mut k = 0;
        for (i, e) in self.elements.iter().enumerate() {
            if Self::has_branch(&e.kind) {
                m.insert(i, base + k);
                k += 1;
            }
        }
        m
    }
    fn size(&self) -> usize {
        self.n_nodes() - 1
            + self
                .elements
                .iter()
                .filter(|e| Self::has_branch(&e.kind))
                .count()
    }
}

pub struct Solution {
    pub x: Vec<Cx>,
}

#[derive(Debug)]
pub struct SolveError(pub String);

fn solve_linear(
    mut a: Vec<Vec<Cx>>,
    mut b: Vec<Cx>,
    names: &[String],
) -> Result<Vec<Cx>, SolveError> {
    let n = b.len();
    let scale = a
        .iter()
        .flatten()
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max)
        .max(1e-30);
    for col in 0..n {
        let (piv, mag) = (col..n)
            .map(|r| (r, a[r][col].abs()))
            .fold((col, -1.0), |m, x| if x.1 > m.1 { x } else { m });
        if mag <= scale * 1e-15 {
            return Err(SolveError(format!(
                "singular circuit at unknown '{}': a node is floating (no DC path to ground) or a loop of voltage sources/inductors is shorted",
                names.get(col).cloned().unwrap_or_default()
            )));
        }
        a.swap(col, piv);
        b.swap(col, piv);
        let p = a[col][col];
        for r in col + 1..n {
            let f = a[r][col] / p;
            if f == Cx::ZERO {
                continue;
            }
            let (top, bottom) = a.split_at_mut(r);
            let pivot_row = &top[col];
            for (dst, v) in bottom[0][col..n].iter_mut().zip(&pivot_row[col..n]) {
                *dst = *dst - f * *v;
            }
            let bv = b[col];
            b[r] = b[r] - f * bv;
        }
    }
    let mut x = vec![Cx::ZERO; n];
    for r in (0..n).rev() {
        let mut s = b[r];
        for c in r + 1..n {
            s = s - a[r][c] * x[c];
        }
        x[r] = s / a[r][r];
    }
    Ok(x)
}

/// How each energy-storage element is represented in one linear solve.
#[derive(Clone, Copy)]
enum Mode {
    Dc,
    Ac(f64),
    Tran { h: f64 },
}

impl Circuit {
    pub fn unknown_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self
            .nodes
            .iter()
            .skip(1)
            .map(|n| format!("V({n})"))
            .collect();
        for e in &self.elements {
            if Self::has_branch(&e.kind) {
                v.push(format!("I({})", e.name));
            }
        }
        v
    }

    /// `state` holds, per element index, (v_prev, i_prev) for C and L in transient mode.
    fn assemble(&self, mode: Mode, t: f64, state: &[(f64, f64)]) -> (Vec<Vec<Cx>>, Vec<Cx>) {
        let n = self.size();
        let mut a = vec![vec![Cx::ZERO; n]; n];
        let mut b = vec![Cx::ZERO; n];
        let idx = |node: usize| node.checked_sub(1);
        let br = self.branch_index();
        let stamp_g = |a: &mut Vec<Vec<Cx>>, p: usize, q: usize, g: Cx| {
            if let Some(i) = idx(p) {
                a[i][i] = a[i][i] + g;
            }
            if let Some(j) = idx(q) {
                a[j][j] = a[j][j] + g;
            }
            if let (Some(i), Some(j)) = (idx(p), idx(q)) {
                a[i][j] = a[i][j] - g;
                a[j][i] = a[j][i] - g;
            }
        };
        let inject = |b: &mut Vec<Cx>, p: usize, q: usize, i_val: Cx| {
            if let Some(i) = idx(p) {
                b[i] = b[i] - i_val;
            }
            if let Some(j) = idx(q) {
                b[j] = b[j] + i_val;
            }
        };
        // Tiny conductance to ground keeps capacitor-only nodes solvable in DC.
        for (i, row) in a.iter_mut().enumerate().take(self.n_nodes() - 1) {
            row[i] = row[i] + Cx::real(1e-12);
        }
        for (ei, e) in self.elements.iter().enumerate() {
            match &e.kind {
                Kind::R(r) => stamp_g(&mut a, e.p, e.n, Cx::real(1.0 / r)),
                Kind::C { c, .. } => match mode {
                    Mode::Dc => {}
                    Mode::Ac(w) => stamp_g(&mut a, e.p, e.n, Cx::new(0.0, w * c)),
                    Mode::Tran { h } => {
                        let g = 2.0 * c / h;
                        let (vp, ip) = state[ei];
                        stamp_g(&mut a, e.p, e.n, Cx::real(g));
                        inject(&mut b, e.p, e.n, Cx::real(-(g * vp + ip)));
                    }
                },
                Kind::G { gm, cp, cn } => {
                    for (row, sgn) in [(e.p, 1.0), (e.n, -1.0)] {
                        if let Some(r) = idx(row) {
                            if let Some(c) = idx(*cp) {
                                a[r][c] = a[r][c] + Cx::real(sgn * gm);
                            }
                            if let Some(c) = idx(*cn) {
                                a[r][c] = a[r][c] - Cx::real(sgn * gm);
                            }
                        }
                    }
                }
                Kind::I(src) => {
                    let val = match mode {
                        Mode::Dc => Cx::real(src.t0()),
                        Mode::Ac(_) => {
                            let ph = src.ac_phase_deg.to_radians();
                            Cx::new(src.ac_mag * ph.cos(), src.ac_mag * ph.sin())
                        }
                        Mode::Tran { .. } => Cx::real(src.at(t)),
                    };
                    inject(&mut b, e.p, e.n, val);
                }
                Kind::V(_) | Kind::L { .. } | Kind::E { .. } => {
                    let k = br[&ei];
                    if let Some(i) = idx(e.p) {
                        a[i][k] = a[i][k] + Cx::real(1.0);
                        a[k][i] = a[k][i] + Cx::real(1.0);
                    }
                    if let Some(j) = idx(e.n) {
                        a[j][k] = a[j][k] - Cx::real(1.0);
                        a[k][j] = a[k][j] - Cx::real(1.0);
                    }
                    match &e.kind {
                        Kind::V(src) => {
                            b[k] = match mode {
                                Mode::Dc => Cx::real(src.t0()),
                                Mode::Ac(_) => {
                                    let ph = src.ac_phase_deg.to_radians();
                                    Cx::new(src.ac_mag * ph.cos(), src.ac_mag * ph.sin())
                                }
                                Mode::Tran { .. } => Cx::real(src.at(t)),
                            }
                        }
                        Kind::L { l, .. } => match mode {
                            Mode::Dc => {}
                            Mode::Ac(w) => a[k][k] = a[k][k] - Cx::new(0.0, w * l),
                            Mode::Tran { h } => {
                                let r = 2.0 * l / h;
                                let (vp, ip) = state[ei];
                                a[k][k] = a[k][k] - Cx::real(r);
                                b[k] = Cx::real(-r * ip - vp);
                            }
                        },
                        Kind::E { gain, cp, cn } => {
                            if let Some(c) = idx(*cp) {
                                a[k][c] = a[k][c] - Cx::real(*gain);
                            }
                            if let Some(c) = idx(*cn) {
                                a[k][c] = a[k][c] + Cx::real(*gain);
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
        (a, b)
    }

    /// Nodes with no path to ground. With `dc_only`, capacitors and current
    /// sources do not count as a path (the node's DC level is then undefined).
    pub fn floating_nodes(&self, dc_only: bool) -> Vec<String> {
        let n = self.n_nodes();
        let mut parent: Vec<usize> = (0..n).collect();
        fn find(p: &mut Vec<usize>, x: usize) -> usize {
            if p[x] != x {
                let r = find(p, p[x]);
                p[x] = r;
            }
            p[x]
        }
        for e in &self.elements {
            let conducts = match e.kind {
                Kind::C { .. } | Kind::I(_) | Kind::G { .. } => !dc_only,
                _ => true,
            };
            if conducts {
                let (a, b) = (find(&mut parent, e.p), find(&mut parent, e.n));
                parent[a] = b;
            }
        }
        let g = find(&mut parent, 0);
        (1..n)
            .filter(|&i| find(&mut parent, i) != g)
            .map(|i| self.nodes[i].clone())
            .collect()
    }

    pub fn node_v(&self, x: &[Cx], node: usize) -> Cx {
        if node == 0 {
            Cx::ZERO
        } else {
            x[node - 1]
        }
    }

    pub fn dc(&self) -> Result<Solution, SolveError> {
        let floating = self.floating_nodes(false);
        if !floating.is_empty() {
            return Err(SolveError(format!(
                "floating node(s) with no connection to ground: {}",
                floating.join(", ")
            )));
        }
        let (a, b) = self.assemble(Mode::Dc, 0.0, &[]);
        Ok(Solution {
            x: solve_linear(a, b, &self.unknown_names())?,
        })
    }

    pub fn ac(&self, freq: f64) -> Result<Solution, SolveError> {
        let (a, b) = self.assemble(Mode::Ac(2.0 * PI * freq), 0.0, &[]);
        Ok(Solution {
            x: solve_linear(a, b, &self.unknown_names())?,
        })
    }

    /// Trapezoidal transient with a fixed step. Returns (times, node-voltage rows).
    pub fn tran(
        &self,
        tstop: f64,
        h: f64,
        uic: bool,
        ic_nodes: &BTreeMap<usize, f64>,
    ) -> Result<(Vec<f64>, Vec<Vec<f64>>), SolveError> {
        let br = self.branch_index();
        let mut state = vec![(0.0, 0.0); self.elements.len()];
        let x0: Vec<f64> = if uic {
            let mut v = vec![0.0; self.size()];
            for (node, val) in ic_nodes {
                if *node > 0 {
                    v[node - 1] = *val;
                }
            }
            v
        } else {
            self.dc()?.x.iter().map(|c| c.re).collect()
        };
        let nv = |x: &[f64], node: usize| if node == 0 { 0.0 } else { x[node - 1] };
        for (ei, e) in self.elements.iter().enumerate() {
            match &e.kind {
                Kind::C { ic, .. } => {
                    let v = if uic {
                        ic.unwrap_or(nv(&x0, e.p) - nv(&x0, e.n))
                    } else {
                        nv(&x0, e.p) - nv(&x0, e.n)
                    };
                    state[ei] = (v, 0.0);
                }
                Kind::L { ic, .. } => {
                    let i = if uic { ic.unwrap_or(0.0) } else { x0[br[&ei]] };
                    state[ei] = (nv(&x0, e.p) - nv(&x0, e.n), i);
                }
                _ => {}
            }
        }
        let steps = (tstop / h).ceil() as usize;
        let mut times = Vec::with_capacity(steps + 1);
        let mut rows = Vec::with_capacity(steps + 1);
        times.push(0.0);
        rows.push((1..self.n_nodes()).map(|n| nv(&x0, n)).collect());
        let names = self.unknown_names();
        for s in 1..=steps {
            let t = (s as f64 * h).min(tstop);
            let (a, b) = self.assemble(Mode::Tran { h }, t, &state);
            let x: Vec<f64> = solve_linear(a, b, &names)?.iter().map(|c| c.re).collect();
            for (ei, e) in self.elements.iter().enumerate() {
                let v = nv(&x, e.p) - nv(&x, e.n);
                match &e.kind {
                    Kind::C { c, .. } => {
                        let (vp, ip) = state[ei];
                        let i = 2.0 * c / h * (v - vp) - ip;
                        state[ei] = (v, i);
                    }
                    Kind::L { .. } => state[ei] = (v, x[br[&ei]]),
                    _ => {}
                }
            }
            times.push(t);
            rows.push((1..self.n_nodes()).map(|n| nv(&x, n)).collect());
        }
        Ok((times, rows))
    }

    pub fn branch_current(&self, x: &[Cx], element: usize) -> Option<Cx> {
        self.branch_index().get(&element).map(|k| x[*k])
    }
}
