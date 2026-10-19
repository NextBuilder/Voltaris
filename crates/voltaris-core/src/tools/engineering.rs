//! `engineering` toolset — closed-form PCB and circuit calculators.
//!
//! Every calculator is a pure function of its arguments: no board, no KiCad,
//! no network. Each response names the method it used, the validity range of
//! that method, and any input that falls outside it, so a caller never mistakes
//! an extrapolation for a design value.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{invalid_arg, opt_f64, opt_str, require_f64, require_str, ToolDef};
use serde_json::{json, Value};

pub mod calc {
    use std::f64::consts::PI;

    pub const ETA0: f64 = 376.730_313_668;
    pub const C_MM_PER_NS: f64 = 299.792_458;
    pub const RHO_CU: f64 = 1.724e-8;
    pub const ALPHA_CU: f64 = 0.003_93;
    pub const MM_PER_MIL: f64 = 0.0254;
    pub const MM_PER_OZ: f64 = 0.035;

    pub fn ipc2221_k(external: bool) -> f64 {
        if external {
            0.048
        } else {
            0.024
        }
    }

    pub fn ipc2221_area_mil2(current_a: f64, rise_c: f64, external: bool) -> f64 {
        (current_a / (ipc2221_k(external) * rise_c.powf(0.44))).powf(1.0 / 0.725)
    }

    pub fn ipc2221_width_mm(current_a: f64, rise_c: f64, thickness_mm: f64, external: bool) -> f64 {
        let area = ipc2221_area_mil2(current_a, rise_c, external);
        area / (thickness_mm / MM_PER_MIL) * MM_PER_MIL
    }

    pub fn ipc2221_current_a(width_mm: f64, thickness_mm: f64, rise_c: f64, external: bool) -> f64 {
        let area = (width_mm / MM_PER_MIL) * (thickness_mm / MM_PER_MIL);
        ipc2221_k(external) * rise_c.powf(0.44) * area.powf(0.725)
    }

    pub fn copper_resistance_ohm(length_mm: f64, area_mm2: f64, temp_c: f64) -> f64 {
        RHO_CU * (1.0 + ALPHA_CU * (temp_c - 20.0)) * (length_mm * 1e-3) / (area_mm2 * 1e-6)
    }

    pub fn agm(mut a: f64, mut b: f64) -> f64 {
        for _ in 0..64 {
            let an = 0.5 * (a + b);
            let bn = (a * b).sqrt();
            if (an - bn).abs() <= 1e-15 * an {
                return an;
            }
            a = an;
            b = bn;
        }
        0.5 * (a + b)
    }

    pub fn ellip_k(k: f64) -> f64 {
        PI / (2.0 * agm(1.0, (1.0 - k * k).sqrt()))
    }

    pub fn k_ratio(k: f64) -> f64 {
        let kp = (1.0 - k * k).sqrt();
        ellip_k(k) / ellip_k(kp)
    }

    fn hj_z01(u: f64) -> f64 {
        let f = 6.0 + (2.0 * PI - 6.0) * (-(30.666 / u).powf(0.7528)).exp();
        ETA0 / (2.0 * PI) * (f / u + (1.0 + 4.0 / (u * u)).sqrt()).ln()
    }

    fn hj_eeff(u: f64, er: f64) -> f64 {
        let a = 1.0
            + ((u.powi(4) + (u / 52.0).powi(2)) / (u.powi(4) + 0.432)).ln() / 49.0
            + (1.0 + (u / 18.1).powi(3)).ln() / 18.7;
        let b = 0.564 * ((er - 0.9) / (er + 3.0)).powf(0.053);
        (er + 1.0) / 2.0 + (er - 1.0) / 2.0 * (1.0 + 10.0 / u).powf(-a * b)
    }

    /// Hammerstad–Jensen microstrip with Hammerstad's finite-thickness
    /// correction. Returns (Z0, εeff). Accuracy ≤1% for 0.01 ≤ W/H ≤ 100.
    pub fn microstrip(w: f64, h: f64, t: f64, er: f64) -> (f64, f64) {
        let u = w / h;
        let (u1, ur) = if t > 0.0 {
            let tn = t / h;
            let coth = 1.0 / (6.517 * u).sqrt().tanh();
            let du1 = tn / PI * (1.0 + 4.0 * std::f64::consts::E / (tn * coth * coth)).ln();
            let dur = 0.5 * (1.0 + 1.0 / (er - 1.0).sqrt().cosh()) * du1;
            (u + du1, u + dur)
        } else {
            (u, u)
        };
        let e_r = hj_eeff(ur, er);
        let z0 = hj_z01(ur) / e_r.sqrt();
        let eeff = e_r * (hj_z01(u1) / hj_z01(ur)).powi(2);
        (z0, eeff)
    }

    /// Exact zero-thickness centred stripline (Cohn). `b` is plane spacing.
    pub fn stripline_exact_thin(w: f64, b: f64, er: f64) -> f64 {
        let k = 1.0 / (PI * w / (2.0 * b)).cosh();
        ETA0 / (4.0 * er.sqrt()) * k_ratio(k)
    }

    /// Wheeler's finite-thickness centred stripline. `b` is plane spacing.
    /// Accuracy ≈0.5% for W/(b−t) < 10.
    pub fn stripline(w: f64, b: f64, t: f64, er: f64) -> f64 {
        let x = t / b;
        let wp = if x > 0.0 {
            let m = 2.0 / (1.0 + 2.0 / 3.0 * x / (1.0 - x));
            let dw = x / (PI * (1.0 - x))
                * (1.0
                    - 0.5
                        * ((x / (2.0 - x)).powi(2) + (0.0796 * x / (w / b + 1.1 * x)).powf(m))
                            .ln());
            w / (b - t) + dw
        } else {
            w / b
        };
        let q = 8.0 / (PI * wp);
        ETA0 / (4.0 * PI * er.sqrt()) * (1.0 + 4.0 / (PI * wp) * (q + (q * q + 6.27).sqrt())).ln()
    }

    /// Offset stripline: trace `h1` above the lower plane and `h2` below the
    /// upper plane, combined from two centred striplines (IPC-2141A method).
    pub fn stripline_asym(w: f64, h1: f64, h2: f64, t: f64, er: f64) -> f64 {
        let z1 = stripline(w, 2.0 * h1 + t, t, er);
        let z2 = stripline(w, 2.0 * h2 + t, t, er);
        2.0 * z1 * z2 / (z1 + z2)
    }

    /// Grounded coplanar waveguide (conductor-backed CPW), zero thickness.
    /// Returns (Z0, εeff).
    pub fn cpwg(w: f64, s: f64, h: f64, er: f64) -> (f64, f64) {
        let k = w / (w + 2.0 * s);
        let k3 = (PI * w / (4.0 * h)).tanh() / (PI * (w + 2.0 * s) / (4.0 * h)).tanh();
        let a = 1.0 / k_ratio(k);
        let b = k_ratio(k3);
        let eeff = (1.0 + er * a * b) / (1.0 + a * b);
        let z0 = 60.0 * PI / eeff.sqrt() / (k_ratio(k) + k_ratio(k3));
        (z0, eeff)
    }

    pub fn diff_microstrip(z0: f64, s: f64, h: f64) -> f64 {
        2.0 * z0 * (1.0 - 0.48 * (-0.96 * s / h).exp())
    }

    pub fn diff_stripline(z0: f64, s: f64, b: f64) -> f64 {
        2.0 * z0 * (1.0 - 0.347 * (-2.9 * s / b).exp())
    }

    pub fn delay_ps_per_mm(eeff: f64) -> f64 {
        eeff.sqrt() / C_MM_PER_NS * 1000.0
    }

    /// Solve f(w) = target by bisection on a monotone-decreasing f.
    pub fn solve_decreasing(target: f64, lo: f64, hi: f64, f: impl Fn(f64) -> f64) -> Option<f64> {
        let (mut a, mut b) = (lo, hi);
        if f(a) < target || f(b) > target {
            return None;
        }
        for _ in 0..200 {
            let m = 0.5 * (a + b);
            if f(m) > target {
                a = m;
            } else {
                b = m;
            }
            if b - a < 1e-7 {
                break;
            }
        }
        Some(0.5 * (a + b))
    }

