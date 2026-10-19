//! `block_diagram` toolset — a system block diagram drawn from the real
//! netlist: ICs, modules and connectors as blocks, signal nets grouped into
//! buses (I2C, SPI, UART, USB, SWD, I2S, CAN, analog, GPIO) and the power tree
//! from sources through regulators to loads, as Mermaid.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::design_model::{self, is_power_net, is_unconnected, short_name, Design};
use crate::tools::{invalid_arg, opt_str, require_str, ToolContext, ToolDef};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::PathBuf;

const BLOCK_PREFIXES: &[&str] = &[
    "U", "IC", "J", "P", "CN", "USB", "BT", "K", "M", "MOD", "Y", "X", "LS", "BZ", "DS", "LCD",
    "AE", "ANT", "SW", "S",
];
const PASSIVE_PREFIXES: &[&str] = &["R", "L", "FB", "RN", "JP", "NT"];

fn ground(net: &str) -> bool {
    let n = short_name(net).to_ascii_uppercase();
    n.starts_with("GND") || n.ends_with("GND") || n == "VSS" || n == "0"
}

fn is_block(d: &Design, r: &str) -> bool {
    let p = Design::prefix(r).to_ascii_uppercase();
    if BLOCK_PREFIXES.contains(&p.as_str()) {
        return true;
    }
    let c = &d.components[r];
    p == "D" && {
        let s = format!("{} {}", c.lib_id, c.value).to_ascii_uppercase();
        s.contains("WS28") || s.contains("SK68") || s.contains("APA102")
    }
}

fn kind(d: &Design, r: &str) -> &'static str {
    let p = Design::prefix(r).to_ascii_uppercase();
    let c = &d.components[r];
    let s = format!("{} {}", c.lib_id, c.value).to_ascii_uppercase();
    match p.as_str() {
        "J" | "P" | "CN" | "USB" => "connector",
        "SW" | "S" => "input",
        "Y" | "X" => "clock",
        "BT" => "battery",
        "AE" | "ANT" => "antenna",
        _ if [
            "REGULATOR",
            "LDO",
            "BUCK",
            "BOOST",
            "AMS1117",
            "AP2112",
            "TPS5",
            "TPS6",
            "MP1",
            "XC62",
            "RT9",
            "ME6211",
            "HT7",
            "LM1117",
            "MCP1700",
            "TLV7",
        ]
        .iter()
        .any(|k| s.contains(k)) =>
        {
            "power"
        }
        _ if [
            "TP4056", "MCP7383", "BQ2", "CHARGER", "DW01", "FS8205", "BQ29", "IP5306",
        ]
        .iter()
        .any(|k| s.contains(k)) =>
        {
            "power"
        }
        _ if [
            "ESP32", "STM32", "RP2040", "RP2350", "NRF5", "ATMEGA", "ATSAM", "GD32", "CH32", "PIC",
            "MCU", "WROOM", "MINI-1",
        ]
        .iter()
        .any(|k| s.contains(k)) =>
        {
            "mcu"
        }
        _ => "ic",
    }
}

pub fn bus_of(net: &str, functions: &[String]) -> &'static str {
    let mut names = vec![short_name(net).to_ascii_uppercase()];
    names.extend(functions.iter().map(|f| f.to_ascii_uppercase()));
    let any = |keys: &[&str]| {
        names.iter().any(|n| {
            keys.iter().any(|k| {
                n == k
                    || n.ends_with(&format!("_{k}"))
                    || n.starts_with(&format!("{k}_"))
                    || n.contains(&format!("_{k}_"))
            })
        })
    };
    let has = |keys: &[&str]| names.iter().any(|n| keys.iter().any(|k| n.contains(k)));
    if has(&["USB_D", "USB_DP", "USB_DN", "USBDP", "USBDM"]) || any(&["D+", "D-", "DP", "DN", "DM"])
    {
        "USB"
    } else if any(&["SDA", "SCL", "I2C_SDA", "I2C_SCL"]) || has(&["I2C", "IIC", "TWI"]) {
        "I2C"
    } else if any(&[
        "MOSI", "MISO", "SCK", "SCLK", "COPI", "CIPO", "SDO", "SDI", "CS", "NSS", "SS",
    ]) || has(&["SPI"])
    {
        "SPI"
    } else if any(&[
        "TX", "RX", "TXD", "RXD", "TXD0", "RXD0", "U0TXD", "U0RXD", "RTS", "CTS",
    ]) || has(&["UART"])
    {
        "UART"
    } else if any(&["SWDIO", "SWCLK", "SWO", "TMS", "TCK", "TDI", "TDO"]) || has(&["SWD", "JTAG"]) {
        "SWD/JTAG"
    } else if any(&["BCLK", "LRCK", "LRCLK", "WS", "MCLK", "DIN", "DOUT"]) || has(&["I2S"]) {
        "I2S"
    } else if any(&["CANH", "CANL", "CAN_H", "CAN_L", "CAN_TX", "CAN_RX"]) {
        "CAN"
    } else if has(&["ANT", "RF_", "LNA"]) || any(&["RF"]) {
        "RF"
    } else if has(&["ADC", "AIN", "SENSE", "_MON", "VBAT_MEAS"]) {
        "analog"
    } else if has(&["XTAL", "XIN", "XOUT", "OSC", "XI", "XO", "XL1", "XL2"]) {
        "clock"
    } else if has(&["EN", "RST", "RESET", "BOOT", "NRST", "RUN"])
        && any(&[
            "EN", "RST", "RESET", "BOOT", "NRST", "RUN", "CHIP_EN", "CHIP_PU", "BOOT0",
        ])
    {
        "control"
    } else {
        "GPIO"
    }
}

