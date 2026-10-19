//! `templates` toolset — Reference circuit library for validated subcircuit templates.
//!
//! Templates are JSON files stored in `~/.voltaris/templates/` (user) and
//! shipped as embedded defaults. Claude retrieves a template and adapts it to the
//! user's project — this prevents hallucinating component values.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, require_str, ToolContext, ToolDef};
use serde_json::json;
use std::path::PathBuf;
use tracing::{debug, info, warn};

// ─── Embedded starter templates ──────────────────────────────────────────────

fn builtin_templates() -> Vec<serde_json::Value> {
    vec![
        json!({
            "id": "usb_c_5v_sink",
            "name": "USB-C Power Sink (5V default)",
            "description": "USB Type-C receptacle with CC resistors for 5V default power and ESD protection on D+/D-.",
            "category": "connectivity/usb",
            "tags": ["usb-c", "power", "5v", "sink"],
            "components": [
                {"ref_prefix": "J", "lib_id": "Connector:USB_C_Receptacle_USB2.0_16P", "value": "USB_C", "notes": "USB-C receptacle, 16-pin USB 2.0"},
                {"ref_prefix": "R", "lib_id": "Device:R", "value": "5.1k", "quantity": 2, "package": "0402", "notes": "CC1 and CC2 pull-down — required for 5V default current"},
                {"ref_prefix": "D", "lib_id": "Device:D_TVS", "value": "PRTR5V0U2X", "quantity": 1, "notes": "ESD protection on D+/D-"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "100nF", "quantity": 1, "package": "0402", "notes": "VBUS decoupling"}
            ],
            "connections": [
                {"from": "J.VBUS", "to_net": "VUSB", "notes": "5V power from USB host"},
                {"from": "J.CC1", "via": "R1", "to_net": "GND", "notes": "5.1k pull-down identifies as UFP sink"},
                {"from": "J.CC2", "via": "R2", "to_net": "GND", "notes": "5.1k pull-down"},
                {"from": "J.D+", "to_net": "USB_DP", "notes": "USB data positive"},
                {"from": "J.D-", "to_net": "USB_DN", "notes": "USB data negative"},
                {"from": "J.GND", "to_net": "GND", "notes": "Ground"}
            ],
            "design_notes": "CC resistor value is critical: 5.1k ±1% for default 5V/900mA. For USB 2.0 only, connect D+/D- directly to MCU. For USB 3.x, route TX/RX as controlled impedance pairs.",
            "references": ["USB Type-C Spec Rev 2.0, Table 4-25"]
        }),
        json!({
            "id": "ldo_3v3",
            "name": "3.3V LDO Regulator",
            "description": "Low-dropout 3.3V regulator with input/output capacitors. Generic topology, adapt MPN to your needs.",
            "category": "power/regulator",
            "tags": ["ldo", "3v3", "regulator", "power"],
            "components": [
                {"ref_prefix": "U", "lib_id": "Regulator_Linear:AMS1117-3.3", "value": "AMS1117-3.3", "notes": "3.3V 1A LDO — substitute AP2112, MCP1700, etc."},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "10uF", "quantity": 1, "package": "0805", "notes": "Input capacitor — ceramic X5R or X7R"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "10uF", "quantity": 1, "package": "0805", "notes": "Output capacitor — ceramic, check ESR requirements in datasheet"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "100nF", "quantity": 2, "package": "0402", "notes": "Bypass caps on input and output"}
            ],
            "connections": [
                {"from": "U.VIN", "to_net": "VIN", "notes": "Input voltage (check max Vin for chosen LDO)"},
                {"from": "U.VOUT", "to_net": "VCC_3V3", "notes": "3.3V regulated output"},
                {"from": "U.GND", "to_net": "GND", "notes": "Ground — ensure low-impedance path"}
            ],
            "design_notes": "Place input and output caps within 5mm of regulator pins. AMS1117 needs >10uF output for stability. For low-noise applications, consider ADP151 or TPS7A20.",
            "references": ["AMS1117 datasheet, Section 8.2"]
        }),
        json!({
            "id": "stm32_minimal",
            "name": "STM32 Minimal System",
            "description": "STM32 MCU with HSE crystal, decoupling caps, reset circuit, and SWD debug header.",
            "category": "mcu/stm32",
            "tags": ["stm32", "mcu", "minimal", "crystal", "swd"],
            "components": [
                {"ref_prefix": "U", "lib_id": "MCU_ST_STM32F4:STM32F411CEUx", "value": "STM32F411CEU6", "notes": "48-pin UFQFPN — substitute any STM32 in same package"},
                {"ref_prefix": "Y", "lib_id": "Device:Crystal", "value": "8MHz", "quantity": 1, "notes": "HSE crystal — check MCU datasheet for supported range"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "20pF", "quantity": 2, "package": "0402", "notes": "Crystal load caps — calculate from datasheet CL spec"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "100nF", "quantity": 5, "package": "0402", "notes": "Decoupling caps — one per VDD/VDDA pin"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "4.7uF", "quantity": 1, "package": "0402", "notes": "Bulk decoupling on VDD"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "1uF", "quantity": 1, "package": "0402", "notes": "VCAP pin — required for internal regulator"},
                {"ref_prefix": "R", "lib_id": "Device:R", "value": "10k", "quantity": 1, "package": "0402", "notes": "NRST pull-up"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "100nF", "quantity": 1, "package": "0402", "notes": "NRST filter cap to GND"},
                {"ref_prefix": "J", "lib_id": "Connector:Conn_ARM_JTAG_SWD_10", "value": "SWD", "notes": "10-pin ARM SWD debug header"}
            ],
            "connections": [
                {"from": "U.VDD", "to_net": "VCC_3V3", "notes": "All VDD pins to 3.3V"},
                {"from": "U.VDDA", "to_net": "VCC_3V3", "notes": "Analog supply — add ferrite bead for sensitive analog work"},
                {"from": "U.VSS", "to_net": "GND", "notes": "All VSS pins to ground"},
                {"from": "U.NRST", "to_net": "NRST", "notes": "Reset with 10k pull-up + 100nF cap"},
                {"from": "U.OSC_IN", "to_net": "HSE_IN", "notes": "Crystal input"},
                {"from": "U.OSC_OUT", "to_net": "HSE_OUT", "notes": "Crystal output"},
                {"from": "U.SWDIO", "to_net": "SWDIO", "notes": "SWD data to debug header"},
                {"from": "U.SWCLK", "to_net": "SWCLK", "notes": "SWD clock to debug header"}
            ],
            "design_notes": "Crystal load cap formula: CL = (C1*C2)/(C1+C2) + Cstray, where Cstray ≈ 3-5pF. Place all decoupling caps within 3mm of their VDD pin. VCAP capacitor value is critical — check your specific STM32 variant's datasheet.",
            "references": ["AN4488: Getting started with STM32F4 MCU hardware development"]
        }),
        json!({
            "id": "i2c_pullups",
            "name": "I2C Bus Pull-ups",
            "description": "Standard I2C pull-up resistors for SDA and SCL lines.",
            "category": "connectivity/i2c",
            "tags": ["i2c", "pull-up", "bus"],
            "components": [
                {"ref_prefix": "R", "lib_id": "Device:R", "value": "4.7k", "quantity": 2, "package": "0402", "notes": "SDA and SCL pull-ups. Use 2.2k for fast-mode (400kHz), 1k for fast-mode plus (1MHz)"}
            ],
            "connections": [
                {"from": "R1.1", "to_net": "SDA", "notes": "I2C data line"},
                {"from": "R1.2", "to_net": "VCC_3V3", "notes": "Pull to I2C bus voltage"},
                {"from": "R2.1", "to_net": "SCL", "notes": "I2C clock line"},
                {"from": "R2.2", "to_net": "VCC_3V3", "notes": "Pull to I2C bus voltage"}
            ],
            "design_notes": "One set of pull-ups per I2C bus — do NOT add pull-ups on every device. Value depends on bus speed and capacitance. 4.7k is safe for standard mode (100kHz) with <400pF bus capacitance.",
            "references": ["NXP UM10204: I2C-bus specification"]
        }),
        json!({
            "id": "led_indicator",
            "name": "LED Indicator Circuit",
            "description": "Simple LED with current-limiting resistor, driven by a GPIO pin.",
            "category": "misc/led",
            "tags": ["led", "indicator", "gpio"],
            "components": [
                {"ref_prefix": "D", "lib_id": "Device:LED", "value": "LED_Green", "quantity": 1, "package": "0603", "notes": "Standard indicator LED"},
                {"ref_prefix": "R", "lib_id": "Device:R", "value": "1k", "quantity": 1, "package": "0402", "notes": "Current limiter: R = (Vcc - Vf) / If. For 3.3V, green Vf≈2.1V, If=1.2mA → 1k"}
            ],
            "connections": [
                {"from": "GPIO", "to": "R1.1", "notes": "GPIO output drives LED through resistor"},
                {"from": "R1.2", "to": "D1.A", "notes": "Resistor to LED anode"},
                {"from": "D1.K", "to_net": "GND", "notes": "LED cathode to ground"}
            ],
            "design_notes": "R = (VCC - Vf) / If. For 3.3V GPIO: green (Vf=2.1V) → 1k gives 1.2mA. Red (Vf=1.8V) → 680R gives 2.2mA. Bright LEDs may only need 0.5mA. Check your LED's datasheet for Vf and recommended If.",
            "references": []
        }),
        json!({
            "id": "buck_converter",
            "name": "Buck Converter (Step-Down)",
            "description": "Synchronous buck converter with input/output caps, inductor, and feedback resistors. Generic topology — adapt MPN and passives to your voltage/current needs.",
            "category": "power/switching",
            "tags": ["buck", "step-down", "switching", "regulator", "power"],
            "components": [
                {"ref_prefix": "U", "lib_id": "Regulator_Switching:TPS563200", "value": "TPS563200DDCR", "notes": "3A sync buck, 4.5-17V input. Substitute: AP63356, MP2315, SY8089"},
                {"ref_prefix": "L", "lib_id": "Device:L", "value": "4.7uH", "quantity": 1, "notes": "Inductor — check datasheet for recommended value and saturation current > Iout*1.3"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "10uF", "quantity": 2, "package": "0805", "notes": "Input capacitors — X5R/X7R ceramic, voltage rating > Vin*1.5"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "22uF", "quantity": 2, "package": "0805", "notes": "Output capacitors — low ESR ceramic"},
                {"ref_prefix": "C", "lib_id": "Device:C", "value": "100nF", "quantity": 1, "package": "0402", "notes": "Boot cap"},
                {"ref_prefix": "R", "lib_id": "Device:R", "value": "100k", "quantity": 1, "package": "0402", "notes": "Feedback upper resistor — adjust for target Vout"},
                {"ref_prefix": "R", "lib_id": "Device:R", "value": "49.9k", "quantity": 1, "package": "0402", "notes": "Feedback lower resistor — Vout = Vref * (1 + Rtop/Rbot)"}
            ],
            "connections": [
                {"from": "U.VIN", "to_net": "VIN", "notes": "Input power"},
                {"from": "U.SW", "to": "L1.1", "notes": "Switch node to inductor"},
                {"from": "L1.2", "to_net": "VOUT", "notes": "Inductor output"},
                {"from": "U.FB", "via": "voltage divider R_top/R_bot", "to_net": "VOUT", "notes": "Feedback voltage divider"},
                {"from": "U.BOOT", "notes": "Bootstrap cap from BOOT to SW"},
                {"from": "U.GND", "to_net": "GND", "notes": "Power ground — kelvin sense to output cap GND"}
            ],
            "design_notes": "Layout is critical: keep input caps close to VIN/GND pins, keep SW trace short and wide (high di/dt), keep feedback divider close to FB pin away from SW node. Ground plane under inductor improves EMI. Calculate passives from datasheet — do NOT guess values.",
            "references": ["TI SLVA477: Application Note for TPS563200"]
        }),
    ]
}