    pub fn via_barrel_area_mm2(drill_mm: f64, plating_mm: f64) -> f64 {
        PI * plating_mm * (drill_mm - plating_mm)
    }

    pub fn via_inductance_nh(length_mm: f64, drill_mm: f64) -> f64 {
        0.2 * length_mm * ((4.0 * length_mm / drill_mm).ln() + 1.0)
    }

    pub const E24: [f64; 24] = [
        1.0, 1.1, 1.2, 1.3, 1.5, 1.6, 1.8, 2.0, 2.2, 2.4, 2.7, 3.0, 3.3, 3.6, 3.9, 4.3, 4.7, 5.1,
        5.6, 6.2, 6.8, 7.5, 8.2, 9.1,
    ];

    pub const E96: [f64; 96] = [
        1.00, 1.02, 1.05, 1.07, 1.10, 1.13, 1.15, 1.18, 1.21, 1.24, 1.27, 1.30, 1.33, 1.37, 1.40,
        1.43, 1.47, 1.50, 1.54, 1.58, 1.62, 1.65, 1.69, 1.74, 1.78, 1.82, 1.87, 1.91, 1.96, 2.00,
        2.05, 2.10, 2.15, 2.21, 2.26, 2.32, 2.37, 2.43, 2.49, 2.55, 2.61, 2.67, 2.74, 2.80, 2.87,
        2.94, 3.01, 3.09, 3.16, 3.24, 3.32, 3.40, 3.48, 3.57, 3.65, 3.74, 3.83, 3.92, 4.02, 4.12,
        4.22, 4.32, 4.42, 4.53, 4.64, 4.75, 4.87, 4.99, 5.11, 5.23, 5.36, 5.49, 5.62, 5.76, 5.90,
        6.04, 6.19, 6.34, 6.49, 6.65, 6.81, 6.98, 7.15, 7.32, 7.50, 7.68, 7.87, 8.06, 8.25, 8.45,
        8.66, 8.87, 9.09, 9.31, 9.53, 9.76,
    ];

    pub fn series(name: &str) -> Option<Vec<f64>> {
        match name {
            "E12" => Some(E24.iter().step_by(2).copied().collect()),
            "E24" => Some(E24.to_vec()),
            "E48" => Some(E96.iter().step_by(2).copied().collect()),
            "E96" => Some(E96.to_vec()),
            _ => None,
        }
    }

    pub fn series_values(mantissas: &[f64], lo: f64, hi: f64) -> Vec<f64> {
        let mut out = Vec::new();
        let mut decade = 10f64.powf(lo.log10().floor() - 1.0);
        while decade <= hi * 10.0 {
            for m in mantissas {
                let v = (m * decade * 1e6).round() / 1e6;
                if v >= lo * (1.0 - 1e-9) && v <= hi * (1.0 + 1e-9) {
                    out.push(v);
                }
            }
            decade *= 10.0;
        }
        out.sort_by(|a, b| a.partial_cmp(b).unwrap());
        out.dedup();
        out
    }

    pub fn nearest_at_least(mantissas: &[f64], x: f64) -> f64 {
        series_values(mantissas, x * 0.5, x * 20.0)
            .into_iter()
            .find(|v| *v >= x * (1.0 - 1e-9))
            .unwrap_or(x)
    }

    pub fn nearest_at_most(mantissas: &[f64], x: f64) -> Option<f64> {
        series_values(mantissas, x * 0.05, x * 2.0)
            .into_iter()
            .rev()
            .find(|v| *v <= x * (1.0 + 1e-9))
    }

    pub fn nearest(mantissas: &[f64], x: f64) -> f64 {
        series_values(mantissas, x * 0.3, x * 3.0)
            .into_iter()
            .min_by(|a, b| {
                ((a / x).ln().abs())
                    .partial_cmp(&(b / x).ln().abs())
                    .unwrap()
            })
            .unwrap_or(x)
    }
}

// ─── Stackup presets ────────────────────────────────────────────────────────

#[derive(Clone, Copy)]
pub struct Dielectric {
    pub kind: &'static str,
    pub construction: &'static str,
    pub thickness_mm: f64,
    pub er: f64,
}

pub struct Stackup {
    pub id: &'static str,
    pub description: &'static str,
    pub copper_mm: &'static [f64],
    pub dielectrics: &'static [Dielectric],
}

const PP7628: Dielectric = Dielectric {
    kind: "prepreg",
    construction: "7628",
    thickness_mm: 0.2104,
    er: 4.4,
};
const PP3313: Dielectric = Dielectric {
    kind: "prepreg",
    construction: "3313",
    thickness_mm: 0.0994,
    er: 4.1,
};
const PP2116: Dielectric = Dielectric {
    kind: "prepreg",
    construction: "2116",
    thickness_mm: 0.1164,
    er: 4.16,
};
const PP1080: Dielectric = Dielectric {
    kind: "prepreg",
    construction: "1080",
    thickness_mm: 0.0764,
    er: 3.91,
};
const CORE_1065: Dielectric = Dielectric {
    kind: "core",
    construction: "core",
    thickness_mm: 1.065,
    er: 4.43,
};
const CORE_1265: Dielectric = Dielectric {
    kind: "core",
    construction: "core",
    thickness_mm: 1.265,
    er: 4.43,
};
const CORE_055: Dielectric = Dielectric {
    kind: "core",
    construction: "core",
    thickness_mm: 0.55,
    er: 4.41,
};
const CORE_2L: Dielectric = Dielectric {
    kind: "core",
    construction: "FR-4",
    thickness_mm: 1.53,
    er: 4.5,
};

const CU4: [f64; 4] = [0.035, 0.0152, 0.0152, 0.035];

pub const STACKUP_SOURCE: &str = "JLCPCB impedance-template API, as captured 2025-05-09 by \
    github.com/gsuberland/jlcpcb_autogenerated_stackups (normalised_json). Vendor data changes: \
    confirm the template in JLCPCB's current impedance calculator before ordering.";

pub static STACKUPS: &[Stackup] = &[
    Stackup {
        id: "JLC04161H-7628",
        description: "JLCPCB 4-layer 1.6 mm, 1 oz outer / 0.5 oz inner, 7628 prepreg (JLCPCB default 4L)",
        copper_mm: &CU4,
        dielectrics: &[PP7628, CORE_1065, PP7628],
    },
    Stackup {
        id: "JLC04161H-3313",
        description: "JLCPCB 4-layer 1.6 mm, 1 oz outer / 0.5 oz inner, 3313 prepreg (thin, narrow 50 Ω traces)",
        copper_mm: &CU4,
        dielectrics: &[PP3313, CORE_1265, PP3313],
    },
    Stackup {
        id: "JLC04161H-2116",
        description: "JLCPCB 4-layer 1.6 mm, 1 oz outer / 0.5 oz inner, 2116 prepreg",
        copper_mm: &CU4,
        dielectrics: &[PP2116, CORE_1265, PP2116],
    },
    Stackup {
        id: "JLC04161H-1080",
        description: "JLCPCB 4-layer 1.6 mm, 1 oz outer / 0.5 oz inner, 1080 prepreg (thinnest)",
        copper_mm: &CU4,
        dielectrics: &[PP1080, CORE_1265, PP1080],
    },
    Stackup {
        id: "JLC06161H-NOREQ",
        description: "JLCPCB 6-layer 1.6 mm default ('no requirement'), 1 oz outer / 0.5 oz inner",
        copper_mm: &[0.035, 0.0152, 0.0152, 0.0152, 0.0152, 0.035],
        dielectrics: &[PP3313, CORE_055, PP2116, CORE_055, PP3313],
    },
    Stackup {
        id: "GENERIC-2L-1.6",
        description: "Typical 2-layer 1.6 mm FR-4, 1 oz both sides. Not an impedance-controlled \
                      product: thickness and εr are nominal, so treat results as estimates",
        copper_mm: &[0.035, 0.035],
        dielectrics: &[CORE_2L],
    },
];

pub fn stackup(id: &str) -> Option<&'static Stackup> {
    STACKUPS.iter().find(|s| s.id.eq_ignore_ascii_case(id))
}

