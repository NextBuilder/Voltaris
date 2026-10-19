//! One electrical model of a design — components with fields, and nets with
//! their pins — shared by the rule engine, change diffs and block diagrams.
//!
//! The authoritative source is KiCad's own netlist (`kicad-cli sch export
//! netlist`), which follows the whole sheet hierarchy. Without kicad-cli the
//! model is built from the given sheet alone and says so.

use crate::tools::placed_pins_by_reference;
use crate::tools::sch_connectivity::net_graph_for;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use voltaris_sexp::parser::{parse_sexp, SexpNode};
use voltaris_sexp::schematic::{
    extract_all_net_labels, extract_wires, pin_endpoint, read_schematic,
};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Component {
    pub reference: String,
    pub value: String,
    pub footprint: String,
    pub lib_id: String,
    pub fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Node {
    pub reference: String,
    pub pin: String,
    pub function: String,
    pub pin_type: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Design {
    pub components: BTreeMap<String, Component>,
    pub nets: BTreeMap<String, Vec<Node>>,
    pub source: String,
    pub hierarchical: bool,
    pub warnings: Vec<String>,
}

const POWER_HINTS: &[&str] = &[
    "GND", "VCC", "VDD", "VBUS", "VSYS", "VIN", "VBAT", "BAT+", "AGND", "DGND", "PGND", "VSS",
];

/// Net name without the sheet path KiCad prefixes to local nets (`/sensor/SDA` → `SDA`).
pub fn short_name(net: &str) -> &str {
    net.rsplit('/').next().unwrap_or(net)
}

pub fn is_power_net(net: &str) -> bool {
    let n = short_name(net).to_ascii_uppercase();
    let rail_like = |t: &str| {
        let t = t.strip_prefix('V').unwrap_or(t);
        !t.is_empty()
            && t.chars().next().is_some_and(|c| c.is_ascii_digit())
            && t.contains('V')
            && t.chars()
                .all(|c| c.is_ascii_digit() || c == 'V' || c == '.')
    };
    n.starts_with('+')
        || POWER_HINTS.contains(&n.as_str())
        || n.starts_with("VCC_")
        || n.starts_with("VDD_")
        || rail_like(&n)
        || n.split_once('_').is_some_and(|(head, tail)| {
            POWER_HINTS.contains(&head)
                && [
                    "RAW", "IN", "OUT", "PROT", "FUSED", "FILT", "MAIN", "SYS", "EXT", "SW", "USB",
                    "BAT",
                ]
                .contains(&tail)
        })
}

pub fn is_unconnected(net: &str) -> bool {
    net.starts_with("unconnected-")
}

pub fn is_virtual(reference: &str) -> bool {
    reference.starts_with('#')
}

/// Case-insensitive glob with `*`, `?` and `|` alternatives, matched against
/// the full name and against the name without its sheet path.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn one(p: &[u8], t: &[u8]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some(b'*'), _) => one(&p[1..], t) || (!t.is_empty() && one(p, &t[1..])),
            (Some(b'?'), Some(_)) => one(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => one(&p[1..], &t[1..]),
            _ => false,
        }
    }
    pattern
        .split('|')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .any(|p| {
            one(p.as_bytes(), text.as_bytes()) || one(p.as_bytes(), short_name(text).as_bytes())
        })
}

fn atom<'a>(node: &'a SexpNode, tag: &str) -> Option<&'a str> {
    node.find(tag)
        .and_then(|n| n.get(1))
        .and_then(|a| a.as_str())
}