/// Voltaris circuit library. Every `lib_id`, footprint and pin number
/// was checked against the KiCad 10.0.6 libraries; `nets` drive the exact
/// wiring plan `apply_template` returns.
const PRO_TEMPLATES: &str = include_str!("templates_pro.json");

fn pro_templates() -> Vec<serde_json::Value> {
    serde_json::from_str(PRO_TEMPLATES).expect("templates_pro.json is valid JSON")
}

// ─── Template storage paths ──────────────────────────────────────────────────

fn user_templates_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var("APPDATA").unwrap_or_default();
        PathBuf::from(appdata).join("voltaris").join("templates")
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join(".voltaris").join("templates")
    }
}

/// Load all templates: builtins + any user-created ones from disk.
async fn load_all_templates() -> Vec<serde_json::Value> {
    let mut templates = builtin_templates();
    templates.extend(pro_templates());

    let user_dir = user_templates_dir();
    if user_dir.is_dir() {
        if let Ok(mut rd) = tokio::fs::read_dir(&user_dir).await {
            while let Ok(Some(entry)) = rd.next_entry().await {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    match tokio::fs::read_to_string(&path).await {
                        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
                            Ok(tmpl) => {
                                debug!(path = %path.display(), "Loaded user template");
                                templates.push(tmpl);
                            }
                            Err(e) => {
                                warn!(path = %path.display(), error = %e, "Failed to parse user template")
                            }
                        },
                        Err(e) => {
                            warn!(path = %path.display(), error = %e, "Failed to read user template")
                        }
                    }
                }
            }
        }
    }

    templates
}