fn copper_layer_name(index: usize, count: usize) -> String {
    if index == 0 {
        "F.Cu".into()
    } else if index + 1 == count {
        "B.Cu".into()
    } else {
        format!("In{index}.Cu")
    }
}

fn layer_index(stack: &Stackup, layer: &str) -> Option<usize> {
    let n = stack.copper_mm.len();
    (0..n).find(|i| copper_layer_name(*i, n).eq_ignore_ascii_case(layer))
}

fn stackup_json(stack: &Stackup) -> Value {
    let n = stack.copper_mm.len();
    let mut layers = Vec::new();
    for i in 0..n {
        layers
            .push(json!({ "copper": copper_layer_name(i, n), "thickness_mm": stack.copper_mm[i] }));
        if let Some(d) = stack.dielectrics.get(i) {
            layers.push(json!({
                "dielectric": d.kind, "construction": d.construction,
                "thickness_mm": d.thickness_mm, "er": d.er
            }));
        }
    }
    let total: f64 = stack.copper_mm.iter().sum::<f64>()
        + stack
            .dielectrics
            .iter()
            .map(|d| d.thickness_mm)
            .sum::<f64>();
    json!({
        "id": stack.id,
        "description": stack.description,
        "copper_layers": n,
        "pressed_thickness_mm": (total * 1000.0).round() / 1000.0,
        "layers": layers
    })
}

// ─── Tool definitions ───────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "calc_trace_width",
            "Size a copper trace for a current (IPC-2221 closed form). Returns the minimum width \
             for the allowed temperature rise, plus resistance, voltage drop and power loss when \
             a length is given. IPC-2221 is conservative for internal layers; IPC-2152 charts \
             usually allow narrower internal traces. Also answers the reverse question: pass \
             width_mm instead of current_a to get the safe current.",
            json!({
                "type": "object",
                "properties": {
                    "current_a": { "type": "number", "exclusiveMinimum": 0, "description": "Continuous current (A). Omit when passing width_mm." },
                    "width_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Existing trace width (mm) — returns its current capacity instead" },
                    "temp_rise_c": { "type": "number", "exclusiveMinimum": 0, "default": 10, "description": "Allowed rise above ambient (°C). 10 is the usual design value." },
                    "copper_oz": { "type": "number", "exclusiveMinimum": 0, "default": 1, "description": "Copper weight in oz (1 oz ≈ 35 µm). Ignored when thickness_mm is given." },
                    "thickness_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Finished copper thickness (mm). JLCPCB 4L inner 0.5 oz finishes at 0.0152." },
                    "layer": { "type": "string", "enum": ["external", "internal"], "default": "external" },
                    "length_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Trace length for resistance / drop / loss" },
                    "ambient_c": { "type": "number", "default": 25 }
                }
            }),
            |args, _ctx| async move { handle_trace_width(args) }
        ),
        tool!(
            "calc_impedance",
            "Characteristic impedance of a trace: microstrip (Hammerstad–Jensen with thickness \
             correction, ≤1%), centred or offset stripline (Wheeler / IPC-2141A, ≈0.5–2%), grounded \
             coplanar waveguide (conformal mapping, zero thickness), and edge-coupled differential \
             pairs (IPC-2141 coupling estimate, ±5–10%). Give geometry directly, or a stackup preset \
             from list_stackups plus a copper layer and the tool derives heights, εr and copper. \
             Set target_ohm to solve for the trace width instead. Results exclude solder mask, \
             which lowers outer-layer impedance by roughly 1–3 Ω.",
            json!({
                "type": "object",
                "properties": {
                    "structure": { "type": "string", "enum": ["microstrip", "stripline", "cpwg", "diff_microstrip", "diff_stripline"], "description": "Transmission-line type" },
                    "stackup": { "type": "string", "description": "Preset id from list_stackups (e.g. JLC04161H-7628). Supplies h, t, εr." },
                    "layer": { "type": "string", "description": "Signal layer within the preset: F.Cu, In1.Cu, …, B.Cu" },
                    "width_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Trace width W" },
                    "target_ohm": { "type": "number", "exclusiveMinimum": 0, "description": "Solve for W giving this impedance (single-ended Z0, or Zdiff for diff structures)" },
                    "height_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Dielectric height to the reference plane (microstrip/CPWG), or to the nearer plane (stripline)" },
                    "height2_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Stripline only: dielectric height to the second plane. Omit for a centred stripline." },
                    "thickness_mm": { "type": "number", "minimum": 0, "description": "Trace copper thickness" },
                    "er": { "type": "number", "exclusiveMinimum": 1, "description": "Relative permittivity" },
                    "gap_mm": { "type": "number", "exclusiveMinimum": 0, "description": "CPWG: trace-to-coplanar-ground gap. Diff: edge-to-edge spacing of the pair." }
                },
                "required": ["structure"]
            }),
            |args, _ctx| async move { handle_impedance(args) }
        ),
        tool!(
            "list_stackups",
            "List the stackup presets calc_impedance accepts: JLCPCB 4- and 6-layer impedance \
             templates and a generic 2-layer board, with every copper and dielectric thickness and εr. \
             To use a board's own stackup instead, read it with the board stackup tools and pass \
             height_mm / er / thickness_mm to calc_impedance.",
            json!({ "type": "object", "properties": {} }),
            |_args, _ctx| async move { handle_list_stackups() }
        ),
        tool!(
            "calc_via",
            "Via electrical properties: barrel resistance, inductance (Goldfarb), current capacity \
             (IPC-2221 external constant applied to the plated barrel), and how many vias in \
             parallel carry a given current.",
            json!({
                "type": "object",
                "properties": {
                    "drill_mm": { "type": "number", "exclusiveMinimum": 0, "description": "Finished drill diameter (JLCPCB minimum 0.3 standard, 0.2 advanced — check current capabilities)" },
                    "plating_mm": { "type": "number", "exclusiveMinimum": 0, "default": 0.018, "description": "Barrel plating thickness. JLCPCB stackup data uses 0.018." },
                    "length_mm": { "type": "number", "exclusiveMinimum": 0, "default": 1.6, "description": "Barrel length (board thickness for a through via)" },
                    "temp_rise_c": { "type": "number", "exclusiveMinimum": 0, "default": 10 },
                    "current_a": { "type": "number", "exclusiveMinimum": 0, "description": "Current to carry — returns the number of vias needed" }
                },
                "required": ["drill_mm"]
            }),
            |args, _ctx| async move { handle_via(args) }
        ),
        tool!(
            "calc_regulator_thermal",
            "Linear-regulator (LDO) power dissipation and junction temperature: \
             Pd = (Vin−Vout)·Iout + Vin·Iq, Tj = Ta + Pd·θJA. θJA must come from the part's \
             datasheet for the copper area you will actually provide. Flags dropout violations and \
             returns the maximum safe current at the given θJA.",
            json!({
                "type": "object",
                "properties": {
                    "vin": { "type": "number", "exclusiveMinimum": 0 },
                    "vout": { "type": "number", "exclusiveMinimum": 0 },
                    "iout_a": { "type": "number", "minimum": 0 },
                    "iq_a": { "type": "number", "minimum": 0, "default": 0, "description": "Quiescent/ground current" },
                    "theta_ja_c_per_w": { "type": "number", "exclusiveMinimum": 0, "description": "Junction-to-ambient thermal resistance from the datasheet" },
                    "ambient_c": { "type": "number", "default": 40, "description": "Worst-case ambient, inside the enclosure" },
                    "tj_max_c": { "type": "number", "default": 125 },
                    "dropout_v": { "type": "number", "minimum": 0, "description": "Datasheet dropout at this current (optional)" }
                },
                "required": ["vin", "vout", "iout_a", "theta_ja_c_per_w"]
            }),
            |args, _ctx| async move { handle_regulator(args) }
        ),
        tool!(
            "calc_divider",
            "Pick standard-value resistors for a voltage divider or a regulator feedback network. \
             Searches every pair in the chosen E-series and resistance range and returns the best \
             pairs with their exact output and error. mode 'divider': Vout = Vin·Rbot/(Rtop+Rbot). \
             mode 'feedback': Vout = Vref·(1 + Rtop/Rbot) — pass vref from the regulator datasheet.",
            json!({
                "type": "object",
                "properties": {
                    "mode": { "type": "string", "enum": ["divider", "feedback"] },
                    "vin": { "type": "number", "exclusiveMinimum": 0, "description": "divider mode: input voltage" },
                    "vref": { "type": "number", "exclusiveMinimum": 0, "description": "feedback mode: regulator reference/FB voltage" },
                    "vout": { "type": "number", "exclusiveMinimum": 0, "description": "Target output voltage" },
                    "series": { "type": "string", "enum": ["E12", "E24", "E48", "E96"], "default": "E96" },
                    "r_min_ohm": { "type": "number", "exclusiveMinimum": 0, "default": 1000 },
                    "r_max_ohm": { "type": "number", "exclusiveMinimum": 0, "default": 1000000 },
                    "fixed_rbot_ohm": { "type": "number", "exclusiveMinimum": 0, "description": "Hold Rbot at this value (datasheets often specify it)" }
                },
                "required": ["mode", "vout"]
            }),
            |args, _ctx| async move { handle_divider(args) }
        ),
        tool!(
            "calc_passive",
            "Small circuit calculators. kind 'led': series resistor R = (Vs−Vf)/If rounded up to E24, \
             with actual current and resistor power. 'rc': solve any one of R, C, fc for an RC \
             low/high-pass (fc = 1/2πRC) plus time constant. 'crystal': load capacitors \
             C = 2·(CL − Cstray), rounded to E12. 'i2c': pull-up range from UM10204 — \
             Rmin = (Vdd−VOL)/IOL, Rmax = tr/(0.8473·Cb) — and a recommended value. \
             'battery': runtime from capacity and average current.",
            json!({
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["led", "rc", "crystal", "i2c", "battery"] },
                    "supply_v": { "type": "number", "exclusiveMinimum": 0, "description": "led: drive voltage; i2c: bus voltage" },
                    "vf_v": { "type": "number", "exclusiveMinimum": 0, "description": "led: forward voltage at If (datasheet)" },
                    "if_ma": { "type": "number", "exclusiveMinimum": 0, "description": "led: target current" },
                    "r_ohm": { "type": "number", "exclusiveMinimum": 0 },
                    "c_f": { "type": "number", "exclusiveMinimum": 0, "description": "rc: capacitance in farads (100nF = 1e-7)" },
                    "fc_hz": { "type": "number", "exclusiveMinimum": 0 },
                    "cl_pf": { "type": "number", "exclusiveMinimum": 0, "description": "crystal: specified load capacitance CL" },
                    "cstray_pf": { "type": "number", "minimum": 0, "default": 4, "description": "crystal: PCB + pin stray capacitance (3–5 pF typical)" },
                    "bus_pf": { "type": "number", "exclusiveMinimum": 0, "description": "i2c: total bus capacitance" },
                    "i2c_mode": { "type": "string", "enum": ["standard", "fast", "fast_plus"], "default": "fast" },
                    "capacity_mah": { "type": "number", "exclusiveMinimum": 0 },
                    "avg_current_ma": { "type": "number", "exclusiveMinimum": 0 },
                    "derating": { "type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 0.8, "description": "battery: usable fraction of rated capacity" }
                },
                "required": ["kind"]
            }),
            |args, _ctx| async move { handle_passive(args) }
        ),
        tool!(
            "calc_buck",
            "Size a buck converter's inductor and capacitors from first principles (continuous \
             conduction): L = Vout·(Vin_max−Vout)/(Vin_max·fsw·ΔIL), peak current, Cout for the \
             ripple target, maximum output-capacitor ESR, and worst-case input RMS current. \
             The regulator datasheet's recommended values take precedence — use this to check \
             them or when the datasheet leaves the choice open.",
            json!({
                "type": "object",
                "properties": {
                    "vin_min": { "type": "number", "exclusiveMinimum": 0 },
                    "vin_max": { "type": "number", "exclusiveMinimum": 0 },
                    "vout": { "type": "number", "exclusiveMinimum": 0 },
                    "iout_a": { "type": "number", "exclusiveMinimum": 0 },
                    "fsw_hz": { "type": "number", "exclusiveMinimum": 0, "description": "Switching frequency" },
                    "ripple_ratio": { "type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 0.3, "description": "Inductor ripple ΔIL / Iout (0.2–0.4 typical)" },
                    "vout_ripple_mv": { "type": "number", "exclusiveMinimum": 0, "description": "Allowed output ripple (default 1% of Vout)" },
                    "efficiency": { "type": "number", "exclusiveMinimum": 0, "maximum": 1, "default": 0.9 }
                },
                "required": ["vin_min", "vin_max", "vout", "iout_a", "fsw_hz"]
            }),
            |args, _ctx| async move { handle_buck(args) }
        ),
    ]
}