/// Parse a KiCad `kicadsexpr` netlist (`(export (version "E") …)`).
pub fn parse_kicad_netlist(text: &str) -> Result<Design, String> {
    let tree = parse_sexp(text).map_err(|e| format!("netlist does not parse: {e:?}"))?;
    let mut d = Design {
        source: "kicad-cli netlist (full hierarchy)".into(),
        hierarchical: true,
        ..Default::default()
    };
    if let Some(comps) = tree.find("components") {
        for c in comps.find_all("comp") {
            let Some(reference) = atom(c, "ref") else {
                continue;
            };
            let mut fields = BTreeMap::new();
            if let Some(fs) = c.find("fields") {
                for f in fs.find_all("field") {
                    if let Some(name) = atom(f, "name") {
                        let value = f.get(2).and_then(|v| v.as_str()).unwrap_or("").to_string();
                        fields.insert(name.to_string(), value);
                    }
                }
            }
            let lib_id = c
                .find("libsource")
                .map(|l| {
                    format!(
                        "{}:{}",
                        atom(l, "lib").unwrap_or(""),
                        atom(l, "part").unwrap_or("")
                    )
                })
                .unwrap_or_default();
            d.components.insert(
                reference.to_string(),
                Component {
                    reference: reference.to_string(),
                    value: atom(c, "value").unwrap_or("").to_string(),
                    footprint: atom(c, "footprint").unwrap_or("").to_string(),
                    lib_id,
                    fields,
                },
            );
        }
    }
    if let Some(nets) = tree.find("nets") {
        for n in nets.find_all("net") {
            let Some(name) = atom(n, "name") else {
                continue;
            };
            let nodes = n
                .find_all("node")
                .iter()
                .filter_map(|node| {
                    Some(Node {
                        reference: atom(node, "ref")?.to_string(),
                        pin: atom(node, "pin")?.to_string(),
                        function: atom(node, "pinfunction").unwrap_or("").to_string(),
                        pin_type: atom(node, "pintype").unwrap_or("").to_string(),
                    })
                })
                .collect();
            d.nets.insert(name.to_string(), nodes);
        }
    }
    if d.components.is_empty() {
        return Err("netlist has no components".into());
    }
    Ok(d)
}

fn symbol_fields(tree: &SexpNode) -> HashMap<String, BTreeMap<String, String>> {
    let mut out = HashMap::new();
    for s in tree.find_all("symbol") {
        if s.find("lib_id").is_none() {
            continue;
        }
        let mut fields = BTreeMap::new();
        let mut reference = None;
        for p in s.find_all("property") {
            let (Some(k), Some(v)) = (
                p.get(1).and_then(|n| n.as_str()),
                p.get(2).and_then(|n| n.as_str()),
            ) else {
                continue;
            };
            if k == "Reference" {
                reference = Some(v.to_string());
            }
            fields.insert(k.to_string(), v.to_string());
        }
        if let Some(r) = reference {
            out.entry(r).or_insert(fields);
        }
    }
    out
}

/// Build the model from one sheet file (no hierarchy traversal).
pub fn from_sheet(path: &Path) -> anyhow::Result<Design> {
    let (_, tree) = read_schematic(path)?;
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let mut graph = net_graph_for(&tree, &wires, &labels);
    let fields = symbol_fields(&tree);
    let mut d = Design {
        source: "single-sheet parse (kicad-cli unavailable)".into(),
        ..Default::default()
    };
    d.hierarchical = !tree.find_all("sheet").is_empty();
    if d.hierarchical {
        d.warnings.push("This sheet contains sub-sheets that were not followed: install or configure kicad-cli for a full-hierarchy model".into());
    }
    let mut roots: BTreeMap<(i64, i64), Vec<Node>> = BTreeMap::new();
    for (inst, pins) in placed_pins_by_reference(&tree) {
        if is_virtual(&inst.reference) {
            continue;
        }
        let f = fields.get(&inst.reference).cloned().unwrap_or_default();
        d.components
            .entry(inst.reference.clone())
            .or_insert_with(|| Component {
                reference: inst.reference.clone(),
                value: inst.value.clone(),
                footprint: inst.footprint.clone(),
                lib_id: inst.lib_id.clone(),
                fields: f,
            });
        for (pin, t) in pins {
            let (x, y) = pin_endpoint(&pin, t);
            let root = graph.root_at(x, y);
            roots.entry(root).or_default().push(Node {
                reference: inst.reference.clone(),
                pin: pin.number.clone(),
                function: pin.name.clone(),
                pin_type: String::new(),
            });
        }
    }
    for (root, mut nodes) in roots {
        nodes.sort();
        nodes.dedup();
        let name = match graph.name_of_root(root) {
            Some(n) => n,
            None if nodes.len() == 1 => {
                format!("unconnected-({}-Pad{})", nodes[0].reference, nodes[0].pin)
            }
            None => format!("Net-({}-Pad{})", nodes[0].reference, nodes[0].pin),
        };
        d.nets.entry(name).or_default().extend(nodes);
    }
    Ok(d)
}