/// Blocks reachable from a net through series passives (two-pin parts that do
/// not go to a supply or ground), up to two hops.
fn blocks_on(d: &Design, net: &str) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut seen: BTreeSet<String> = BTreeSet::from([net.to_string()]);
    let mut q = VecDeque::from([(net.to_string(), 0)]);
    while let Some((n, depth)) = q.pop_front() {
        for node in d.nets.get(&n).into_iter().flatten() {
            let r = &node.reference;
            if !d.components.contains_key(r) {
                continue;
            }
            if is_block(d, r) {
                out.entry(r.clone())
                    .or_default()
                    .push(if node.function.is_empty() {
                        node.pin.clone()
                    } else {
                        node.function.clone()
                    });
            } else if depth < 2
                && PASSIVE_PREFIXES.contains(&Design::prefix(r).to_ascii_uppercase().as_str())
            {
                let pins = d.nets_of(r);
                if pins.len() == 2 {
                    for (other, x) in pins {
                        if x.pin != node.pin
                            && !is_power_net(other)
                            && !ground(other)
                            && seen.insert(other.to_string())
                        {
                            q.push_back((other.to_string(), depth + 1));
                        }
                    }
                }
            }
        }
    }
    out
}

type Rails = BTreeMap<String, (BTreeSet<String>, BTreeSet<String>, BTreeSet<String>)>;

fn id(r: &str) -> String {
    r.chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect()
}

fn esc(s: &str) -> String {
    s.replace('"', "'").replace(['<', '>'], "")
}

pub struct Diagram {
    pub mermaid: String,
    pub summary: Value,
}