// ─── Handlers ───────────────────────────────────────────────────────────────

fn r3(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

fn sig(x: f64, digits: i32) -> f64 {
    if x == 0.0 || !x.is_finite() {
        return x;
    }
    let p = digits - 1 - x.abs().log10().floor() as i32;
    let m = 10f64.powi(p);
    (x * m).round() / m
}

fn eng(x: f64, unit: &str) -> String {
    let (scale, prefix) = [
        (1e9, "G"),
        (1e6, "M"),
        (1e3, "k"),
        (1.0, ""),
        (1e-3, "m"),
        (1e-6, "µ"),
        (1e-9, "n"),
        (1e-12, "p"),
    ]
    .into_iter()
    .find(|(s, _)| x.abs() >= *s * 0.999_999)
    .unwrap_or((1e-12, "p"));
    format!("{}{}{}", sig(x / scale, 3), prefix, unit)
}

fn handle_list_stackups() -> anyhow::Result<CallToolResult> {
    Ok(CallToolResult::json(&json!({
        "source": STACKUP_SOURCE,
        "stackups": STACKUPS.iter().map(stackup_json).collect::<Vec<_>>()
    })))
}

fn handle_trace_width(args: &Value) -> anyhow::Result<CallToolResult> {
    let rise = opt_f64(args, "temp_rise_c").unwrap_or(10.0);
    let external = opt_str(args, "layer").unwrap_or("external") == "external";
    let thickness = match opt_f64(args, "thickness_mm") {
        Some(t) => t,
        None => opt_f64(args, "copper_oz").unwrap_or(1.0) * calc::MM_PER_OZ,
    };
    let ambient = opt_f64(args, "ambient_c").unwrap_or(25.0);
    let current = opt_f64(args, "current_a");
    let width_in = opt_f64(args, "width_mm");
    let (width, current) = match (current, width_in) {
        (Some(i), None) => (calc::ipc2221_width_mm(i, rise, thickness, external), i),
        (None, Some(w)) => (w, calc::ipc2221_current_a(w, thickness, rise, external)),
        _ => {
            return Ok(invalid_arg(
                "current_a",
                "pass exactly one of current_a (to size a trace) or width_mm (to rate one)",
            ))
        }
    };
    let mut warnings = Vec::new();
    if current > 35.0 {
        warnings.push("IPC-2221 curves are fitted only up to 35 A".to_string());
    }
    if !(10.0..=100.0).contains(&rise) {
        warnings.push("IPC-2221 curves are fitted for 10–100 °C rise".to_string());
    }
    if width / calc::MM_PER_MIL > 400.0 {
        warnings.push("IPC-2221 curves are fitted only up to 400 mil (10.16 mm) width".to_string());
    }
    let mut out = json!({
        "method": format!("IPC-2221 {} layer: I = {}·ΔT^0.44·A^0.725 (A in mil²)",
            if external { "external" } else { "internal" }, calc::ipc2221_k(external)),
        "current_a": sig(current, 4),
        "temp_rise_c": rise,
        "copper_thickness_mm": thickness,
        "width_mm": r3(width),
        "width_mil": sig(width / calc::MM_PER_MIL, 4),
        "cross_section_mm2": sig(width * thickness, 4),
        "warnings": warnings,
    });
    if width_in.is_none() {
        out["recommended_width_mm"] = json!(((width / 0.05).ceil() * 0.05 * 100.0).round() / 100.0);
    }
    if let Some(len) = opt_f64(args, "length_mm") {
        let r = calc::copper_resistance_ohm(len, width * thickness, ambient + rise);
        out["length_mm"] = json!(len);
        out["resistance_mohm"] = json!(sig(r * 1000.0, 4));
        out["voltage_drop_mv"] = json!(sig(r * current * 1000.0, 4));
        out["power_loss_mw"] = json!(sig(r * current * current * 1000.0, 4));
        out["resistance_basis"] = json!(format!(
            "ρ = 1.724e-8 Ω·m at 20 °C, α = 0.00393/°C, evaluated at {} °C",
            ambient + rise
        ));
    }
    Ok(CallToolResult::json(&out))
}

struct Geometry {
    h: f64,
    h2: Option<f64>,
    t: f64,
    er: f64,
    source: Value,
}

fn geometry_from_args(args: &Value, structure: &str) -> Result<Geometry, CallToolResult> {
    let stripline = structure.contains("stripline");
    if let Some(id) = opt_str(args, "stackup") {
        let Some(stack) = stackup(id) else {
            return Err(invalid_arg(
                "stackup",
                &format!("unknown preset '{id}'; call list_stackups"),
            ));
        };
        let layer = opt_str(args, "layer").unwrap_or("F.Cu");
        let Some(i) = layer_index(stack, layer) else {
            return Err(invalid_arg(
                "layer",
                &format!("{} has no copper layer '{layer}'", stack.id),
            ));
        };
        let n = stack.copper_mm.len();
        let outer = i == 0 || i + 1 == n;
        if outer == stripline {
            return Err(invalid_arg(
                "structure",
                &format!(
                    "{layer} is an {} layer; use {}",
                    if outer { "outer" } else { "inner" },
                    if outer {
                        "microstrip, cpwg or diff_microstrip"
                    } else {
                        "stripline or diff_stripline"
                    }
                ),
            ));
        }
        let t = stack.copper_mm[i];
        let geo = if outer {
            let d = if i == 0 {
                stack.dielectrics[0]
            } else {
                stack.dielectrics[n - 2]
            };
            Geometry {
                h: d.thickness_mm,
                h2: None,
                t,
                er: d.er,
                source: json!({}),
            }
        } else {
            let above = stack.dielectrics[i - 1];
            let below = stack.dielectrics[i];
            let er = (above.er * above.thickness_mm + below.er * below.thickness_mm)
                / (above.thickness_mm + below.thickness_mm);
            Geometry {
                h: above.thickness_mm,
                h2: Some(below.thickness_mm),
                t,
                er,
                source: json!({}),
            }
        };
        let mut geo = geo;
        geo.source = json!({
            "stackup": stack.id, "layer": copper_layer_name(i, n),
            "derived": { "height_mm": geo.h, "height2_mm": geo.h2, "thickness_mm": geo.t, "er": sig(geo.er, 4) },
            "data_source": STACKUP_SOURCE,
            "assumption": if outer {
                "the adjacent inner copper layer is a solid reference plane"
            } else {
                "both adjacent copper layers are solid reference planes; εr is the thickness-weighted mix"
            }
        });
        if let Some(t) = opt_f64(args, "thickness_mm") {
            geo.t = t;
        }
        return Ok(geo);
    }
    let h = opt_f64(args, "height_mm")
        .ok_or_else(|| invalid_arg("height_mm", "required unless stackup is given"))?;
    let er =
        opt_f64(args, "er").ok_or_else(|| invalid_arg("er", "required unless stackup is given"))?;
    let t = opt_f64(args, "thickness_mm").unwrap_or(0.035);
    Ok(Geometry {
        h,
        h2: opt_f64(args, "height2_mm"),
        t,
        er,
        source: json!({ "geometry": "caller-supplied" }),
    })
}

fn impedance_at(
    structure: &str,
    w: f64,
    g: &Geometry,
    gap: Option<f64>,
) -> (f64, f64, Option<f64>) {
    match structure {
        "microstrip" => {
            let (z, e) = calc::microstrip(w, g.h, g.t, g.er);
            (z, e, None)
        }
        "diff_microstrip" => {
            let (z, e) = calc::microstrip(w, g.h, g.t, g.er);
            (z, e, Some(calc::diff_microstrip(z, gap.unwrap_or(w), g.h)))
        }
        "cpwg" => {
            let (z, e) = calc::cpwg(w, gap.unwrap_or(w), g.h, g.er);
            (z, e, None)
        }
        "stripline" | "diff_stripline" => {
            let z = match g.h2 {
                Some(h2) if (h2 - g.h).abs() > 1e-9 => calc::stripline_asym(w, g.h, h2, g.t, g.er),
                _ => calc::stripline(w, 2.0 * g.h + g.t, g.t, g.er),
            };
            let b = g.h + g.h2.unwrap_or(g.h) + g.t;
            let zd = (structure == "diff_stripline")
                .then(|| calc::diff_stripline(z, gap.unwrap_or(w), b));
            (z, g.er, zd)
        }
        _ => (f64::NAN, f64::NAN, None),
    }
}

fn handle_impedance(args: &Value) -> anyhow::Result<CallToolResult> {
    let structure = match require_str(args, "structure") {
        Ok(s) => s.to_string(),
        Err(e) => return Ok(e),
    };
    let g = match geometry_from_args(args, &structure) {
        Ok(g) => g,
        Err(e) => return Ok(e),
    };
    let gap = opt_f64(args, "gap_mm");
    let is_diff = structure.starts_with("diff_");
    if (structure == "cpwg" || is_diff) && gap.is_none() {
        return Ok(invalid_arg(
            "gap_mm",
            "required for cpwg and differential structures",
        ));
    }
    let width = match (opt_f64(args, "width_mm"), opt_f64(args, "target_ohm")) {
        (Some(w), None) => w,
        (None, Some(target)) => {
            let f = |w: f64| {
                let (z, _, zd) = impedance_at(&structure, w, &g, gap);
                if is_diff {
                    zd.unwrap_or(z)
                } else {
                    z
                }
            };
            match calc::solve_decreasing(target, 0.01, 20.0, f) {
                Some(w) => w,
                None => {
                    return Ok(CallToolResult::error(format!(
                        "No trace width between 0.01 and 20 mm gives {target} Ω on this geometry; \
                         change the dielectric height, gap or structure"
                    )))
                }
            }
        }
        _ => {
            return Ok(invalid_arg(
                "width_mm",
                "pass exactly one of width_mm or target_ohm",
            ))
        }
    };
    let (z0, eeff, zdiff) = impedance_at(&structure, width, &g, gap);
    let method = match structure.as_str() {
        "microstrip" => "Hammerstad–Jensen (1980) with Hammerstad thickness correction; ≤1% for 0.01 ≤ W/H ≤ 100",
        "cpwg" => "Conductor-backed CPW conformal mapping (Wadell 1991), zero-thickness conductors; thick copper lowers Z slightly",
        "stripline" => "Wheeler (1978) finite-thickness stripline; offset placement combined per IPC-2141A",
        "diff_microstrip" => "Single-ended Hammerstad–Jensen, coupled with IPC-2141 Zdiff = 2·Z0·(1−0.48·e^(−0.96·S/H)); ±5–10%",
        _ => "Single-ended Wheeler stripline, coupled with IPC-2141 Zdiff = 2·Z0·(1−0.347·e^(−2.9·S/B)); ±5–10%",
    };
    let mut warnings =
        vec!["Excludes solder mask (≈1–3 Ω lower on outer layers) and etch trapezoid".to_string()];
    if is_diff {
        warnings.push(
            "Coupled-line estimate: confirm with the fab's field-solver calculator before ordering"
                .into(),
        );
    }
    if structure == "microstrip" && !(0.01..=100.0).contains(&(width / g.h)) {
        warnings.push("W/H outside the Hammerstad–Jensen fit range".into());
    }
    let mut out = json!({
        "structure": structure,
        "width_mm": r3(width),
        "z0_ohm": sig(z0, 4),
        "eeff": sig(eeff, 4),
        "delay_ps_per_mm": sig(calc::delay_ps_per_mm(eeff), 4),
        "geometry": { "height_mm": g.h, "height2_mm": g.h2, "thickness_mm": g.t, "er": sig(g.er, 4), "gap_mm": gap },
        "basis": g.source,
        "method": method,
        "warnings": warnings,
    });
    if let Some(zd) = zdiff {
        out["zdiff_ohm"] = json!(sig(zd, 4));
    }
    if let Some(target) = opt_f64(args, "target_ohm") {
        out["target_ohm"] = json!(target);
        out["recommended_width_mm"] =
            json!(((width / 0.005).round() * 0.005 * 1000.0).round() / 1000.0);
    }
    Ok(CallToolResult::json(&out))
}

fn handle_via(args: &Value) -> anyhow::Result<CallToolResult> {
    let drill = match require_f64(args, "drill_mm") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let plating = opt_f64(args, "plating_mm").unwrap_or(0.018);
    if plating * 2.0 >= drill {
        return Ok(invalid_arg(
            "plating_mm",
            "plating must be less than half the drill diameter",
        ));
    }
    let length = opt_f64(args, "length_mm").unwrap_or(1.6);
    let rise = opt_f64(args, "temp_rise_c").unwrap_or(10.0);
    let area = calc::via_barrel_area_mm2(drill, plating);
    let area_mil2 = area / (calc::MM_PER_MIL * calc::MM_PER_MIL);
    let i_max = calc::ipc2221_k(true) * rise.powf(0.44) * area_mil2.powf(0.725);
    let r = calc::copper_resistance_ohm(length, area, 25.0 + rise);
    let mut out = json!({
        "drill_mm": drill, "plating_mm": plating, "length_mm": length,
        "barrel_area_mm2": sig(area, 4),
        "resistance_mohm": sig(r * 1000.0, 4),
        "inductance_nh": sig(calc::via_inductance_nh(length, drill), 3),
        "current_capacity_a": sig(i_max, 3),
        "method": "Barrel area π·t·(d−t); IPC-2221 external constant on that area; Goldfarb L = 0.2·h·(ln(4h/d)+1) nH",
        "note": "Vias also conduct heat into planes, so real capacity is usually higher; this is the conservative figure."
    });
    if let Some(i) = opt_f64(args, "current_a") {
        out["current_a"] = json!(i);
        out["vias_needed"] = json!((i / i_max).ceil().max(1.0));
    }
    Ok(CallToolResult::json(&out))
}

fn handle_regulator(args: &Value) -> anyhow::Result<CallToolResult> {
    let get = |k| require_f64(args, k);
    let (vin, vout, iout, theta) = match (
        get("vin"),
        get("vout"),
        get("iout_a"),
        get("theta_ja_c_per_w"),
    ) {
        (Ok(a), Ok(b), Ok(c), Ok(d)) => (a, b, c, d),
        (Err(e), ..) | (_, Err(e), ..) | (_, _, Err(e), _) | (.., Err(e)) => return Ok(e),
    };
    let iq = opt_f64(args, "iq_a").unwrap_or(0.0);
    let ta = opt_f64(args, "ambient_c").unwrap_or(40.0);
    let tj_max = opt_f64(args, "tj_max_c").unwrap_or(125.0);
    if vin <= vout {
        return Ok(invalid_arg(
            "vin",
            "must exceed vout for a linear regulator",
        ));
    }
    let pd = (vin - vout) * iout + vin * iq;
    let tj = ta + pd * theta;
    let p_allow = (tj_max - ta) / theta;
    let i_max = ((p_allow - vin * iq) / (vin - vout)).max(0.0);
    let mut issues = Vec::new();
    if tj > tj_max {
        issues.push(format!("Junction reaches {:.0} °C, above the {:.0} °C limit — the part will enter thermal shutdown", tj, tj_max));
    } else if tj > tj_max - 20.0 {
        issues.push(format!(
            "Junction reaches {:.0} °C — less than 20 °C margin; add copper or reduce Vin",
            tj
        ));
    }
    if let Some(d) = opt_f64(args, "dropout_v") {
        if vin - vout < d {
            issues.push(format!(
                "Headroom {:.2} V is below the {:.2} V dropout — output will not regulate",
                vin - vout,
                d
            ));
        }
    }
    Ok(CallToolResult::json(&json!({
        "power_dissipation_w": sig(pd, 3),
        "junction_temp_c": sig(tj, 3),
        "efficiency_pct": sig(vout * iout / (vin * (iout + iq)).max(1e-30) * 100.0, 3),
        "max_current_at_this_theta_a": sig(i_max, 3),
        "verdict": if issues.iter().any(|i| i.contains("shutdown") || i.contains("not regulate")) { "fail" } else if issues.is_empty() { "ok" } else { "marginal" },
        "issues": issues,
        "method": "Pd = (Vin−Vout)·Iout + Vin·Iq; Tj = Ta + Pd·θJA",
    })))
}

fn handle_divider(args: &Value) -> anyhow::Result<CallToolResult> {
    let mode = match require_str(args, "mode") {
        Ok(m) => m.to_string(),
        Err(e) => return Ok(e),
    };
    let vout = match require_f64(args, "vout") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let source_v = if mode == "divider" {
        opt_f64(args, "vin")
    } else {
        opt_f64(args, "vref")
    };
    let Some(vs) = source_v else {
        return Ok(invalid_arg(
            if mode == "divider" { "vin" } else { "vref" },
            "required for this mode",
        ));
    };
    if mode == "divider" && vout >= vs {
        return Ok(invalid_arg("vout", "must be below vin for a divider"));
    }
    if mode == "feedback" && vout <= vs {
        return Ok(invalid_arg(
            "vout",
            "must be above vref for a feedback network",
        ));
    }
    let series_name = opt_str(args, "series").unwrap_or("E96");
    let Some(mant) = calc::series(series_name) else {
        return Ok(invalid_arg("series", "use E12, E24, E48 or E96"));
    };
    let lo = opt_f64(args, "r_min_ohm").unwrap_or(1e3);
    let hi = opt_f64(args, "r_max_ohm").unwrap_or(1e6);
    let values = calc::series_values(&mant, lo, hi);
    let bottoms = match opt_f64(args, "fixed_rbot_ohm") {
        Some(r) => vec![r],
        None => values.clone(),
    };
    let calc_out = |rt: f64, rb: f64| {
        if mode == "divider" {
            vs * rb / (rt + rb)
        } else {
            vs * (1.0 + rt / rb)
        }
    };
    let mut best: Vec<(f64, f64, f64)> = Vec::new();
    for &rb in &bottoms {
        for &rt in &values {
            let v = calc_out(rt, rb);
            best.push(((v - vout).abs() / vout, rt, rb));
        }
    }
    best.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap()
            .then((a.1 + a.2).partial_cmp(&(b.1 + b.2)).unwrap())
    });
    best.dedup_by(|a, b| (a.0 - b.0).abs() < 1e-12);
    let rows: Vec<Value> = best
        .iter()
        .take(5)
        .map(|(err, rt, rb)| {
            let v = calc_out(*rt, *rb);
            let i = if mode == "divider" {
                vs / (rt + rb)
            } else {
                v / (rt + rb)
            };
            json!({
                "r_top": eng(*rt, "Ω"), "r_bot": eng(*rb, "Ω"),
                "r_top_ohm": rt, "r_bot_ohm": rb,
                "vout": sig(v, 5), "error_pct": (err * 100.0 * 1e4).round() / 1e4,
                "divider_current_ua": sig(i * 1e6, 3),
            })
        })
        .collect();
    Ok(CallToolResult::json(&json!({
        "mode": mode, "target_vout": vout, "series": series_name,
        "range_ohm": [lo, hi],
        "best": rows,
        "note": "Error excludes resistor tolerance: with 1% parts the worst-case output shifts by about ±2% (divider) or ±2%·(1−Vref/Vout) (feedback). For ADC inputs keep the source impedance within the ADC's limit.",
    })))
}

