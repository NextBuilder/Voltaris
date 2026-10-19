//! A SPICE netlist subset for the built-in linear solver.

use super::mna::{Circuit, Element, Kind, Source, Wave};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Analysis {
    Op,
    Ac {
        sweep: String,
        points: usize,
        fstart: f64,
        fstop: f64,
    },
    Tran {
        tstep: f64,
        tstop: f64,
        uic: bool,
    },
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub circuit: Circuit,
    pub analyses: Vec<Analysis>,
    pub ic: BTreeMap<usize, f64>,
    pub warnings: Vec<String>,
}

/// SPICE number: `10k`, `4.7u`, `100nF`, `1meg`, `2.2e-6`. As in SPICE, `m` is milli and `meg` is mega.
pub fn number(token: &str) -> Option<f64> {
    let t = token.trim().to_ascii_lowercase();
    let split = t
        .char_indices()
        .find(|(i, c)| {
            !(c.is_ascii_digit()
                || *c == '.'
                || *c == '-'
                || *c == '+'
                || ((*c == 'e')
                    && t[i + 1..]
                        .starts_with(|d: char| d.is_ascii_digit() || d == '-' || d == '+')))
        })
        .map(|(i, _)| i)
        .unwrap_or(t.len());
    let base: f64 = t[..split].parse().ok()?;
    let rest = &t[split..];
    let mult = if rest.starts_with("meg") {
        1e6
    } else if rest.starts_with("mil") {
        25.4e-6
    } else {
        match rest.chars().next() {
            None => 1.0,
            Some('t') => 1e12,
            Some('g') => 1e9,
            Some('k') => 1e3,
            Some('m') => 1e-3,
            Some('u') | Some('µ') => 1e-6,
            Some('n') => 1e-9,
            Some('p') => 1e-12,
            Some('f') => 1e-15,
            Some(c) if c.is_ascii_alphabetic() => 1.0,
            _ => return None,
        }
    };
    Some(base * mult)
}

fn tokens(line: &str) -> Vec<String> {
    let spaced = line
        .replace('(', " ( ")
        .replace(')', " ) ")
        .replace(',', " ")
        .replace('=', " = ");
    spaced.split_whitespace().map(str::to_string).collect()
}

fn group(toks: &[String], start: usize) -> Option<(Vec<f64>, usize)> {
    let mut i = start;
    if toks.get(i).map(String::as_str) != Some("(") {
        return None;
    }
    i += 1;
    let mut vals = Vec::new();
    while let Some(t) = toks.get(i) {
        if t == ")" {
            return Some((vals, i + 1));
        }
        vals.push(number(t)?);
        i += 1;
    }
    None
}

fn source(toks: &[String], name: &str) -> Result<Source, String> {
    let mut s = Source {
        dc: 0.0,
        ac_mag: 0.0,
        ac_phase_deg: 0.0,
        wave: Wave::Dc,
    };
    let mut i = 0;
    while i < toks.len() {
        let t = toks[i].to_ascii_lowercase();
        match t.as_str() {
            "dc" => {
                s.dc = toks
                    .get(i + 1)
                    .and_then(|v| number(v))
                    .ok_or(format!("{name}: DC needs a value"))?;
                i += 2;
            }
            "ac" => {
                s.ac_mag = toks.get(i + 1).and_then(|v| number(v)).unwrap_or(1.0);
                i += 2;
                if let Some(ph) = toks.get(i).and_then(|v| number(v)) {
                    s.ac_phase_deg = ph;
                    i += 1;
                }
            }
            "pulse" | "sin" | "pwl" => {
                let (v, next) = group(toks, i + 1)
                    .ok_or(format!("{name}: {t} needs a parenthesised value list"))?;
                let g = |k: usize| v.get(k).copied().unwrap_or(0.0);
                s.wave = match t.as_str() {
                    "pulse" => Wave::Pulse {
                        v1: g(0),
                        v2: g(1),
                        td: g(2),
                        tr: g(3),
                        tf: g(4),
                        pw: if v.len() > 5 { g(5) } else { f64::INFINITY },
                        per: g(6),
                    },
                    "sin" => Wave::Sin {
                        vo: g(0),
                        va: g(1),
                        freq: g(2),
                        td: g(3),
                        theta: g(4),
                    },
                    _ => {
                        if v.len() % 2 != 0 {
                            return Err(format!("{name}: PWL needs time/value pairs"));
                        }
                        Wave::Pwl(v.chunks(2).map(|p| (p[0], p[1])).collect())
                    }
                };
                if let Wave::Pulse { v1, .. } | Wave::Sin { vo: v1, .. } = &s.wave {
                    if s.dc == 0.0 {
                        s.dc = *v1;
                    }
                }
                if let Wave::Pwl(p) = &s.wave {
                    s.dc = p.first().map(|x| x.1).unwrap_or(0.0);
                }
                i = next;
            }
            _ => {
                if let Some(v) = number(&t) {
                    s.dc = v;
                    i += 1;
                } else {
                    return Err(format!("{name}: cannot read '{}'", toks[i]));
                }
            }
        }
    }
    if s.wave == Wave::Dc {
        s.wave = Wave::Dc;
    }
    Ok(s)
}