pub fn build(d: &Design, direction: &str, include_gpio: bool) -> Diagram {
    let blocks: Vec<&String> = d
        .components
        .keys()
        .filter(|r| !design_model::is_virtual(r) && is_block(d, r))
        .collect();
    let mut links: BTreeMap<(String, String, &'static str), BTreeSet<String>> = BTreeMap::new();
    let mut rails: Rails = BTreeMap::new();
    for (net, nodes) in &d.nets {
        if is_unconnected(net) || ground(net) {
            continue;
        }
        if is_power_net(net) {
            let rail = rails.entry(short_name(net).to_string()).or_default();
            for n in nodes {
                if !d.components.contains_key(&n.reference) {
                    continue;
                }
                let f = n.function.to_ascii_uppercase();
                let block = is_block(d, &n.reference);
                if block
                    && (n.pin_type == "power_out"
                        || ["VOUT", "OUT", "VO", "BAT", "VSYS", "OUTPUT"].contains(&f.as_str()))
                {
                    rail.0.insert(n.reference.clone());
                } else if block && matches!(kind(d, &n.reference), "connector" | "battery") {
                    rail.2.insert(n.reference.clone());
                } else if block {
                    rail.1.insert(n.reference.clone());
                } else if Design::prefix(&n.reference).eq_ignore_ascii_case("L") {
                    for (other, x) in d.nets_of(&n.reference) {
                        if x.pin != n.pin {
                            for o in d.nets.get(other).into_iter().flatten() {
                                if is_block(d, &o.reference)
                                    && o.function.to_ascii_uppercase().contains("SW")
                                {
                                    rail.0.insert(o.reference.clone());
                                }
                            }
                        }
                    }
                }
            }
            continue;
        }
        let functions: Vec<String> = nodes
            .iter()
            .map(|n| n.function.clone())
            .filter(|f| !f.is_empty())
            .collect();
        let bus = bus_of(net, &functions);
        if bus == "GPIO" && !include_gpio {
            continue;
        }
        let on = blocks_on(d, net);
        let refs: Vec<&String> = on.keys().collect();
        for i in 0..refs.len() {
            for j in i + 1..refs.len() {
                let (a, b) = (refs[i].clone(), refs[j].clone());
                let label = if net.starts_with("Net-(") {
                    on[&a].first().cloned().unwrap_or_default()
                } else {
                    short_name(net).to_string()
                };
                links.entry((a, b, bus)).or_default().insert(label);
            }
        }
    }
    for rail in rails.values_mut() {
        let producers = rail.0.clone();
        rail.1.retain(|r| !producers.contains(r));
        rail.2.retain(|r| !producers.contains(r));
    }
    let mut series: BTreeSet<(String, String, String)> = BTreeSet::new();
    for (r, c) in &d.components {
        let p = Design::prefix(r).to_ascii_uppercase();
        if !["F", "FB", "L", "R", "D", "Q", "JP"].contains(&p.as_str()) {
            continue;
        }
        let pins = d.nets_of(r);
        let nets: BTreeSet<&str> = pins
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| is_power_net(n) && !ground(n))
            .collect();
        if nets.len() == 2
            && pins
                .iter()
                .all(|(n, _)| is_power_net(n) || is_unconnected(n) || p == "Q" || p == "D")
        {
            let v: Vec<&str> = nets.into_iter().collect();
            let what = match p.as_str() {
                "F" => "fuse",
                "FB" | "L" => "filter",
                "D" => "diode",
                "Q" => "switch",
                "JP" => "jumper",
                _ => "resistor",
            };
            let label = if c.value.is_empty() {
                format!("{r} {what}")
            } else {
                format!("{r} {what} {}", c.value)
            };
            series.insert((
                short_name(v[0]).to_string(),
                short_name(v[1]).to_string(),
                label,
            ));
        }
    }
    for (a, b, _) in &series {
        rails.entry(a.clone()).or_default();
        rails.entry(b.clone()).or_default();
    }
    let linked: BTreeSet<&String> = series.iter().flat_map(|(a, b, _)| [a, b]).collect();
    rails.retain(|name, (p, c, s)| {
        !(p.is_empty() && c.is_empty() && s.is_empty()) || linked.contains(name)
    });
    let fed = |name: &String, rails: &Rails| {
        let mut seen = BTreeSet::from([name.clone()]);
        let mut q = vec![name.clone()];
        while let Some(n) = q.pop() {
            if rails
                .get(&n)
                .is_some_and(|(p, _, s)| !p.is_empty() || !s.is_empty())
            {
                return true;
            }
            for (a, b, _) in &series {
                for (x, y) in [(a, b), (b, a)] {
                    if *x == n && seen.insert(y.clone()) {
                        q.push(y.clone());
                    }
                }
            }
        }
        false
    };
    let unsourced: Vec<String> = rails
        .iter()
        .filter(|(n, (_, c, _))| !c.is_empty() && !fed(n, &rails))
        .map(|(n, _)| n.clone())
        .collect();

    let mut m = format!("flowchart {direction}\n");
    let groups: [(&str, &str); 7] = [
        ("connector", "Connectors"),
        ("power", "Power"),
        ("mcu", "Processing"),
        ("ic", "Peripherals"),
        ("input", "User inputs"),
        ("clock", "Clocks"),
        ("battery", "Power"),
    ];
    let mut by_group: BTreeMap<&str, Vec<&String>> = BTreeMap::new();
    for b in &blocks {
        let k = kind(d, b);
        let g = groups
            .iter()
            .find(|(x, _)| *x == k)
            .map(|(_, g)| *g)
            .unwrap_or("Peripherals");
        by_group.entry(g).or_default().push(b);
    }
    for (g, members) in &by_group {
        m.push_str(&format!("  subgraph {}[\"{}\"]\n", id(g), g));
        for b in members {
            let c = &d.components[*b];
            let shape = match kind(d, b) {
                "connector" => ("[/", "/]"),
                "mcu" => ("[[", "]]"),
                "battery" => ("[(", ")]"),
                _ => ("[", "]"),
            };
            m.push_str(&format!(
                "    {}{}\"{}<br/>{}\"{}\n",
                id(b),
                shape.0,
                esc(b),
                esc(&c.value),
                shape.1
            ));
        }
        m.push_str("  end\n");
    }
    for (rail, (producers, consumers, sources)) in &rails {
        let rid = format!("rail_{}", id(rail));
        m.push_str(&format!("  {rid}([\"{}\"])\n", esc(rail)));
        for p in producers {
            m.push_str(&format!("  {} ==> {rid}\n", id(p)));
        }
        for s in sources {
            m.push_str(&format!("  {} === {rid}\n", id(s)));
        }
        for c in consumers {
            m.push_str(&format!("  {rid} -.-> {}\n", id(c)));
        }
    }
    for (a, b, label) in &series {
        m.push_str(&format!(
            "  rail_{} ---|\"{}\"| rail_{}\n",
            id(a),
            esc(label),
            id(b)
        ));
    }
    let mut buses = Vec::new();
    for ((a, b, bus), nets) in &links {
        let list: Vec<&String> = nets.iter().collect();
        let label = if *bus == "GPIO" || *bus == "control" || *bus == "analog" {
            let shown: Vec<&str> = list.iter().take(3).map(|s| s.as_str()).collect();
            format!(
                "{bus}: {}{}",
                shown.join(", "),
                if list.len() > 3 {
                    format!(" +{}", list.len() - 3)
                } else {
                    String::new()
                }
            )
        } else {
            bus.to_string()
        };
        m.push_str(&format!(
            "  {} <-->|\"{}\"| {}\n",
            id(a),
            esc(&label),
            id(b)
        ));
        buses.push(json!({ "between": [a, b], "bus": bus, "nets": list }));
    }
    m.push_str("  classDef power fill:#fff3cd,stroke:#b8860b\n  classDef mcu fill:#dbeafe,stroke:#1d4ed8\n  classDef conn fill:#e5e7eb,stroke:#374151\n");
    for b in &blocks {
        let cls = match kind(d, b) {
            "power" | "battery" => "power",
            "mcu" => "mcu",
            "connector" => "conn",
            _ => continue,
        };
        m.push_str(&format!("  class {} {cls}\n", id(b)));
    }
    let summary = json!({
        "blocks": blocks.iter().map(|b| json!({ "reference": b, "value": d.components[*b].value, "kind": kind(d, b) })).collect::<Vec<_>>(),
        "buses": buses,
        "power_rails": rails.iter().map(|(r, (p, c, s))| json!({ "rail": r, "produced_by": p, "fed_from": s, "powers": c })).collect::<Vec<_>>(),
        "rail_links": series.iter().map(|(a, b, l)| json!({ "between": [a, b], "through": l })).collect::<Vec<_>>(),
        "rails_without_source": unsourced,
    });
    Diagram {
        mermaid: m,
        summary,
    }
}

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "generate_block_diagram",
        "Draw the system block diagram from the real netlist as Mermaid: ICs, modules and \
         connectors as blocks (grouped as connectors, power, processing, peripherals), signal \
         nets merged into labelled buses (USB, I2C, SPI, UART, SWD/JTAG, I2S, CAN, RF, analog, \
         control, GPIO) — followed through series resistors and ferrites — and the power tree \
         (thick arrow: regulator output to rail; plain line: connector or battery feeding a rail; \
         dotted arrow: rail to the parts it powers). Writes a .md (Mermaid fence) or .mmd file \
         when 'output' is given and always returns the diagram text and a structured summary. \
         Use it to check the architecture matches the brief.",
        json!({
            "type": "object",
            "properties": {
                "schematic": { "type": "string", "description": "Root .kicad_sch" },
                "output": { "type": "string", "description": "Path ending in .md or .mmd" },
                "direction": { "type": "string", "enum": ["LR", "TB"], "default": "LR" },
                "include_gpio": { "type": "boolean", "default": true, "description": "Draw plain GPIO links too" }
            },
            "required": ["schematic"]
        }),
        |args, ctx| async move { handle_generate(args, ctx).await }
    )]
}