/// Full-hierarchy model via kicad-cli when available, else the single sheet.
pub async fn load(schematic: &Path, kicad_cli: &str) -> anyhow::Result<Design> {
    if crate::kicad_install::find_cli(kicad_cli).is_some() {
        let tmp = std::env::temp_dir().join(format!(
            "kcp-netlist-{}-{}.net",
            std::process::id(),
            schematic
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("design")
        ));
        let result =
            crate::tools::cli::export_netlist(kicad_cli, schematic, &tmp, "kicadsexpr").await;
        if result.is_ok() {
            if let Ok(text) = std::fs::read_to_string(&tmp) {
                let _ = std::fs::remove_file(&tmp);
                if let Ok(d) = parse_kicad_netlist(&text) {
                    return Ok(d);
                }
            }
        }
        let mut d = from_sheet(schematic)?;
        d.warnings.push("kicad-cli was found but its netlist export failed; fell back to the single-sheet parse".into());
        return Ok(d);
    }
    from_sheet(schematic)
}

impl Design {
    /// Nets each component pin sits on, by reference.
    pub fn nets_of(&self, reference: &str) -> Vec<(&str, &Node)> {
        self.nets
            .iter()
            .flat_map(|(n, nodes)| {
                nodes
                    .iter()
                    .filter(|x| x.reference == reference)
                    .map(move |x| (n.as_str(), x))
            })
            .collect()
    }

    pub fn prefix(reference: &str) -> String {
        reference
            .chars()
            .take_while(|c| c.is_ascii_alphabetic())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_kicad10_netlist_parses() {
        let text = include_str!("../../tests/fixtures/unassigned_footprint.net");
        let d = parse_kicad_netlist(text).unwrap();
        assert!(d.components.contains_key("C1") && d.components.contains_key("R1"));
        assert_eq!(
            d.components["C1"].footprint,
            "Capacitor_SMD:C_0603_1608Metric"
        );
        assert_eq!(d.components["C1"].lib_id, "Device:C");
        assert_eq!(
            d.components["C1"]
                .fields
                .get("Description")
                .map(String::as_str),
            Some("Unpolarized capacitor")
        );
        let n = &d.nets["Net-(C1-Pad1)"];
        assert!(n.iter().any(|x| x.reference == "R1" && x.pin == "1"));
        assert!(is_unconnected("unconnected-(C1-Pad2)"));
    }

    #[test]
    fn globs_and_power_names() {
        assert!(glob_match("SDA|*_SDA", "/sensor/I2C_SDA"));
        assert!(glob_match("usb_d*", "USB_DP"));
        assert!(!glob_match("ADC*", "VBAT_SENSE"));
        assert!(glob_match("?DA", "SDA"));
        for p in [
            "+3V3",
            "GND",
            "VBUS",
            "+5V",
            "/power/VCC",
            "V3V3",
            "AGND",
            "VBUS_RAW",
            "VIN_PROT",
            "VBAT_SW",
        ] {
            assert!(is_power_net(p), "{p}");
        }
        for s in [
            "SDA",
            "USB_DP",
            "EN",
            "VBAT_SENSE",
            "LED_DATA",
            "VBUS_DET",
            "VIN_MON",
        ] {
            assert!(!is_power_net(s), "{s}");
        }
    }
}