// ─── Tool definitions ─────────────────────────────────────────────────────────

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "search_templates",
            "Search the reference circuit template library. Returns matching templates for \
             common subcircuits: ESP32-S3/C3/C6 module cores, USB-C device port with ESD, CH340C \
             auto-program, Li-ion charger/protection/power-path, LDOs, buck, WS2812B, SHT30, STM32, \
             I2C pull-ups, LEDs. Use these instead of designing from scratch — templates carry \
             verified values, footprints and (for most) an exact pin-level wiring plan.",
            json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search string: 'usb-c', '3.3v regulator', 'stm32', 'i2c', 'led', 'buck converter', etc."
                    },
                    "category": {
                        "type": "string",
                        "description": "Filter by category: 'power', 'connectivity', 'mcu', 'misc' (optional)"
                    }
                },
                "required": ["query"]
            }),
            |args, ctx| async move { handle_search_templates(args, ctx).await }
        ),
        tool!(
            "get_template",
            "Get full details for a reference circuit template including all components, \
             connections, and design notes. Use the template ID from search_templates.",
            json!({
                "type": "object",
                "properties": {
                    "template_id": { "type": "string", "description": "Template ID (e.g. 'usb_c_5v_sink', 'ldo_3v3', 'stm32_minimal')" }
                },
                "required": ["template_id"]
            }),
            |args, ctx| async move { handle_get_template(args, ctx).await }
        ),
        tool!(
            "apply_template",
            "Instantiate a reference circuit template into the current schematic. Places every \
             component with its footprint. Templates that define nets also return 'wiring_plan': \
             ready-to-run batch_connect_to_net calls with exact reference/pin pairs, plus pins that \
             need no-connect flags. Use net_mappings to rename template nets to your project's nets \
             (e.g. {\"VBUS\": \"+5V\"}).",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "template_id": { "type": "string", "description": "Template ID to instantiate" },
                    "position_x": { "type": "number", "description": "X position to place the subcircuit (mm)", "default": 100.0 },
                    "position_y": { "type": "number", "description": "Y position to place the subcircuit (mm)", "default": 100.0 },
                    "net_mappings": {
                        "type": "object",
                        "additionalProperties": true,
                        "description": "Map template net names to your project's net names. E.g. {\"VUSB\": \"VCC_5V\", \"GND\": \"GND\"}"
                    },
                    "ref_start": {
                        "type": "integer",
                        "description": "Starting reference number (e.g. 10 → R10, C10, U10). Auto-detected if omitted."
                    }
                },
                "required": ["schematic", "template_id"]
            }),
            |args, ctx| async move { handle_apply_template(args, ctx).await }
        ),
        tool!(
            "list_template_categories",
            "List all available template categories and the number of templates in each.",
            json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            |args, ctx| async move { handle_list_categories(args, ctx).await }
        ),
    ]
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_search_templates(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let query = match require_str(args, "query") {
        Ok(v) => v.to_lowercase(),
        Err(e) => return Ok(e),
    };
    let category_filter = args["category"].as_str().map(|s| s.to_lowercase());

    info!(query = %query, category = ?category_filter, "Searching templates");

    let templates = load_all_templates().await;
    let mut results = Vec::new();

    for tmpl in &templates {
        let id = tmpl["id"].as_str().unwrap_or("");
        let name = tmpl["name"].as_str().unwrap_or("");
        let desc = tmpl["description"].as_str().unwrap_or("");
        let category = tmpl["category"].as_str().unwrap_or("");
        let tags: Vec<&str> = tmpl["tags"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        // Category filter
        if let Some(ref cat) = category_filter {
            if !category.to_lowercase().contains(cat) {
                continue;
            }
        }

        // Search across name, description, tags, category
        let haystack =
            format!("{} {} {} {} {}", id, name, desc, category, tags.join(" ")).to_lowercase();

        let matches = query.split_whitespace().all(|word| haystack.contains(word));

        if matches {
            let component_count: usize =
                tmpl["components"].as_array().map(|a| a.len()).unwrap_or(0);
            results.push(json!({
                "id": id,
                "name": name,
                "description": desc,
                "category": category,
                "tags": tags,
                "component_count": component_count
            }));
        }
    }

    debug!(query = %query, results = results.len(), "Template search complete");

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "query": query,
            "count": results.len(),
            "templates": results
        }))
        .unwrap(),
    ))
}