fn handle_passive(args: &Value) -> anyhow::Result<CallToolResult> {
    let kind = match require_str(args, "kind") {
        Ok(k) => k,
        Err(e) => return Ok(e),
    };
    let need = |k: &str| {
        opt_f64(args, k).ok_or_else(|| invalid_arg(k, &format!("required for kind '{kind}'")))
    };
    let result = (|| -> Result<Value, CallToolResult> {
        Ok(match kind {
            "led" => {
                let (vs, vf, ima) = (need("supply_v")?, need("vf_v")?, need("if_ma")?);
                if vf >= vs {
                    return Err(invalid_arg(
                        "vf_v",
                        "forward voltage must be below the supply",
                    ));
                }
                let r = (vs - vf) / (ima / 1000.0);
                let rs = calc::nearest_at_least(&calc::E24, r);
                let ia = (vs - vf) / rs;
                let p = ia * ia * rs;
                json!({
                    "exact_ohm": sig(r, 4), "resistor_ohm": rs, "resistor": eng(rs, "Ω"),
                    "actual_if_ma": sig(ia * 1000.0, 3), "resistor_power_mw": sig(p * 1000.0, 3),
                    "min_resistor_rating": if p * 2.0 <= 0.0625 { "0402 (1/16 W)" } else if p * 2.0 <= 0.1 { "0603 (1/10 W)" } else if p * 2.0 <= 0.125 { "0805 (1/8 W)" } else if p * 2.0 <= 0.25 { "1206 (1/4 W)" } else { "≥ 1/2 W — use a current-limit driver instead" },
                    "method": "R = (Vs − Vf)/If rounded up to E24; rating at 2× dissipation"
                })
            }
            "rc" => {
                let (r, c, f) = (
                    opt_f64(args, "r_ohm"),
                    opt_f64(args, "c_f"),
                    opt_f64(args, "fc_hz"),
                );
                let two_pi = 2.0 * std::f64::consts::PI;
                let (r, c, f) = match (r, c, f) {
                    (Some(r), Some(c), None) => (r, c, 1.0 / (two_pi * r * c)),
                    (Some(r), None, Some(f)) => (r, 1.0 / (two_pi * r * f), f),
                    (None, Some(c), Some(f)) => (1.0 / (two_pi * c * f), c, f),
                    _ => {
                        return Err(invalid_arg(
                            "r_ohm",
                            "pass exactly two of r_ohm, c_f, fc_hz",
                        ))
                    }
                };
                json!({
                    "r_ohm": sig(r, 4), "c_f": sig(c, 4), "fc_hz": sig(f, 4),
                    "r": eng(r, "Ω"), "c": eng(c, "F"), "fc": eng(f, "Hz"),
                    "nearest_standard": { "r": eng(calc::nearest(&calc::E24, r), "Ω"), "c": eng(calc::nearest(&calc::series("E12").unwrap(), c), "F") },
                    "time_constant_s": sig(r * c, 4), "settle_to_1pct_s": sig(4.6 * r * c, 4),
                    "method": "fc = 1/(2πRC); −3 dB at fc, first order (20 dB/decade)"
                })
            }
            "crystal" => {
                let cl = need("cl_pf")?;
                let cs = opt_f64(args, "cstray_pf").unwrap_or(4.0);
                let c = 2.0 * (cl - cs);
                if c <= 0.0 {
                    return Err(invalid_arg(
                        "cstray_pf",
                        "stray capacitance must be below CL",
                    ));
                }
                let std = calc::nearest(&calc::series("E12").unwrap(), c);
                json!({
                    "c1_c2_exact_pf": sig(c, 3), "c1_c2_standard_pf": std,
                    "resulting_cl_pf": sig(std / 2.0 + cs, 3),
                    "method": "C1 = C2 = 2·(CL − Cstray); CL = C1·C2/(C1+C2) + Cstray",
                    "note": "Use C0G/NP0 capacitors. Many MCU datasheets (e.g. ESP32) state the exact load caps for their recommended crystal — prefer those."
                })
            }
            "i2c" => {
                let vdd = need("supply_v")?;
                let cb = need("bus_pf")? * 1e-12;
                let mode = opt_str(args, "i2c_mode").unwrap_or("fast");
                let (tr, iol) = match mode {
                    "standard" => (1000e-9, 3e-3),
                    "fast_plus" => (120e-9, 20e-3),
                    _ => (300e-9, 3e-3),
                };
                let vol = if vdd > 2.0 { 0.4 } else { 0.2 * vdd };
                let rmin = (vdd - vol) / iol;
                let rmax = tr / (0.8473 * cb);
                if rmax < rmin {
                    json!({ "verdict": "infeasible", "r_min_ohm": sig(rmin, 3), "r_max_ohm": sig(rmax, 3),
                        "advice": "Bus capacitance is too high for this mode: lower the speed, shorten the bus, or add a buffer (e.g. PCA9515)." })
                } else {
                    let rec = calc::nearest_at_most(&calc::E24, rmax * 0.8)
                        .filter(|r| *r >= rmin)
                        .unwrap_or(rmin);
                    json!({ "verdict": "ok", "mode": mode, "r_min_ohm": sig(rmin, 3), "r_max_ohm": sig(rmax, 3),
                        "recommended_ohm": rec, "recommended": eng(rec, "Ω"),
                        "method": "NXP UM10204: Rmin = (Vdd−VOL)/IOL, Rmax = tr/(0.8473·Cb)",
                        "note": "Fit one pull-up pair per bus, not one per device. Include each device's pin capacitance in bus_pf." })
                }
            }
            "battery" => {
                let cap = need("capacity_mah")?;
                let i = need("avg_current_ma")?;
                let d = opt_f64(args, "derating").unwrap_or(0.8);
                let h = cap * d / i;
                json!({ "runtime_hours": sig(h, 3), "runtime_days": sig(h / 24.0, 3),
                    "method": "t = capacity × derating / average current",
                    "note": "Average current must include sleep current and regulator quiescent current; Li-ion capacity also falls in cold conditions." })
            }
            _ => return Err(invalid_arg("kind", "unknown calculator")),
        })
    })();
    Ok(match result {
        Ok(v) => CallToolResult::json(&json!({ "kind": kind, "result": v })),
        Err(e) => e,
    })
}