async fn handle_generate(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let sch = PathBuf::from(match require_str(args, "schematic") {
        Ok(s) => s,
        Err(e) => return Ok(e),
    });
    if !sch.is_file() {
        return Ok(invalid_arg("schematic", "file does not exist"));
    }
    let design = design_model::load(&sch, &ctx.config.kicad_cli).await?;
    let direction = opt_str(args, "direction").unwrap_or("LR");
    let diagram = build(
        &design,
        direction,
        args["include_gpio"].as_bool().unwrap_or(true),
    );
    let mut written = None;
    if let Some(out) = opt_str(args, "output") {
        let path = PathBuf::from(out);
        let body = match path.extension().and_then(|e| e.to_str()) {
            Some("mmd") => diagram.mermaid.clone(),
            Some("md") => format!(
                "# Block diagram — {}\n\n```mermaid\n{}```\n",
                sch.file_stem().and_then(|s| s.to_str()).unwrap_or("design"),
                diagram.mermaid
            ),
            _ => return Ok(invalid_arg("output", "must end in .md or .mmd")),
        };
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, body)?;
        written = Some(path.display().to_string());
    }
    Ok(CallToolResult::json(&json!({
        "mermaid": diagram.mermaid,
        "summary": diagram.summary,
        "written": written,
        "design_source": design.source,
        "design_warnings": design.warnings,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::design_model::{Component, Node};

    #[test]
    fn buses_power_tree_and_series_resistors() {
        let mut d = Design::default();
        for (r, v, lib) in [
            ("U1", "ESP32-S3-WROOM-1", "RF_Module:ESP32-S3-WROOM-1"),
            ("U2", "AP2112K-3.3", "Regulator_Linear:AP2112K-3.3"),
            ("U3", "SHT30", "Sensor:SHT30"),
            ("J1", "USB_C", "Connector:USB_C"),
            ("R5", "22", "Device:R"),
            ("R1", "4.7k", "Device:R"),
            ("C1", "1u", "Device:C"),
        ] {
            d.components.insert(
                r.into(),
                Component {
                    reference: r.into(),
                    value: v.into(),
                    lib_id: lib.into(),
                    ..Default::default()
                },
            );
        }
        let n = |r: &str, p: &str, f: &str, t: &str| Node {
            reference: r.into(),
            pin: p.into(),
            function: f.into(),
            pin_type: t.into(),
        };
        d.nets.insert(
            "VBUS".into(),
            vec![
                n("J1", "A4", "VBUS", "passive"),
                n("U2", "1", "VIN", "power_in"),
            ],
        );
        d.nets.insert(
            "+3V3".into(),
            vec![
                n("U2", "5", "VOUT", "power_out"),
                n("U1", "2", "3V3", "power_in"),
                n("U3", "5", "VDD", "power_in"),
                n("R1", "2", "", "passive"),
                n("C1", "1", "", "passive"),
            ],
        );
        d.nets.insert(
            "GND".into(),
            vec![n("U1", "1", "GND", "power_in"), n("C1", "2", "", "passive")],
        );
        d.nets.insert(
            "/SDA".into(),
            vec![
                n("U1", "12", "IO8", "bidirectional"),
                n("U3", "1", "SDA", "bidirectional"),
                n("R1", "1", "", "passive"),
            ],
        );
        d.nets.insert(
            "Net-(J1-DP)".into(),
            vec![
                n("J1", "A6", "D+", "bidirectional"),
                n("R5", "1", "", "passive"),
            ],
        );
        d.nets.insert(
            "/USB_DP".into(),
            vec![
                n("R5", "2", "", "passive"),
                n("U1", "14", "USB_D+", "bidirectional"),
            ],
        );
        let g = build(&d, "LR", true);
        let s = &g.summary;
        let bus = |a: &str, b: &str| {
            s["buses"]
                .as_array()
                .unwrap()
                .iter()
                .find(|x| x["between"] == json!([a, b]))
                .map(|x| x["bus"].clone())
        };
        assert_eq!(bus("U1", "U3"), Some(json!("I2C")));
        assert_eq!(bus("J1", "U1"), Some(json!("USB")), "{s}");
        let rail = s["power_rails"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rail"] == "+3V3")
            .unwrap();
        assert_eq!(rail["produced_by"], json!(["U2"]));
        assert_eq!(rail["powers"], json!(["U1", "U3"]));
        let vbus = s["power_rails"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rail"] == "VBUS")
            .unwrap();
        assert_eq!(vbus["fed_from"], json!(["J1"]));
        assert!(g.mermaid.starts_with("flowchart LR\n"));
        assert!(
            g.mermaid.contains("U2 ==> rail__3V3") && g.mermaid.contains("U1 <-->|\"I2C\"| U3"),
            "{}",
            g.mermaid
        );
        assert_eq!(kind(&d, "U2"), "power");
        assert_eq!(kind(&d, "U1"), "mcu");
    }

    #[test]
    fn bus_names() {
        assert_eq!(bus_of("SPI_MOSI", &[]), "SPI");
        assert_eq!(bus_of("Net-(U1-Pad5)", &["U0TXD".into()]), "UART");
        assert_eq!(bus_of("VBAT_SENSE", &[]), "analog");
        assert_eq!(bus_of("LED_DATA", &[]), "GPIO");
        assert_eq!(bus_of("SWDIO", &[]), "SWD/JTAG");
        assert_eq!(bus_of("CHIP_EN", &[]), "control");
        assert_eq!(bus_of("/BTN_USER", &[]), "GPIO");
    }
}
