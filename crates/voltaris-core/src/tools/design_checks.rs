//! `design_checks` toolset — company/project design rules written once in
//! plain English, stored as structured checks, and evaluated mechanically on
//! every run against the netlist and the board.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::design_model::{
    self, glob_match, is_power_net, is_unconnected, is_virtual, Design,
};
use crate::tools::{invalid_arg, opt_str, require_str, ToolContext, ToolDef};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct CompSel {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ref_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lib_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub footprint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude_refs: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Select {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exclude: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub touches: Option<CompSel>,
    #[serde(default)]
    pub include_power: bool,
    #[serde(default, flatten)]
    pub component: CompSel,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Connects {
    #[serde(default, flatten)]
    pub component: CompSel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other_pin_net: Option<String>,
    #[serde(default = "one")]
    pub min: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
}

fn one() -> usize {
    1
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct PinNet {
    pub pin: String,
    pub net: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Require {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connects: Option<Connects>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_connects: Option<CompSel>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_nodes: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_nodes: Option<usize>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fields: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub footprint: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pin_net: Option<PinNet>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connects_net: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub power_pins_decoupled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_track_width_mm: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default)]
    pub id: String,
    pub description: String,
    pub scope: String,
    #[serde(default)]
    pub select: Select,
    pub require: Require,
    #[serde(default = "warning")]
    pub severity: String,
}

fn warning() -> String {
    "warning".into()
}

fn ground_net(net: &str) -> bool {
    let n = design_model::short_name(net).to_ascii_uppercase();
    n == "GND"
        || n.starts_with("GND")
        || n == "VSS"
        || n == "AGND"
        || n == "DGND"
        || n == "PGND"
        || n == "GNDA"
        || n == "GNDD"
        || n == "GNDPWR"
        || n.ends_with("_GND")
}

fn net_pattern(pattern: &str, net: &str) -> bool {
    match pattern {
        "@power" => is_power_net(net) && !ground_net(net),
        "@ground" => ground_net(net),
        "@any_power" => is_power_net(net),
        p => glob_match(p, net),
    }
}

fn comp_matches(sel: &CompSel, d: &Design, reference: &str) -> bool {
    let Some(c) = d.components.get(reference) else {
        return false;
    };
    let prefix = Design::prefix(reference);
    sel.ref_prefix
        .as_ref()
        .is_none_or(|p| p.split('|').any(|x| x.trim().eq_ignore_ascii_case(&prefix)))
        && sel.value.as_ref().is_none_or(|p| glob_match(p, &c.value))
        && sel.lib_id.as_ref().is_none_or(|p| glob_match(p, &c.lib_id))
        && sel
            .footprint
            .as_ref()
            .is_none_or(|p| glob_match(p, &c.footprint))
        && sel
            .exclude_refs
            .as_ref()
            .is_none_or(|p| !glob_match(p, reference))
}

pub fn validate(rule: &Rule) -> Result<(), String> {
    if rule.description.trim().is_empty() {
        return Err("description is required — it is the plain-English rule".into());
    }
    if !["error", "warning"].contains(&rule.severity.as_str()) {
        return Err("severity must be 'error' or 'warning'".into());
    }
    let r = &rule.require;
    let net_reqs = r.connects.is_some()
        || r.not_connects.is_some()
        || r.min_nodes.is_some()
        || r.max_nodes.is_some();
    let comp_reqs = !r.fields.is_empty()
        || r.footprint
        || r.value.is_some()
        || r.pin_net.is_some()
        || r.connects_net.is_some()
        || r.power_pins_decoupled;
    let board_reqs = r.min_track_width_mm.is_some();
    match rule.scope.as_str() {
        "nets" if net_reqs && !comp_reqs && !board_reqs => Ok(()),
        "components" if comp_reqs && !net_reqs && !board_reqs => Ok(()),
        "board_nets" if board_reqs && !net_reqs && !comp_reqs => Ok(()),
        "nets" | "components" | "board_nets" => Err(format!(
            "scope '{}' needs its own requirements: nets → connects/not_connects/min_nodes/max_nodes; components → fields/footprint/value/pin_net/connects_net/power_pins_decoupled; board_nets → min_track_width_mm",
            rule.scope
        )),
        _ => Err("scope must be 'nets', 'components' or 'board_nets'".into()),
    }
}

pub fn slug(text: &str) -> String {
    let s: String = text
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    s.split('-')
        .filter(|p| !p.is_empty())
        .take(6)
        .collect::<Vec<_>>()
        .join("-")
}

pub fn builtin_rules() -> Vec<Rule> {
    let j = json!([
        {"id": "pro-footprints", "description": "Every part has a footprint assigned", "scope": "components", "severity": "error",
         "select": {"exclude_refs": "#*"}, "require": {"footprint": true}},
        {"id": "pro-i2c-pullups", "description": "Every I2C line has exactly one pull-up resistor to a supply", "scope": "nets",
         "select": {"name": "SDA|SCL|*_SDA|*_SCL|SDA_*|SCL_*|*SDA?|*SCL?"}, "require": {"connects": {"ref_prefix": "R", "other_pin_net": "@power", "min": 1, "max": 1}}},
        {"id": "pro-usb-esd", "description": "USB data lines at a connector have ESD protection", "scope": "nets",
         "select": {"name": "*USB*D*|D+|D-|*_DP|*_DN|*_D+|*_D-", "touches": {"ref_prefix": "J|P|CN|USB"}},
         "require": {"connects": {"value": "*USBLC*|*ESD*|*TVS*|*PRTR*|*SP05*|*TPD*|*SRV05*|*PESD*"}}},
        {"id": "pro-adc-filter", "description": "Analog/ADC inputs have a filter capacitor to ground", "scope": "nets",
         "select": {"name": "*ADC*|*_SENSE|*AIN*|*VBAT_MEAS*|*_MON"}, "require": {"connects": {"ref_prefix": "C", "other_pin_net": "@ground"}}},
        {"id": "pro-reset-pullup", "description": "Reset/enable lines have a pull-up so they never float", "scope": "nets",
         "select": {"name": "EN|*RESET*|*RST*|RUN|CHIP_EN|CHIP_PU|*_EN"}, "require": {"connects": {"ref_prefix": "R", "other_pin_net": "@power"}}},
        {"id": "pro-no-dangling-nets", "description": "Every named signal net connects at least two pins", "scope": "nets",
         "select": {}, "require": {"min_nodes": 2}},
        {"id": "pro-ic-decoupling", "description": "Every IC supply pin net has a capacitor to ground", "scope": "components",
         "select": {"ref_prefix": "U|IC"}, "require": {"power_pins_decoupled": true}},
        {"id": "pro-lcsc", "description": "Every assembled part carries an LCSC part number for JLCPCB assembly", "scope": "components",
         "select": {"exclude_refs": "#*|TP*|H*|MH*|FID*|G*|NT*|LOGO*"}, "require": {"fields": ["LCSC"]}}
    ]);
    serde_json::from_value(j).expect("builtin rules are valid")
}

#[derive(Debug, Serialize)]
pub struct Violation {
    pub object: String,
    pub detail: String,
}

pub fn evaluate(
    rule: &Rule,
    d: &Design,
    board: Option<&voltaris_sexp::parser::SexpNode>,
) -> (usize, Vec<Violation>, Option<String>) {
    let mut out = Vec::new();
    let mut checked = 0;
    let r = &rule.require;
    match rule.scope.as_str() {
        "nets" => {
            for (net, nodes) in &d.nets {
                if is_unconnected(net) || net.starts_with("Net-(") && rule.select.name.is_some() {
                    continue;
                }
                if !rule.select.include_power && is_power_net(net) {
                    continue;
                }
                if net.starts_with("Net-(") && r.min_nodes.is_some() {
                    continue;
                }
                if rule
                    .select
                    .name
                    .as_ref()
                    .is_some_and(|p| !glob_match(p, net))
                    || rule
                        .select
                        .exclude
                        .as_ref()
                        .is_some_and(|p| glob_match(p, net))
                {
                    continue;
                }
                if let Some(t) = &rule.select.touches {
                    if !nodes.iter().any(|n| comp_matches(t, d, &n.reference)) {
                        continue;
                    }
                }
                checked += 1;
                if let Some(c) = &r.connects {
                    let refs: BTreeSet<&str> = nodes
                        .iter()
                        .filter(|n| comp_matches(&c.component, d, &n.reference))
                        .filter(|n| {
                            c.other_pin_net.as_ref().is_none_or(|p| {
                                d.nets_of(&n.reference).iter().any(|(other, node)| {
                                    node.pin != n.pin && *other != net && net_pattern(p, other)
                                })
                            })
                        })
                        .map(|n| n.reference.as_str())
                        .collect();
                    if refs.len() < c.min {
                        out.push(Violation {
                            object: net.clone(),
                            detail: format!(
                                "found {} matching part(s), needs at least {}",
                                refs.len(),
                                c.min
                            ),
                        });
                    } else if c.max.is_some_and(|m| refs.len() > m) {
                        out.push(Violation {
                            object: net.clone(),
                            detail: format!(
                                "found {} matching parts ({}), allows at most {}",
                                refs.len(),
                                refs.iter().copied().collect::<Vec<_>>().join(", "),
                                c.max.unwrap()
                            ),
                        });
                    }
                }
                if let Some(sel) = &r.not_connects {
                    let bad: Vec<&str> = nodes
                        .iter()
                        .filter(|n| comp_matches(sel, d, &n.reference))
                        .map(|n| n.reference.as_str())
                        .collect();
                    if !bad.is_empty() {
                        out.push(Violation {
                            object: net.clone(),
                            detail: format!("must not connect {}", bad.join(", ")),
                        });
                    }
                }
                if r.min_nodes.is_some_and(|m| nodes.len() < m) {
                    out.push(Violation {
                        object: net.clone(),
                        detail: format!("connects {} pin(s)", nodes.len()),
                    });
                }
                if r.max_nodes.is_some_and(|m| nodes.len() > m) {
                    out.push(Violation {
                        object: net.clone(),
                        detail: format!(
                            "connects {} pins, limit {}",
                            nodes.len(),
                            r.max_nodes.unwrap()
                        ),
                    });
                }
            }
        }
        "components" => {
            for (reference, c) in &d.components {
                if is_virtual(reference) || !comp_matches(&rule.select.component, d, reference) {
                    continue;
                }
                checked += 1;
                for f in &r.fields {
                    if c.fields
                        .get(f)
                        .is_none_or(|v| v.trim().is_empty() || v.trim() == "~")
                    {
                        out.push(Violation {
                            object: reference.clone(),
                            detail: format!("field '{f}' is missing or empty"),
                        });
                    }
                }
                if r.footprint && c.footprint.trim().is_empty() {
                    out.push(Violation {
                        object: reference.clone(),
                        detail: "no footprint assigned".into(),
                    });
                }
                if let Some(p) = &r.value {
                    if !glob_match(p, &c.value) {
                        out.push(Violation {
                            object: reference.clone(),
                            detail: format!("value '{}' does not match '{p}'", c.value),
                        });
                    }
                }
                let pins = d.nets_of(reference);
                if let Some(pn) = &r.pin_net {
                    let matching: Vec<_> = pins
                        .iter()
                        .filter(|(_, n)| {
                            glob_match(&pn.pin, &n.pin) || glob_match(&pn.pin, &n.function)
                        })
                        .collect();
                    if matching.is_empty() {
                        out.push(Violation {
                            object: reference.clone(),
                            detail: format!("no pin matches '{}'", pn.pin),
                        });
                    }
                    for (net, n) in matching {
                        if !net_pattern(&pn.net, net) {
                            out.push(Violation {
                                object: format!("{reference}.{}", n.pin),
                                detail: format!(
                                    "pin {} is on '{net}', expected '{}'",
                                    if n.function.is_empty() {
                                        &n.pin
                                    } else {
                                        &n.function
                                    },
                                    pn.net
                                ),
                            });
                        }
                    }
                }
                if let Some(p) = &r.connects_net {
                    if !pins.iter().any(|(net, _)| net_pattern(p, net)) {
                        out.push(Violation {
                            object: reference.clone(),
                            detail: format!("no pin on a net matching '{p}'"),
                        });
                    }
                }
                if r.power_pins_decoupled {
                    let mut supply: BTreeSet<&str> = BTreeSet::new();
                    for (net, n) in &pins {
                        let f = n.function.to_ascii_uppercase();
                        let is_supply_pin = n.pin_type == "power_in"
                            || (n.pin_type.is_empty()
                                && [
                                    "VDD", "VCC", "VIN", "3V3", "IOVDD", "AVDD", "DVDD", "V+",
                                    "VBAT",
                                ]
                                .iter()
                                .any(|k| f.contains(k)));
                        if is_supply_pin && !ground_net(net) && !is_unconnected(net) {
                            supply.insert(net);
                        }
                    }
                    for net in supply {
                        let has_cap = d.nets.get(net).is_some_and(|nodes| {
                            nodes.iter().any(|n| {
                                Design::prefix(&n.reference) == "C"
                                    && d.nets_of(&n.reference)
                                        .iter()
                                        .any(|(o, x)| x.pin != n.pin && ground_net(o))
                            })
                        });
                        if !has_cap {
                            out.push(Violation {
                                object: format!("{reference} / {net}"),
                                detail: "supply net has no capacitor to ground".into(),
                            });
                        }
                    }
                }
            }
        }
        "board_nets" => {
            let Some(board) = board else {
                return (0, out, Some("board file not found".into()));
            };
            let mut per_net: BTreeMap<String, (f64, usize)> = BTreeMap::new();
            for t in voltaris_sexp::board::tracks(board).items {
                let Some(net) = t.net else { continue };
                if rule
                    .select
                    .name
                    .as_ref()
                    .is_some_and(|p| !net_pattern(p, &net))
                    || rule
                        .select
                        .exclude
                        .as_ref()
                        .is_some_and(|p| glob_match(p, &net))
                {
                    continue;
                }
                let e = per_net.entry(net).or_insert((f64::MAX, 0));
                e.0 = e.0.min(t.width);
                e.1 += 1;
            }
            let min = r.min_track_width_mm.unwrap_or(0.0);
            for (net, (w, n)) in per_net {
                checked += 1;
                if w + 1e-9 < min {
                    out.push(Violation {
                        object: net,
                        detail: format!("narrowest of {n} segment(s) is {w} mm, needs ≥ {min} mm"),
                    });
                }
            }
        }
        _ => {}
    }
    (checked, out, None)
}

fn project_dir(arg: &str) -> PathBuf {
    let p = PathBuf::from(arg);
    if p.is_dir() {
        p
    } else {
        p.parent().map(Path::to_path_buf).unwrap_or(p)
    }
}

fn store_path(dir: &Path) -> PathBuf {
    dir.join(".voltaris").join("checks.json")
}

pub fn load_rules(dir: &Path) -> Vec<Rule> {
    std::fs::read_to_string(store_path(dir))
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .and_then(|v| serde_json::from_value(v["rules"].clone()).ok())
        .unwrap_or_default()
}

fn save_rules(dir: &Path, rules: &[Rule]) -> anyhow::Result<()> {
    let path = store_path(dir);
    std::fs::create_dir_all(path.parent().unwrap())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(
        &tmp,
        serde_json::to_string_pretty(&json!({ "version": 1, "rules": rules }))?,
    )?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

fn rule_schema() -> Value {
    let compsel = json!({
        "type": "object",
        "properties": {
            "ref_prefix": { "type": "string", "description": "Reference prefixes, '|'-separated: \"R\", \"U|IC\"" },
            "value": { "type": "string", "description": "Glob on the value: \"*USBLC*|*ESD*\"" },
            "lib_id": { "type": "string" },
            "footprint": { "type": "string" },
            "exclude_refs": { "type": "string", "description": "Glob of references to skip" }
        }
    });
    let mut connects = compsel.clone();
    connects["properties"]["other_pin_net"] = json!({ "type": "string", "description": "The matched part's other pin must sit on this net: a glob, or @power / @ground / @any_power" });
    connects["properties"]["min"] = json!({ "type": "integer", "minimum": 0, "default": 1 });
    connects["properties"]["max"] = json!({ "type": "integer", "minimum": 0 });
    let mut select = compsel.clone();
    select["properties"]["name"] = json!({ "type": "string", "description": "Net-name glob (nets / board_nets), '|' for alternatives; matches with or without the sheet path" });
    select["properties"]["exclude"] = json!({ "type": "string" });
    select["properties"]["touches"] = compsel.clone();
    select["properties"]["include_power"] = json!({ "type": "boolean", "default": false, "description": "nets scope: also check supply nets" });
    json!({
        "type": "object",
        "properties": {
            "id": { "type": "string", "description": "Short id; derived from the description when omitted" },
            "description": { "type": "string", "description": "The rule in plain English, shown in reports" },
            "scope": { "type": "string", "enum": ["nets", "components", "board_nets"] },
            "severity": { "type": "string", "enum": ["error", "warning"], "default": "warning" },
            "select": select,
            "require": {
                "type": "object",
                "properties": {
                    "connects": connects,
                    "not_connects": compsel,
                    "min_nodes": { "type": "integer", "minimum": 0 },
                    "max_nodes": { "type": "integer", "minimum": 0 },
                    "fields": { "type": "array", "items": { "type": "string" } },
                    "footprint": { "type": "boolean" },
                    "value": { "type": "string" },
                    "pin_net": { "type": "object", "properties": { "pin": { "type": "string" }, "net": { "type": "string" } }, "required": ["pin", "net"] },
                    "connects_net": { "type": "string" },
                    "power_pins_decoupled": { "type": "boolean" },
                    "min_track_width_mm": { "type": "number", "exclusiveMinimum": 0 }
                }
            }
        },
        "required": ["description", "scope", "require"]
    })
}

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "add_check_rule",
            "Store a design rule as an automatic check for this project (<project>/.voltaris/checks.json). \
             Translate the user's plain-English rule into one structured check: scope 'nets' (select nets \
             by name glob and/or the parts they touch; require connects / not_connects / min_nodes / \
             max_nodes), 'components' (select parts; require fields / footprint / value / pin_net / \
             connects_net / power_pins_decoupled) or 'board_nets' (require min_track_width_mm). \
             Net patterns accept @power, @ground and @any_power. Example — \"every ADC input needs a \
             filter cap\": scope nets, select.name \"*ADC*\", require.connects {ref_prefix \"C\", \
             other_pin_net \"@ground\"}. A rule with the same id is replaced.",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string", "description": "Path to the .kicad_pro or project directory" },
                    "rule": rule_schema()
                },
                "required": ["project", "rule"]
            }),
            |args, _ctx| async move { handle_add(args) }
        ),
        tool!(
            "list_check_rules",
            "List this project's stored check rules, and optionally the built-in professional rule pack \
             (footprints, I2C pull-ups, USB ESD, ADC filters, reset pull-ups, dangling nets, IC decoupling, \
             LCSC numbers).",
            json!({
                "type": "object",
                "properties": {
                    "project": { "type": "string" },
                    "include_builtin": { "type": "boolean", "default": true }
                },
                "required": ["project"]
            }),
            |args, _ctx| async move { handle_list(args) }
        ),
        tool!(
            "remove_check_rule",
            "Delete one stored check rule by id.",
            json!({
                "type": "object",
                "properties": { "project": { "type": "string" }, "id": { "type": "string" } },
                "required": ["project", "id"]
            }),
            |args, _ctx| async move { handle_remove(args) }
        ),
        tool!(
            "run_design_checks",
            "Run every stored check rule (plus the built-in professional pack unless builtin is false) \
             against the design: the full-hierarchy KiCad netlist via kicad-cli, or the given sheet when \
             kicad-cli is unavailable, and the board file for board_nets rules. Returns each rule's \
             status, how many objects it checked, and every violation with the net or reference and the \
             reason. Verdict is fail on any error-severity violation and incomplete when part of the \
             design could not be checked.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Root .kicad_sch" },
                    "board": { "type": "string", "description": ".kicad_pcb for board_nets rules (default: next to the schematic)" },
                    "builtin": { "type": "boolean", "default": true },
                    "only": { "type": "array", "items": { "type": "string" }, "description": "Run only these rule ids" },
                    "rules": { "type": "array", "items": rule_schema(), "description": "Extra one-off rules, not stored" }
                },
                "required": ["schematic"]
            }),
            |args, ctx| async move { handle_run(args, ctx).await }
        ),
    ]
}