async fn handle_get_template(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let template_id = match require_str(args, "template_id") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    info!(template_id = %template_id, "Loading template");

    let templates = load_all_templates().await;
    let tmpl = templates
        .iter()
        .find(|t| t["id"].as_str() == Some(&template_id));

    match tmpl {
        Some(t) => Ok(CallToolResult::text(serde_json::to_string(t).unwrap())),
        None => {
            warn!(template_id = %template_id, "Template not found");
            Ok(CallToolResult::error(format!(
                "Template '{}' not found. Use search_templates to find available templates.",
                template_id
            )))
        }
    }
}

async fn handle_apply_template(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let template_id = match require_str(args, "template_id") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let base_x = args["position_x"].as_f64().unwrap_or(100.0);
    let base_y = args["position_y"].as_f64().unwrap_or(100.0);
    let net_mappings: std::collections::HashMap<String, String> = args["net_mappings"]
        .as_object()
        .map(|m| {
            m.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    info!(
        template_id = %template_id,
        schematic = %sch_path.display(),
        position = ?(base_x, base_y),
        net_mappings = ?net_mappings,
        "Applying template"
    );

    let templates = load_all_templates().await;
    let tmpl = match templates
        .iter()
        .find(|t| t["id"].as_str() == Some(&template_id))
    {
        Some(t) => t.clone(),
        None => {
            warn!(template_id = %template_id, "Template not found for apply");
            return Ok(CallToolResult::error(format!(
                "Template '{}' not found",
                template_id
            )));
        }
    };

    // Templates are ordinary symbol placements. Load and validate the same
    // instance context used by the single and batch placement tools rather
    // than hand-writing a second, incomplete interpretation of KiCad's
    // instance metadata (#609).
    let mut sch = match voltaris_schematic_editor::Schematic::load(&sch_path) {
        Ok(schematic) => schematic,
        Err(error) => return Ok(CallToolResult::error(error.to_string())),
    };
    let context = match crate::tools::sheet_instance_context(&sch_path, &mut sch) {
        Ok(context) => context,
        Err(error) => return Ok(error.into_tool_result()),
    };
    if let Err(error) = crate::tools::validate_sheet_instance_state(&sch_path, &sch, &context) {
        return Ok(error.into_tool_result());
    }
    let source = match crate::tools::library::KiCadSymbolSource::for_file(&sch_path) {
        Ok(source) => source,
        Err(error) => return Ok(error.into_tool_result()),
    };

    // Determine starting reference numbers by scanning existing components
    let ref_start = args["ref_start"]
        .as_u64()
        .map(|n| n as usize)
        .unwrap_or_else(|| find_next_ref_number(&sch.to_source()));

    let components = tmpl["components"].as_array().cloned().unwrap_or_default();
    let mut placed = Vec::new();
    let mut placed_targets = Vec::new();
    let mut ref_counters: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let mut role_map: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();

    // Place components in a column layout
    let spacing_y = 15.0; // mm between components
    for comp in components.iter() {
        let ref_prefix = comp["ref_prefix"].as_str().unwrap_or("U");
        let lib_id = comp["lib_id"].as_str().unwrap_or("Device:R");
        let value = comp["value"].as_str().unwrap_or("");
        let quantity = comp["quantity"].as_u64().unwrap_or(1) as usize;
        let notes = comp["notes"].as_str().unwrap_or("");
        let footprint = comp["footprint"].as_str();
        let role = comp["role"].as_str();

        for _q in 0..quantity {
            let counter = ref_counters
                .entry(ref_prefix.to_string())
                .or_insert(ref_start);
            let reference = format!("{}{}", ref_prefix, counter);
            *counter += 1;

            let x = base_x;
            let y = base_y + (placed.len() as f64) * spacing_y;
            let placement = match crate::tools::sch_components::place_one_component(
                &mut sch,
                &context.instance_paths,
                &context.project_name,
                lib_id,
                x,
                y,
                0.0,
                None,
                Some(&reference),
                Some(value),
                footprint,
                1,
                &source,
            ) {
                Ok(placement) => placement,
                Err(error) => return Ok(error),
            };
            placed_targets.push((placement.uuid, reference.clone()));

            if let Some(role) = role {
                role_map.insert(role.to_string(), reference.clone());
            }
            placed.push(json!({
                "reference": reference,
                "role": role,
                "lib_id": lib_id,
                "value": value,
                "footprint": footprint,
                "x": x, "y": y,
                "notes": notes
            }));

            debug!(reference = %reference, lib_id = %lib_id, value = %value, "Placed template component");
        }
    }

    // The editor's overwrite is an atomic compare-and-swap against the exact
    // revision loaded above. All placements are prepared in memory, so a
    // preflight or library failure leaves the schematic byte-for-byte intact.
    if let Err(error) = sch.overwrite() {
        return Ok(crate::tools::mutation_outcome_uncertain(
            &sch_path,
            "apply_template",
            format!("schematic persistence failed: {error}"),
        ));
    }
    let committed = match voltaris_schematic_editor::Schematic::load(&sch_path) {
        Ok(schematic) => schematic,
        Err(error) => {
            return Ok(crate::tools::mutation_outcome_uncertain(
                &sch_path,
                "apply_template",
                format!("saved schematic could not be reloaded: {error}"),
            ))
        }
    };
    if let Err(error) = crate::tools::validate_sheet_instance_state(&sch_path, &committed, &context)
    {
        return Ok(crate::tools::mutation_outcome_uncertain(
            &sch_path,
            "apply_template",
            format!("saved template instance validation failed: {error}"),
        ));
    }
    for (uuid, reference) in &placed_targets {
        if !committed
            .symbols
            .as_slice()
            .iter()
            .any(|symbol| symbol.uuid == *uuid && symbol.reference() == Some(reference))
        {
            return Ok(crate::tools::mutation_outcome_uncertain(
                &sch_path,
                "apply_template",
                format!("saved template readback did not contain {reference} with UUID {uuid}"),
            ));
        }
    }

    info!(
        template_id = %template_id,
        components_placed = placed.len(),
        "Template applied successfully"
    );

    // Build the net mapping guide for the user/Claude to wire up
    let connections = tmpl["connections"].as_array().cloned().unwrap_or_default();
    let mapped_connections: Vec<serde_json::Value> = connections
        .iter()
        .map(|conn| {
            let mut c = conn.clone();
            let original_net = c["to_net"].as_str().map(String::from);
            if let Some(net) = original_net {
                if let Some(mapped) = net_mappings.get(&net) {
                    c["to_net"] = json!(mapped);
                    c["mapped_from"] = json!(net);
                }
            }
            c
        })
        .collect();

    if tmpl.get("nets").is_some() {
        let plan = match wiring_plan(&tmpl, &role_map, &net_mappings, &sch_path) {
            Ok(plan) => plan,
            Err(message) => {
                return Ok(CallToolResult::error(format!(
                    "{template_id} was placed but its wiring plan could not be built: {message}"
                )))
            }
        };
        return Ok(CallToolResult::json(&json!({
            "template": template_id,
            "components_placed": placed,
            "role_map": role_map,
            "wiring_plan": plan["calls"],
            "no_connect_pins": plan["no_connect"],
            "design_notes": tmpl["design_notes"],
            "layout_notes": tmpl["layout_notes"],
            "references": tmpl["references"],
            "next_steps": "Run every wiring_plan entry in order (each is a ready batch_connect_to_net call), \
                           add a no-connect flag on each no_connect_pins entry, then run_erc. \
                           Power nets use global labels so they join the project's power symbols of the same name."
        })));
    }

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "template": template_id,
            "components_placed": placed,
            "connections_to_wire": mapped_connections,
            "design_notes": tmpl["design_notes"],
            "next_steps": "Use connect_to_net or connect_pins to wire the placed components according to the connections list above."
        }))
        .unwrap(),
    ))
}