pub fn parse(text: &str) -> Result<Parsed, String> {
    let mut out = Parsed::default();
    out.circuit.node("0");
    let mut lines: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.split(';').next().unwrap_or("").trim();
        if line.is_empty() || line.starts_with('*') {
            continue;
        }
        if let Some(cont) = line.strip_prefix('+') {
            if let Some(last) = lines.last_mut() {
                last.push(' ');
                last.push_str(cont.trim());
            }
            continue;
        }
        lines.push(line.to_string());
    }
    let mut in_control = false;
    for (n, line) in lines.iter().enumerate() {
        let lower = line.to_ascii_lowercase();
        if lower.starts_with(".control") {
            in_control = true;
            continue;
        }
        if lower.starts_with(".endc") {
            in_control = false;
            continue;
        }
        if in_control {
            continue;
        }
        let toks = tokens(line);
        let head = toks[0].to_ascii_lowercase();
        if head.starts_with('.') {
            match head.as_str() {
                ".op" => out.analyses.push(Analysis::Op),
                ".ac" => {
                    let sweep = toks
                        .get(1)
                        .map(|s| s.to_ascii_lowercase())
                        .unwrap_or_default();
                    let pts = toks
                        .get(2)
                        .and_then(|v| number(v))
                        .ok_or(".ac needs: dec|oct|lin N fstart fstop")?;
                    let f1 = toks
                        .get(3)
                        .and_then(|v| number(v))
                        .ok_or(".ac needs fstart")?;
                    let f2 = toks
                        .get(4)
                        .and_then(|v| number(v))
                        .ok_or(".ac needs fstop")?;
                    if !["dec", "oct", "lin"].contains(&sweep.as_str()) || f1 <= 0.0 || f2 <= f1 {
                        return Err(format!("line {}: invalid .ac sweep", n + 1));
                    }
                    out.analyses.push(Analysis::Ac {
                        sweep,
                        points: pts as usize,
                        fstart: f1,
                        fstop: f2,
                    });
                }
                ".tran" => {
                    let step = toks
                        .get(1)
                        .and_then(|v| number(v))
                        .ok_or(".tran needs tstep tstop")?;
                    let stop = toks
                        .get(2)
                        .and_then(|v| number(v))
                        .ok_or(".tran needs tstop")?;
                    let uic = toks.iter().any(|t| t.eq_ignore_ascii_case("uic"));
                    out.analyses.push(Analysis::Tran {
                        tstep: step,
                        tstop: stop,
                        uic,
                    });
                }
                ".ic" => {
                    let mut i = 1;
                    while i + 5 < toks.len() + 1 && i < toks.len() {
                        if toks[i].eq_ignore_ascii_case("v")
                            && toks.get(i + 1).map(String::as_str) == Some("(")
                        {
                            let node = out.circuit.node(&toks[i + 2]);
                            let val = toks
                                .get(i + 5)
                                .and_then(|v| number(v))
                                .ok_or(".ic needs V(node)=value")?;
                            out.ic.insert(node, val);
                            i += 6;
                        } else {
                            i += 1;
                        }
                    }
                }
                ".end" | ".title" | ".save" | ".options" | ".option" | ".probe" | ".temp" => {}
                other => out.warnings.push(format!(
                    "line {}: '{other}' is not supported by the built-in solver and was ignored",
                    n + 1
                )),
            }
            continue;
        }
        let name = toks[0].clone();
        let first = head.chars().next().unwrap();
        let need = |k: usize| -> Result<&String, String> {
            toks.get(k).ok_or(format!("{name}: missing field {k}"))
        };
        let value = |k: usize| -> Result<f64, String> {
            let t = need(k)?;
            number(t).ok_or(format!("{name}: '{t}' is not a number"))
        };
        let kind_known = matches!(first, 'r' | 'c' | 'l' | 'v' | 'i' | 'e' | 'g');
        if !kind_known {
            if n == 0 {
                continue;
            }
            return Err(format!(
                "{name}: element type '{}' is nonlinear or unsupported in the built-in linear solver (R, C, L, V, I, E, G only) — use run_ngspice for diodes, transistors, switches and models",
                first.to_ascii_uppercase()
            ));
        }
        let p = out.circuit.node(need(1)?);
        let q = out.circuit.node(need(2)?);
        let ic_val = || {
            toks.iter()
                .position(|t| t.eq_ignore_ascii_case("ic"))
                .and_then(|i| toks.get(i + 2))
                .and_then(|v| number(v))
        };
        let kind = match first {
            'r' => {
                let r = value(3)?;
                if r <= 0.0 {
                    return Err(format!("{name}: resistance must be positive"));
                }
                Kind::R(r)
            }
            'c' => Kind::C {
                c: value(3)?,
                ic: ic_val(),
            },
            'l' => Kind::L {
                l: value(3)?,
                ic: ic_val(),
            },
            'v' => Kind::V(source(&toks[3..], &name)?),
            'i' => Kind::I(source(&toks[3..], &name)?),
            'e' | 'g' => {
                let cp = out.circuit.node(need(3)?);
                let cn = out.circuit.node(need(4)?);
                let k = value(5)?;
                if first == 'e' {
                    Kind::E { gain: k, cp, cn }
                } else {
                    Kind::G { gm: k, cp, cn }
                }
            }
            _ => unreachable!(),
        };
        out.circuit.elements.push(Element {
            name,
            p,
            n: q,
            kind,
        });
    }
    if out.circuit.elements.is_empty() {
        return Err("netlist has no elements".into());
    }
    Ok(out)
}