fn parse_rule(v: &Value) -> Result<Rule, String> {
    let mut rule: Rule = serde_json::from_value(v.clone()).map_err(|e| e.to_string())?;
    if rule.id.trim().is_empty() {
        rule.id = slug(&rule.description);
    }
    validate(&rule)?;
    Ok(rule)
}

fn handle_add(args: &Value) -> anyhow::Result<CallToolResult> {
    let project = match require_str(args, "project") {
        Ok(p) => project_dir(p),
        Err(e) => return Ok(e),
    };
    let rule = match parse_rule(&args["rule"]) {
        Ok(r) => r,
        Err(e) => return Ok(invalid_arg("rule", &e)),
    };
    if builtin_rules().iter().any(|b| b.id == rule.id) {
        return Ok(invalid_arg(
            "rule",
            "ids starting with 'pro-' are reserved for the built-in pack",
        ));
    }
    let mut rules = load_rules(&project);
    let replaced = rules.iter().any(|r| r.id == rule.id);
    rules.retain(|r| r.id != rule.id);
    rules.push(rule.clone());
    save_rules(&project, &rules)?;
    Ok(CallToolResult::json(
        &json!({ "stored": rule, "replaced": replaced, "file": store_path(&project).display().to_string(), "total_rules": rules.len() }),
    ))
}