/// Turn a template's `nets` map into ready-to-run `batch_connect_to_net`
/// argument objects, resolving roles to placed references and template net
/// names through the caller's `net_mappings`.
fn wiring_plan(
    tmpl: &serde_json::Value,
    role_map: &std::collections::BTreeMap<String, String>,
    net_mappings: &std::collections::HashMap<String, String>,
    sch_path: &std::path::Path,
) -> Result<serde_json::Value, String> {
    let resolve = |spec: &str| -> Result<(String, String), String> {
        let (role, pin) = spec
            .split_once(':')
            .ok_or_else(|| format!("pin spec '{spec}' is not ROLE:PIN"))?;
        let reference = role_map
            .get(role)
            .ok_or_else(|| format!("pin spec '{spec}' names unknown role '{role}'"))?;
        Ok((reference.clone(), pin.to_string()))
    };
    let power: std::collections::HashSet<&str> = tmpl["power_nets"]
        .as_array()
        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();
    let mut calls = Vec::new();
    let nets = tmpl["nets"].as_object().ok_or("'nets' must be an object")?;
    for (net, pins) in nets {
        let pins = pins
            .as_array()
            .ok_or_else(|| format!("net '{net}' must list pins"))?;
        let mut specs = Vec::new();
        for pin in pins {
            let spec = pin
                .as_str()
                .ok_or_else(|| format!("net '{net}' has a non-string pin"))?;
            let (reference, pin_number) = resolve(spec)?;
            specs.push(json!({ "reference": reference, "pin_number": pin_number }));
        }
        let name = net_mappings
            .get(net)
            .cloned()
            .unwrap_or_else(|| net.clone());
        let mut args = json!({
            "schematic": sch_path.display().to_string(),
            "net_name": name,
            "pins": specs,
        });
        if power.contains(net.as_str()) {
            args["label_type"] = json!("global_label");
        }
        calls.push(json!({ "tool": "batch_connect_to_net", "arguments": args }));
    }
    let mut no_connect = Vec::new();
    for pin in tmpl["no_connect"].as_array().into_iter().flatten() {
        let spec = pin.as_str().ok_or("no_connect entries must be strings")?;
        let (reference, pin_number) = resolve(spec)?;
        no_connect.push(json!({ "reference": reference, "pin_number": pin_number }));
    }
    Ok(json!({ "calls": calls, "no_connect": no_connect }))
}