fn handle_buck(args: &Value) -> anyhow::Result<CallToolResult> {
    let get = |k| require_f64(args, k);
    let vals: Result<Vec<f64>, _> = ["vin_min", "vin_max", "vout", "iout_a", "fsw_hz"]
        .into_iter()
        .map(get)
        .collect();
    let v = match vals {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let (vin_min, vin_max, vout, iout, fsw) = (v[0], v[1], v[2], v[3], v[4]);
    if vin_min > vin_max {
        return Ok(invalid_arg("vin_min", "must not exceed vin_max"));
    }
    if vout >= vin_min {
        return Ok(invalid_arg("vout", "a buck needs vout below vin_min"));
    }
    let r = opt_f64(args, "ripple_ratio").unwrap_or(0.3);
    let eff = opt_f64(args, "efficiency").unwrap_or(0.9);
    let dv = opt_f64(args, "vout_ripple_mv")
        .map(|m| m / 1000.0)
        .unwrap_or(vout * 0.01);
    let di = r * iout;
    let l = vout * (vin_max - vout) / (vin_max * fsw * di);
    let l_std = calc::nearest_at_least(&[1.0, 1.5, 2.2, 3.3, 4.7, 6.8], l);
    let di_std = vout * (vin_max - vout) / (vin_max * fsw * l_std);
    let ipk = iout + di_std / 2.0;
    let cout = di_std / (8.0 * fsw * dv);
    let esr = dv / di_std;
    let d_max = (vout / (vin_min * eff)).min(1.0);
    let d_worst = if d_max >= 0.5 && vout / (vin_max * eff) <= 0.5 {
        0.5
    } else {
        d_max
    };
    let cin_rms = iout * (d_worst * (1.0 - d_worst)).sqrt();
    Ok(CallToolResult::json(&json!({
        "duty_cycle_range": [sig(vout / (vin_max * eff), 3), sig(d_max, 3)],
        "inductor": {
            "computed_uh": sig(l * 1e6, 3), "standard_uh": sig(l_std * 1e6, 3),
            "ripple_a": sig(di_std, 3), "peak_a": sig(ipk, 3),
            "min_saturation_a": sig(ipk * 1.3, 3),
            "min_rms_rating_a": sig((iout * iout + di_std * di_std / 12.0).sqrt(), 3)
        },
        "output_capacitor": {
            "min_effective_uf": sig(cout * 1e6, 3), "max_esr_mohm": sig(esr * 1000.0, 3),
            "note": "Effective capacitance: ceramic X5R/X7R loses 30–70% at DC bias — check the vendor's DC-bias curve and buy 2–3× the figure. Loop stability may require more; follow the datasheet."
        },
        "input_capacitor": { "rms_current_a": sig(cin_rms, 3), "note": "Place within 2 mm of VIN/GND pins; this loop has the highest di/dt on the board." },
        "method": "CCM buck: L = Vout(Vin_max−Vout)/(Vin_max·fsw·ΔIL); Cout = ΔIL/(8·fsw·ΔVout); Icin,rms = Iout·√(D(1−D))"
    })))
}

#[cfg(test)]
mod tests {
    use super::calc::*;
    use super::*;

    fn close(a: f64, b: f64, rel: f64) -> bool {
        ((a - b) / b).abs() <= rel
    }

    #[test]
    fn ipc2221_reference_point() {
        // 1 A, 10 °C, 1 oz external → the textbook ≈ 0.30 mm (11.8 mil).
        let w = ipc2221_width_mm(1.0, 10.0, 0.035, true);
        assert!(close(w, 0.30, 0.03), "{w}");
        let i = ipc2221_current_a(w, 0.035, 10.0, true);
        assert!(close(i, 1.0, 1e-9));
        assert!(ipc2221_width_mm(1.0, 10.0, 0.035, false) > w * 2.5);
    }

    #[test]
    fn elliptic_k_known_values() {
        assert!(close(ellip_k(0.0), std::f64::consts::FRAC_PI_2, 1e-12));
        assert!(close(ellip_k(0.5_f64.sqrt()), 1.854_074_677_301_372, 1e-12));
    }

    #[test]
    fn microstrip_matches_scikit_rf_hammerstad_jensen() {
        // Reference values from scikit-rf 2.2.0 MLine(model='hammerstadjensen'),
        // quasi-static (dispersion off). See tests/data/engineering_reference.py.
        for (w, h, t, er, z_ref, e_ref) in REF_MICROSTRIP {
            let (z, e) = microstrip(*w, *h, *t, *er);
            assert!(close(z, *z_ref, 0.002), "w={w} h={h}: {z} vs {z_ref}");
            assert!(close(e, *e_ref, 0.002), "w={w} h={h}: {e} vs {e_ref}");
        }
    }

    #[test]
    fn wheeler_stripline_converges_to_exact_thin_strip() {
        for (w, b) in [(0.1, 0.5), (0.2, 0.5), (0.3, 1.0), (1.0, 1.2)] {
            let exact = stripline_exact_thin(w, b, 4.3);
            let wheeler = stripline(w, b, 1e-9, 4.3);
            assert!(
                close(wheeler, exact, 0.006),
                "w={w} b={b}: {wheeler} vs {exact}"
            );
        }
        let thick = stripline(0.2, 0.5, 0.035, 4.3);
        assert!(thick < stripline(0.2, 0.5, 1e-9, 4.3));
        let sym = stripline_asym(0.2, 0.2325, 0.2325, 0.035, 4.3);
        assert!(close(sym, stripline(0.2, 0.5, 0.035, 4.3), 1e-9));
    }

    #[test]
    fn cpwg_matches_scikit_rf() {
        for (w, s, h, er, z_ref, e_ref) in REF_CPWG {
            let (z, e) = cpwg(*w, *s, *h, *er);
            assert!(close(z, *z_ref, 0.005), "w={w} s={s}: {z} vs {z_ref}");
            assert!(close(e, *e_ref, 0.005), "w={w} s={s}: {e} vs {e_ref}");
        }
    }

    #[test]
    fn width_solver_inverts_the_model() {
        let w =
            solve_decreasing(50.0, 0.01, 20.0, |w| microstrip(w, 0.2104, 0.035, 4.4).0).unwrap();
        assert!(close(microstrip(w, 0.2104, 0.035, 4.4).0, 50.0, 1e-6));
        assert!(w > 0.3 && w < 0.45, "{w}");
    }

    #[test]
    fn e_series_tables_are_complete_and_sorted() {
        assert_eq!(E96.len(), 96);
        assert!(E96.windows(2).all(|p| p[0] < p[1]));
        assert_eq!(
            series("E12").unwrap(),
            vec![1.0, 1.2, 1.5, 1.8, 2.2, 2.7, 3.3, 3.9, 4.7, 5.6, 6.8, 8.2]
        );
        assert_eq!(nearest_at_least(&E24, 430.0), 430.0);
        assert_eq!(nearest_at_least(&E24, 431.0), 470.0);
    }

    #[test]
    fn divider_finds_exact_feedback_pair() {
        let out = handle_divider(&json!({"mode":"feedback","vref":0.6,"vout":3.3,"series":"E96","fixed_rbot_ohm":10000.0})).unwrap();
        let v: Value = serde_json::from_str(match &out.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text,
            _ => panic!(),
        })
        .unwrap();
        assert_eq!(v["best"][0]["r_top_ohm"], 45300.0);
        assert!(v["best"][0]["error_pct"].as_f64().unwrap() < 0.6);
    }

    #[test]
    fn stackup_presets_resolve_layers() {
        let s = stackup("jlc04161h-7628").unwrap();
        assert_eq!(layer_index(s, "B.Cu"), Some(3));
        let g = geometry_from_args(
            &json!({"stackup":"JLC04161H-7628","layer":"F.Cu"}),
            "microstrip",
        )
        .ok()
        .unwrap();
        assert_eq!((g.h, g.t, g.er), (0.2104, 0.035, 4.4));
        assert!(geometry_from_args(
            &json!({"stackup":"JLC04161H-7628","layer":"In1.Cu"}),
            "microstrip"
        )
        .is_err());
        let six = geometry_from_args(
            &json!({"stackup":"JLC06161H-NOREQ","layer":"In2.Cu"}),
            "stripline",
        )
        .ok()
        .unwrap();
        assert_eq!((six.h, six.h2), (0.55, Some(0.1164)));
    }

    #[test]
    fn i2c_and_crystal_follow_their_standards() {
        // UM10204 fast mode, 3.3 V, 200 pF: Rmin 967 Ω, Rmax 1770 Ω.
        let out =
            handle_passive(&json!({"kind":"i2c","supply_v":3.3,"bus_pf":200.0,"i2c_mode":"fast"}))
                .unwrap();
        let text = match &out.content[0] {
            crate::mcp::protocol::ToolContent::Text { text } => text.clone(),
            _ => panic!(),
        };
        let v: Value = serde_json::from_str(&text).unwrap();
        assert!(close(
            v["result"]["r_min_ohm"].as_f64().unwrap(),
            967.0,
            0.002
        ));
        assert!(close(
            v["result"]["r_max_ohm"].as_f64().unwrap(),
            1770.0,
            0.002
        ));
        let rec = v["result"]["recommended_ohm"].as_f64().unwrap();
        assert!((967.0..=1770.0).contains(&rec));
    }

    // Generated by crates/voltaris-core/tests/data/engineering_reference.py (scikit-rf 2.2.0).
    const REF_MICROSTRIP: &[(f64, f64, f64, f64, f64, f64)] = &[
        (0.36, 0.2104, 0.035, 4.4, 50.9220, 3.1763),
        (0.2, 0.2104, 0.035, 4.4, 67.9407, 2.9929),
        (0.18, 0.0994, 0.035, 4.1, 49.2177, 2.9354),
        (1.0, 0.2104, 0.035, 4.4, 26.2973, 3.5497),
        (3.0, 1.53, 0.035, 4.5, 48.3428, 3.3807),
        (0.5, 1.53, 0.035, 4.5, 106.7684, 2.9870),
        (0.1, 0.0764, 0.0, 3.91, 65.2917, 2.9109),
        (2.0, 0.5, 0.0, 10.2, 20.5387, 7.8982),
    ];
    const REF_CPWG: &[(f64, f64, f64, f64, f64, f64)] = &[
        (0.3, 0.15, 0.2104, 4.4, 54.7208, 3.0513),
        (0.5, 0.2, 0.2104, 4.4, 42.0979, 3.2572),
        (1.0, 0.15, 1.53, 4.5, 50.0671, 2.8045),
        (0.25, 0.1, 0.0994, 4.1, 42.1222, 3.0853),
    ];
}