fn handle_list(args: &Value) -> anyhow::Result<CallToolResult> {
    let project = match require_str(args, "project") {
        Ok(p) => project_dir(p),
        Err(e) => return Ok(e),
    };
    let builtin = args["include_builtin"].as_bool().unwrap_or(true);
    Ok(CallToolResult::json(&json!({
        "project_rules": load_rules(&project),
        "builtin_rules": if builtin { json!(builtin_rules()) } else { json!([]) }
    })))
}

fn handle_remove(args: &Value) -> anyhow::Result<CallToolResult> {
    let project = match require_str(args, "project") {
        Ok(p) => project_dir(p),
        Err(e) => return Ok(e),
    };
    let id = match require_str(args, "id") {
        Ok(i) => i.to_string(),
        Err(e) => return Ok(e),
    };
    let mut rules = load_rules(&project);
    let before = rules.len();
    rules.retain(|r| r.id != id);
    if rules.len() == before {
        return Ok(invalid_arg("id", &format!("no stored rule '{id}'")));
    }
    save_rules(&project, &rules)?;
    Ok(CallToolResult::json(
        &json!({ "removed": id, "remaining": rules.len() }),
    ))
}

async fn handle_run(args: &Value, ctx: &ToolContext) -> anyhow::Result<CallToolResult> {
    let sch = PathBuf::from(match require_str(args, "schematic") {
        Ok(s) => s,
        Err(e) => return Ok(e),
    });
    if !sch.is_file() {
        return Ok(invalid_arg("schematic", "file does not exist"));
    }
    let dir = sch.parent().map(Path::to_path_buf).unwrap_or_default();
    let mut rules = Vec::new();
    if args["builtin"].as_bool().unwrap_or(true) {
        rules.extend(builtin_rules());
    }
    rules.extend(load_rules(&dir));
    if let Some(extra) = args["rules"].as_array() {
        for v in extra {
            match parse_rule(v) {
                Ok(r) => rules.push(r),
                Err(e) => return Ok(invalid_arg("rules", &e)),
            }
        }
    }
    if let Some(only) = args["only"].as_array() {
        let ids: BTreeSet<&str> = only.iter().filter_map(|v| v.as_str()).collect();
        rules.retain(|r| ids.contains(r.id.as_str()));
    }
    let design = design_model::load(&sch, &ctx.config.kicad_cli).await?;
    let board_path = opt_str(args, "board")
        .map(PathBuf::from)
        .unwrap_or_else(|| sch.with_extension("kicad_pcb"));
    let board_tree = std::fs::read_to_string(&board_path)
        .ok()
        .and_then(|t| voltaris_sexp::parser::parse_sexp(&t).ok());
    let mut results = Vec::new();
    let (mut errors, mut warnings, mut incomplete) = (0usize, 0usize, Vec::new());
    for rule in &rules {
        let (checked, violations, skipped) = evaluate(rule, &design, board_tree.as_ref());
        let status = if let Some(reason) = &skipped {
            incomplete.push(format!("{}: {reason}", rule.id));
            "not_run"
        } else if violations.is_empty() {
            "pass"
        } else {
            if rule.severity == "error" {
                errors += violations.len()
            } else {
                warnings += violations.len()
            }
            "fail"
        };
        results.push(json!({ "id": rule.id, "description": rule.description, "severity": rule.severity, "status": status, "checked": checked, "violations": violations }));
    }
    if design.hierarchical && !design.source.starts_with("kicad-cli") {
        incomplete.push("sub-sheets were not followed (kicad-cli unavailable)".into());
    }
    let verdict = if errors > 0 {
        "fail"
    } else if !incomplete.is_empty() {
        "incomplete"
    } else if warnings > 0 {
        "pass_with_warnings"
    } else {
        "pass"
    };
    Ok(CallToolResult::json(&json!({
        "verdict": verdict,
        "errors": errors,
        "warnings": warnings,
        "incomplete": incomplete,
        "design_source": design.source,
        "design_warnings": design.warnings,
        "parts": design.components.len(),
        "nets": design.nets.len(),
        "results": results,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::design_model::{Component, Node};

    fn design() -> Design {
        let mut d = Design::default();
        let comp = |r: &str, v: &str, fp: &str, fields: &[(&str, &str)]| Component {
            reference: r.into(),
            value: v.into(),
            footprint: fp.into(),
            lib_id: String::new(),
            fields: fields
                .iter()
                .map(|(a, b)| (a.to_string(), b.to_string()))
                .collect(),
        };
        for c in [
            comp(
                "U1",
                "ESP32-S3-WROOM-1",
                "RF_Module:ESP32-S3-WROOM-1",
                &[("LCSC", "C2913202")],
            ),
            comp("R1", "4.7k", "R_0402", &[("LCSC", "C25900")]),
            comp("R2", "4.7k", "R_0402", &[]),
            comp("R3", "4.7k", "R_0402", &[]),
            comp("C1", "100n", "", &[]),
            comp("J1", "USB_C", "USB", &[]),
            comp("U2", "USBLC6-2SC6", "SOT-23-6", &[]),
        ] {
            d.components.insert(c.reference.clone(), c);
        }
        let n = |r: &str, p: &str, f: &str, t: &str| Node {
            reference: r.into(),
            pin: p.into(),
            function: f.into(),
            pin_type: t.into(),
        };
        d.nets.insert(
            "/SDA".into(),
            vec![
                n("U1", "12", "IO8", "bidirectional"),
                n("R1", "1", "", "passive"),
                n("R3", "1", "", "passive"),
            ],
        );
        d.nets
            .insert("/SCL".into(), vec![n("U1", "17", "IO9", "bidirectional")]);
        d.nets.insert(
            "+3V3".into(),
            vec![
                n("U1", "2", "3V3", "power_in"),
                n("R1", "2", "", "passive"),
                n("R3", "2", "", "passive"),
            ],
        );
        d.nets.insert(
            "GND".into(),
            vec![n("U1", "1", "GND", "power_in"), n("C1", "2", "", "passive")],
        );
        d.nets.insert(
            "/VBAT_ADC".into(),
            vec![
                n("U1", "4", "IO4", "bidirectional"),
                n("C1", "1", "", "passive"),
                n("R2", "1", "", "passive"),
            ],
        );
        d.nets.insert(
            "/USB_DP".into(),
            vec![
                n("J1", "A6", "D+", "bidirectional"),
                n("U1", "14", "USB_D+", "bidirectional"),
            ],
        );
        d
    }

    fn violations(id: &str) -> Vec<String> {
        let rule = builtin_rules().into_iter().find(|r| r.id == id).unwrap();
        evaluate(&rule, &design(), None)
            .1
            .into_iter()
            .map(|v| format!("{}: {}", v.object, v.detail))
            .collect()
    }

    #[test]
    fn builtin_pack_finds_real_problems() {
        let i2c = violations("pro-i2c-pullups");
        assert!(
            i2c.iter()
                .any(|v| v.starts_with("/SDA") && v.contains("at most 1")),
            "{i2c:?}"
        );
        assert!(
            i2c.iter()
                .any(|v| v.starts_with("/SCL") && v.contains("at least 1")),
            "{i2c:?}"
        );
        assert!(violations("pro-adc-filter").is_empty());
        assert_eq!(
            violations("pro-usb-esd").len(),
            1,
            "USB_DP touches J1 but no ESD part"
        );
        assert_eq!(
            violations("pro-footprints"),
            vec!["C1: no footprint assigned"]
        );
        let dec = violations("pro-ic-decoupling");
        assert_eq!(
            dec,
            vec!["U1 / +3V3: supply net has no capacitor to ground"]
        );
        assert!(violations("pro-no-dangling-nets")
            .iter()
            .any(|v| v.starts_with("/SCL")));
        assert!(violations("pro-lcsc").iter().any(|v| v.starts_with("R2")));
    }

    #[test]
    fn rules_validate_scope_and_round_trip() {
        let r = parse_rule(&json!({"description": "Every ADC input needs a filter cap", "scope": "nets", "select": {"name": "*ADC*"}, "require": {"connects": {"ref_prefix": "C", "other_pin_net": "@ground"}}})).unwrap();
        assert_eq!(r.id, "every-adc-input-needs-a-filter");
        assert!(parse_rule(
            &json!({"description": "x", "scope": "nets", "require": {"fields": ["MPN"]}})
        )
        .is_err());
        assert!(parse_rule(
            &json!({"description": "x", "scope": "parts", "require": {"footprint": true}})
        )
        .is_err());
        assert!(parse_rule(&json!({"description": "x", "scope": "components", "require": {"footprint": true, "bogus": 1}})).is_err());
        let dir = std::env::temp_dir().join(format!("kcp-checks-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        save_rules(&dir, std::slice::from_ref(&r)).unwrap();
        assert_eq!(load_rules(&dir), vec![r]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pin_net_and_board_width_rules() {
        let pin_rule = parse_rule(&json!({"description": "ESP32 IO8 must be SDA", "scope": "components", "select": {"ref_prefix": "U", "value": "ESP32*"}, "require": {"pin_net": {"pin": "IO8", "net": "*SDA"}}})).unwrap();
        assert!(evaluate(&pin_rule, &design(), None).1.is_empty());
        let board = voltaris_sexp::parser::parse_sexp(r#"(kicad_pcb (net 0 "") (net 1 "+3V3") (segment (start 0 0) (end 5 0) (width 0.2) (layer "F.Cu") (net 1)))"#).unwrap();
        let w = parse_rule(&json!({"description": "Power traces at least 0.4 mm", "scope": "board_nets", "select": {"name": "@any_power"}, "require": {"min_track_width_mm": 0.4}, "severity": "error"})).unwrap();
        let (checked, v, _) = evaluate(&w, &design(), Some(&board));
        assert_eq!((checked, v.len()), (1, 1));
        assert!(evaluate(&w, &design(), None).2.is_some());
    }
}