async fn handle_list_categories(
    _args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let templates = load_all_templates().await;
    let mut categories: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for tmpl in &templates {
        let cat = tmpl["category"]
            .as_str()
            .unwrap_or("uncategorized")
            .to_string();
        *categories.entry(cat).or_insert(0) += 1;
    }

    let mut sorted: Vec<_> = categories.into_iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    Ok(CallToolResult::text(
        serde_json::to_string(&json!({
            "categories": sorted.iter().map(|(cat, count)| json!({"category": cat, "count": count})).collect::<Vec<_>>(),
            "total_templates": templates.len()
        }))
        .unwrap(),
    ))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Find the next available reference number by scanning existing references in the schematic.
fn find_next_ref_number(content: &str) -> usize {
    let mut max_ref = 0usize;
    let mut pos = 0;
    while let Some(ref_pos) = content[pos..].find("(reference \"") {
        let abs = pos + ref_pos + 12;
        if let Some(end) = content[abs..].find('"') {
            let reference = &content[abs..abs + end];
            // Extract the numeric suffix
            let num_str: String = reference
                .chars()
                .rev()
                .take_while(|c| c.is_ascii_digit())
                .collect::<String>()
                .chars()
                .rev()
                .collect();
            if let Ok(num) = num_str.parse::<usize>() {
                if num > max_ref {
                    max_ref = num;
                }
            }
        }
        pos = abs + 1;
    }
    max_ref + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::ToolRouter;
    use crate::tools::{ServerConfig, KICAD_ENV_LOCK};
    use std::sync::Arc;

    struct TemplateEnvironment {
        _guard: std::sync::MutexGuard<'static, ()>,
        old_appdata: Option<std::ffi::OsString>,
        old_home: Option<std::ffi::OsString>,
    }

    impl Drop for TemplateEnvironment {
        fn drop(&mut self) {
            restore_env("APPDATA", self.old_appdata.take());
            restore_env("HOME", self.old_home.take());
        }
    }

    fn restore_env(name: &str, value: Option<std::ffi::OsString>) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }

    fn test_ctx() -> Arc<ToolContext> {
        Arc::new(ToolContext::new(
            ServerConfig {
                kicad_cli: String::new(),
                kicad_binary: String::new(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                eager_toolsets: false,
            },
            Arc::new(ToolRouter::new()),
        ))
    }

    fn template_fixture() -> (tempfile::TempDir, std::path::PathBuf, TemplateEnvironment) {
        let guard = KICAD_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config");
        let old_appdata = std::env::var_os("APPDATA");
        let old_home = std::env::var_os("HOME");
        std::env::set_var("APPDATA", &config);
        std::env::set_var("HOME", &config);
        let template_dir = if cfg!(target_os = "windows") {
            config.join("voltaris").join("templates")
        } else {
            config.join(".voltaris").join("templates")
        };
        std::fs::create_dir_all(&template_dir).unwrap();
        std::fs::write(
            template_dir.join("one_resistor.json"),
            serde_json::to_vec_pretty(&json!({
                "id": "test_one_resistor",
                "name": "One resistor",
                "description": "Hermetic placement fixture",
                "category": "test",
                "components": [
                    {"ref_prefix": "R", "lib_id": "Device:R", "value": "4.7k"}
                ],
                "connections": [],
                "design_notes": "test"
            }))
            .unwrap(),
        )
        .unwrap();

        // A real KiCad schematic fixture, including a fully embedded
        // Device:R definition, makes this independent of a KiCad installation.
        let schematic = dir.path().join("derived_lib_name.kicad_sch");
        std::fs::copy(
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/derived_lib_name.kicad_sch"
            ),
            &schematic,
        )
        .unwrap();
        std::fs::write(dir.path().join("derived_lib_name.kicad_pro"), "{}\n").unwrap();

        let environment = TemplateEnvironment {
            _guard: guard,
            old_appdata,
            old_home,
        };
        (dir, schematic, environment)
    }

    async fn call_registered(
        definitions: Vec<ToolDef>,
        name: &str,
        args: serde_json::Value,
        ctx: Arc<ToolContext>,
    ) -> CallToolResult {
        let tool = definitions
            .into_iter()
            .find(|tool| tool.name == name)
            .expect("registered tool");
        (tool.handler)(&args, ctx).await.unwrap()
    }

    #[tokio::test]
    async fn apply_template_uses_project_identity_and_leaves_batch_writes_usable() {
        let (_dir, schematic, _environment) = template_fixture();
        let ctx = test_ctx();

        let applied = call_registered(
            tools(),
            "apply_template",
            json!({
                "schematic": schematic.display().to_string(),
                "template_id": "test_one_resistor",
                "position_x": 180.0,
                "position_y": 100.0
            }),
            ctx.clone(),
        )
        .await;
        assert!(!applied.is_error, "{applied:?}");

        let saved = voltaris_schematic_editor::Schematic::load(&schematic).unwrap();
        let placed = saved
            .symbols
            .as_slice()
            .iter()
            .find(|symbol| symbol.position().0 > 175.0)
            .expect("template-created symbol");
        let instances = placed.instances();
        assert_eq!(instances.len(), 1, "{instances:?}");
        assert_eq!(instances[0].project.as_deref(), Some("derived_lib_name"));
        assert_eq!(
            instances[0].path.as_deref(),
            Some("/11111111-1111-4111-8111-111111111111")
        );
        assert_eq!(instances[0].reference.as_deref(), Some("R3"));
        assert_eq!(instances[0].unit, Some(1));
        let source = std::fs::read_to_string(&schematic).unwrap();
        assert!(!source.contains("(project \"\""), "{source}");

        // Exercise the registered batch tool after the template mutation. The
        // original defect made this exact next call fail stale_target.
        let batch = call_registered(
            crate::tools::sch_batch::tools(),
            "batch_place_components",
            json!({
                "schematic": schematic.display().to_string(),
                "components": [{
                    "lib_id": "Device:R", "reference": "R99",
                    "x": 190.0, "y": 100.0
                }]
            }),
            ctx,
        )
        .await;
        assert!(!batch.is_error, "{batch:?}");
        assert!(voltaris_schematic_editor::Schematic::load(&schematic)
            .unwrap()
            .symbols
            .by_reference("R99")
            .is_some());
    }

    #[tokio::test]
    async fn apply_template_refuses_malformed_instance_metadata_without_writing() {
        let (_dir, schematic, _environment) = template_fixture();
        let malformed = std::fs::read_to_string(&schematic).unwrap().replacen(
            "(project \"derived_lib_name\"",
            "(project \"\"",
            1,
        );
        assert_ne!(
            malformed,
            std::fs::read_to_string(&schematic).unwrap(),
            "fixture must contain an instance project to corrupt"
        );
        std::fs::write(&schematic, &malformed).unwrap();
        let before = std::fs::read(&schematic).unwrap();

        let refused = call_registered(
            tools(),
            "apply_template",
            json!({
                "schematic": schematic.display().to_string(),
                "template_id": "test_one_resistor"
            }),
            test_ctx(),
        )
        .await;
        assert!(refused.is_error, "{refused:?}");
        assert_eq!(
            crate::mcp::error::extract_error_kind(&refused).as_deref(),
            Some("stale_target")
        );
        assert_eq!(std::fs::read(&schematic).unwrap(), before);
    }
    /// Every pro template must be internally consistent: unique roles, every
    /// net pin names a declared role, nothing listed twice.
    #[test]
    fn pro_templates_are_self_consistent() {
        let ids: Vec<String> = builtin_templates()
            .iter()
            .map(|t| t["id"].as_str().unwrap().to_string())
            .collect();
        for tmpl in pro_templates() {
            let id = tmpl["id"].as_str().unwrap();
            assert!(
                !ids.contains(&id.to_string()),
                "{id} duplicates a builtin id"
            );
            let roles: Vec<&str> = tmpl["components"]
                .as_array()
                .unwrap()
                .iter()
                .map(|c| {
                    c["role"]
                        .as_str()
                        .unwrap_or_else(|| panic!("{id}: component without role"))
                })
                .collect();
            let unique: std::collections::BTreeSet<_> = roles.iter().collect();
            assert_eq!(unique.len(), roles.len(), "{id}: duplicate role");
            let mut seen = std::collections::BTreeSet::new();
            let mut role_pins = |spec: &str| {
                let (role, _) = spec
                    .split_once(':')
                    .unwrap_or_else(|| panic!("{id}: bad spec {spec}"));
                assert!(roles.contains(&role), "{id}: {spec} names unknown role");
                assert!(seen.insert(spec.to_string()), "{id}: {spec} appears twice");
            };
            for pins in tmpl["nets"].as_object().unwrap().values() {
                for pin in pins.as_array().unwrap() {
                    role_pins(pin.as_str().unwrap());
                }
            }
            for pin in tmpl["no_connect"].as_array().into_iter().flatten() {
                role_pins(pin.as_str().unwrap());
            }
            for net in tmpl["power_nets"].as_array().into_iter().flatten() {
                assert!(
                    tmpl["nets"].get(net.as_str().unwrap()).is_some(),
                    "{id}: power net {net} undefined"
                );
            }
        }
    }

    /// With KiCad's libraries available (KICAD_SYMBOLS / KICAD_FOOTPRINTS),
    /// every lib_id, footprint and pin number must exist. Skips otherwise.
    #[test]
    fn pro_templates_resolve_against_kicad_libraries() {
        let (Ok(symbols), Ok(footprints)) = (
            std::env::var("KICAD_SYMBOLS"),
            std::env::var("KICAD_FOOTPRINTS"),
        ) else {
            eprintln!("skipping: set KICAD_SYMBOLS and KICAD_FOOTPRINTS to KiCad 10 library roots");
            return;
        };
        let read_symbol = |lib_id: &str| -> String {
            let (lib, sym) = lib_id.split_once(':').unwrap();
            std::fs::read_to_string(format!("{symbols}/{lib}.kicad_symdir/{sym}.kicad_sym"))
                .unwrap_or_else(|_| panic!("symbol {lib_id} not found"))
        };
        let pin_numbers = |lib_id: &str| -> std::collections::BTreeSet<String> {
            let mut text = read_symbol(lib_id);
            if let Some(i) = text.find("(extends \"") {
                let rest = &text[i + 10..];
                let base = &rest[..rest.find('"').unwrap()];
                let lib = lib_id.split_once(':').unwrap().0;
                text = read_symbol(&format!("{lib}:{base}"));
            }
            text.match_indices("(number \"")
                .map(|(i, m)| {
                    let rest = &text[i + m.len()..];
                    rest[..rest.find('"').unwrap()].to_string()
                })
                .collect()
        };
        for tmpl in pro_templates() {
            let id = tmpl["id"].as_str().unwrap();
            let mut role_lib = std::collections::BTreeMap::new();
            for c in tmpl["components"].as_array().unwrap() {
                let lib_id = c["lib_id"].as_str().unwrap();
                role_lib.insert(c["role"].as_str().unwrap().to_string(), pin_numbers(lib_id));
                if let Some(fp) = c["footprint"].as_str() {
                    let (lib, name) = fp.split_once(':').unwrap();
                    assert!(
                        std::path::Path::new(&format!(
                            "{footprints}/{lib}.pretty/{name}.kicad_mod"
                        ))
                        .is_file(),
                        "{id}: footprint {fp} not found"
                    );
                }
            }
            let specs = tmpl["nets"]
                .as_object()
                .unwrap()
                .values()
                .flat_map(|p| p.as_array().unwrap().clone())
                .chain(tmpl["no_connect"].as_array().cloned().unwrap_or_default());
            for spec in specs {
                let (role, pin) = spec.as_str().unwrap().split_once(':').unwrap();
                assert!(
                    role_lib[role].contains(pin),
                    "{id}: {role} has no pin {pin}"
                );
            }
        }
        for tmpl in builtin_templates() {
            for c in tmpl["components"].as_array().unwrap() {
                read_symbol(c["lib_id"].as_str().unwrap());
            }
        }
    }
}
