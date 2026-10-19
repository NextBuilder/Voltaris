//! `firmware` toolset — derive firmware pin maps and a bring-up sketch from the
//! schematic, so the code that runs on the board is generated from the same
//! netlist the board is built from.

use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::sch_connectivity::net_graph_for;
use crate::tools::{get_path, invalid_arg, opt_str, placed_pins_by_reference, ToolDef};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use voltaris_sexp::schematic::{
    extract_all_net_labels, extract_symbol_instances, extract_wires, pin_endpoint, read_schematic,
};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Chip {
    Esp32,
    Esp32S3,
    Esp32C3,
    Esp32C6,
    Rp2040,
    Rp2350,
    Stm32,
    Nrf52,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Family {
    Esp,
    Rp,
    Stm32,
    Nrf,
}

impl Chip {
    pub fn family(self) -> Family {
        match self {
            Chip::Esp32 | Chip::Esp32S3 | Chip::Esp32C3 | Chip::Esp32C6 => Family::Esp,
            Chip::Rp2040 | Chip::Rp2350 => Family::Rp,
            Chip::Stm32 => Family::Stm32,
            Chip::Nrf52 => Family::Nrf,
        }
    }
}

impl Family {
    pub fn frameworks(self) -> &'static [&'static str] {
        match self {
            Family::Esp => &["arduino", "esp-idf"],
            Family::Rp => &["arduino", "pico-sdk"],
            Family::Stm32 => &["arduino", "stm32-hal"],
            Family::Nrf => &["nrfx"],
        }
    }
}

/// A GPIO: `port` is 0 for single-bank parts (ESP32, RP2040/RP2350), the
/// letter index for STM32 (A = 0) and the port number for nRF52.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PinId {
    pub port: u8,
    pub num: u8,
}

impl PinId {
    pub fn gpio(num: u8) -> Self {
        PinId { port: 0, num }
    }
    pub fn label(self, family: Family) -> String {
        match family {
            Family::Esp | Family::Rp => format!("GPIO{}", self.num),
            Family::Stm32 => format!("P{}{}", (b'A' + self.port) as char, self.num),
            Family::Nrf => format!("P{}.{:02}", self.port, self.num),
        }
    }
}

pub struct Profile {
    pub name: &'static str,
    pub family: Family,
    pub uart0: (u8, u8),
    pub usb: Option<(u8, u8)>,
    pub strapping: &'static [u8],
    pub input_only: &'static [u8],
    pub adc1: &'static [u8],
    pub adc2: &'static [u8],
    pub caution: &'static [(u8, &'static str)],
}

pub fn profile(chip: Chip) -> Profile {
    let base = |name, family| Profile {
        name,
        family,
        uart0: (255, 255),
        usb: None,
        strapping: &[],
        input_only: &[],
        adc1: &[],
        adc2: &[],
        caution: &[],
    };
    match chip {
        Chip::Esp32 => Profile {
            name: "ESP32",
            family: Family::Esp,
            uart0: (1, 3),
            usb: None,
            strapping: &[0, 2, 5, 12, 15],
            input_only: &[34, 35, 36, 37, 38, 39],
            adc1: &[32, 33, 34, 35, 36, 37, 38, 39],
            adc2: &[0, 2, 4, 12, 13, 14, 15, 25, 26, 27],
            caution: &[
                (12, "MTDI strapping sets flash voltage: if pulled high at reset, 3.3 V flash will not boot"),
                (6, "connected to the module's SPI flash — do not use"),
                (7, "connected to the module's SPI flash — do not use"),
                (8, "connected to the module's SPI flash — do not use"),
                (9, "connected to the module's SPI flash — do not use"),
                (10, "connected to the module's SPI flash — do not use"),
                (11, "connected to the module's SPI flash — do not use"),
            ],
        },
        Chip::Esp32S3 => Profile {
            name: "ESP32-S3",
            family: Family::Esp,
            uart0: (43, 44),
            usb: Some((19, 20)),
            strapping: &[0, 3, 45, 46],
            input_only: &[],
            adc1: &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
            adc2: &[11, 12, 13, 14, 15, 16, 17, 18, 19, 20],
            caution: &[
                (45, "strapping: must be low at reset unless the VDD_SPI eFuse is burned (high selects 1.8 V flash supply)"),
                (35, "used by octal PSRAM on R8 module variants"),
                (36, "used by octal PSRAM on R8 module variants"),
                (37, "used by octal PSRAM on R8 module variants"),
                (33, "used by octal flash/PSRAM where present"),
                (34, "used by octal flash/PSRAM where present"),
            ],
        },
        Chip::Esp32C3 => Profile {
            name: "ESP32-C3",
            family: Family::Esp,
            uart0: (21, 20),
            usb: Some((18, 19)),
            strapping: &[2, 8, 9],
            input_only: &[],
            adc1: &[0, 1, 2, 3, 4],
            adc2: &[5],
            caution: &[(11, "VDD_SPI by default; usable as GPIO only after burning the VDD_SPI_AS_GPIO eFuse")],
        },
        Chip::Esp32C6 => Profile {
            name: "ESP32-C6",
            family: Family::Esp,
            uart0: (16, 17),
            usb: Some((12, 13)),
            strapping: &[8, 9, 15, 4, 5],
            input_only: &[],
            adc1: &[0, 1, 2, 3, 4, 5, 6],
            adc2: &[],
            caution: &[],
        },
        Chip::Rp2040 => Profile { adc1: &[26, 27, 28, 29], ..base("RP2040", Family::Rp) },
        Chip::Rp2350 => Profile { adc1: &[26, 27, 28, 29, 40, 41, 42, 43, 44, 45, 46, 47], ..base("RP2350", Family::Rp) },
        Chip::Stm32 => Profile { usb: Some((11, 12)), ..base("STM32", Family::Stm32) },
        Chip::Nrf52 => base("nRF52", Family::Nrf),
    }
}

pub fn detect_chip(lib_id: &str, value: &str) -> Option<Chip> {
    let hay = format!("{lib_id} {value}").to_ascii_uppercase();
    if hay.contains("ESP32-S3") {
        Some(Chip::Esp32S3)
    } else if hay.contains("ESP32-C3") {
        Some(Chip::Esp32C3)
    } else if hay.contains("ESP32-C6") {
        Some(Chip::Esp32C6)
    } else if hay.contains("ESP32-S2") || hay.contains("ESP32-H") || hay.contains("ESP32-P4") {
        None
    } else if hay.contains("ESP32") {
        Some(Chip::Esp32)
    } else if hay.contains("RP2350")
        || hay.contains("RP2354")
        || hay.contains("PICO2")
        || hay.contains("PICO_2")
        || hay.contains("PICO 2")
    {
        Some(Chip::Rp2350)
    } else if hay.contains("RP2040") || hay.contains("RASPBERRYPI_PICO") || hay.contains("RPI_PICO")
    {
        Some(Chip::Rp2040)
    } else if hay.contains("STM32") {
        Some(Chip::Stm32)
    } else if hay.contains("NRF52")
        || hay.contains("MDBT42")
        || hay.contains("MDBT50")
        || hay.contains("BMD-3")
        || hay.contains("E73-")
    {
        Some(Chip::Nrf52)
    } else {
        None
    }
}

fn number_after(name: &str, tag: &str) -> Option<u8> {
    let upper = name.to_ascii_uppercase();
    let mut search = 0;
    while let Some(i) = upper[search..].find(tag) {
        let start = search + i + tag.len();
        let preceded_ok = search + i == 0
            || !upper.as_bytes()[search + i - 1].is_ascii_alphabetic()
            || tag == "GPIO";
        let digits: String = upper[start..]
            .chars()
            .take_while(|c| c.is_ascii_digit())
            .collect();
        if preceded_ok && !digits.is_empty() {
            return digits.parse().ok();
        }
        search = start;
    }
    None
}

fn tokens(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(['/', '-', ' ', ',', '(', ')'])
        .map(|t| t.trim().to_ascii_uppercase())
}

fn stm32_pin(name: &str) -> Option<PinId> {
    tokens(name).find_map(|t| {
        let b = t.as_bytes();
        if b.len() >= 3
            && b.len() <= 4
            && b[0] == b'P'
            && (b'A'..=b'K').contains(&b[1])
            && b[2..].iter().all(u8::is_ascii_digit)
        {
            let num: u8 = t[2..].parse().ok()?;
            (num <= 15).then_some(PinId {
                port: b[1] - b'A',
                num,
            })
        } else {
            None
        }
    })
}

fn nrf_pin(name: &str) -> Option<PinId> {
    tokens(name).find_map(|t| {
        let rest = t.strip_prefix('P')?;
        let (port, num) = rest.split_once('.')?;
        let (port, num): (u8, u8) = (port.parse().ok()?, num.parse().ok()?);
        (port <= 1 && num <= 31).then_some(PinId { port, num })
    })
}

/// Map a module/chip pin name to its GPIO, or None for non-GPIO pins.
pub fn gpio_of(chip: Chip, pin_name: &str) -> Option<PinId> {
    let upper = pin_name.to_ascii_uppercase();
    match chip.family() {
        Family::Stm32 => return stm32_pin(&upper),
        Family::Nrf => return nrf_pin(&upper),
        Family::Rp => {
            return number_after(&upper, "GPIO")
                .or_else(|| {
                    tokens(&upper).find_map(|t| {
                        t.split('_')
                            .next()
                            .and_then(|h| h.strip_prefix("GP"))
                            .and_then(|d| d.parse().ok())
                    })
                })
                .filter(|n| *n <= 47)
                .map(PinId::gpio);
        }
        Family::Esp => {}
    }
    let p = profile(chip);
    if let Some(n) = number_after(&upper, "GPIO").or_else(|| number_after(&upper, "IO")) {
        return Some(PinId::gpio(n));
    }
    let n = match upper.as_str() {
        "TXD0" | "U0TXD" | "TXD" => Some(p.uart0.0),
        "RXD0" | "U0RXD" | "RXD" => Some(p.uart0.1),
        "USB_D-" => p.usb.map(|u| u.0),
        "USB_D+" => p.usb.map(|u| u.1),
        "SENSOR_VP" if chip == Chip::Esp32 => Some(36),
        "SENSOR_VN" if chip == Chip::Esp32 => Some(39),
        "MTMS" if chip != Chip::Esp32 => Some(4),
        "MTDI" if chip != Chip::Esp32 => Some(5),
        "MTCK" if chip != Chip::Esp32 => Some(6),
        "MTDO" if chip != Chip::Esp32 => Some(7),
        _ => None,
    };
    n.map(PinId::gpio)
}

const POWER_NETS: &[&str] = &[
    "GND", "GNDA", "GNDD", "VCC", "VDD", "VBUS", "VSYS", "VIN", "+3V3", "+3.3V", "3V3", "+5V",
    "5V", "+1V8", "BAT+", "BAT-", "VBAT",
];

fn is_meaningful(net: &str) -> bool {
    let u = net.to_ascii_uppercase();
    !net.starts_with("Net-(")
        && !net.starts_with("unconnected-")
        && !POWER_NETS.contains(&u.as_str())
}

pub fn macro_name(net: &str) -> String {
    let mut out = String::from("PIN_");
    let mut last_us = true;
    for c in net.trim_start_matches(['/', '+', '~']).chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
            last_us = false;
        } else if !last_us {
            out.push('_');
            last_us = true;
        }
    }
    while out.ends_with('_') {
        out.pop();
    }
    out
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Role {
    I2cSda,
    I2cScl,
    Pixel,
    Led,
    Button,
    Analog,
    UsbNative,
    Uart,
    Spi,
    Other,
}

pub fn infer_role(net: &str, id: PinId, p: &Profile) -> Role {
    let u = net.to_ascii_uppercase();
    let has = |s: &str| u.contains(s);
    if id.port == 0 && p.usb.is_some_and(|(dm, dp)| id.num == dm || id.num == dp) && has("USB") {
        Role::UsbNative
    } else if has("SDA") {
        Role::I2cSda
    } else if has("SCL") && !has("SCLK") {
        Role::I2cScl
    } else if has("WS2812")
        || has("NEOPIXEL")
        || has("PIXEL")
        || has("LED_DATA")
        || has("LED_DIN")
        || has("RGB_DATA")
    {
        Role::Pixel
    } else if has("LED") {
        Role::Led
    } else if has("BTN") || has("BUTTON") || has("BOOT") || has("KEY") || u.starts_with("SW") {
        Role::Button
    } else if has("ADC") || has("SENSE") || has("VBAT") || has("_MON") || has("AIN") {
        Role::Analog
    } else if has("TX") || has("RX") {
        Role::Uart
    } else if has("MOSI") || has("MISO") || has("SCK") || has("SCLK") || has("_CS") || u == "CS" {
        Role::Spi
    } else {
        Role::Other
    }
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::I2cSda => "I2C SDA",
            Role::I2cScl => "I2C SCL",
            Role::Pixel => "addressable LED data",
            Role::Led => "LED output",
            Role::Button => "button input",
            Role::Analog => "analog input",
            Role::UsbNative => "native USB",
            Role::Uart => "UART",
            Role::Spi => "SPI",
            Role::Other => "GPIO",
        }
    }
}

pub struct PinUse {
    pub id: PinId,
    pub net: String,
    pub pin_name: String,
    pub role: Role,
}

fn drives(role: Role) -> bool {
    matches!(
        role,
        Role::Led | Role::Pixel | Role::Uart | Role::Spi | Role::I2cSda | Role::I2cScl
    )
}

/// Cautions that follow from the pin itself on STM32 and nRF52 parts.
fn pin_cautions(p: &Profile, u: &PinUse) -> Vec<String> {
    let name = u.pin_name.to_ascii_uppercase();
    let label = u.id.label(p.family);
    let mut w = Vec::new();
    match p.family {
        Family::Stm32 => {
            let msg = match (u.id.port, u.id.num) {
                (0, 13) => Some("SWDIO — using it as GPIO disables SWD programming and debugging after boot; keep it free"),
                (0, 14) => Some("SWCLK — using it as GPIO disables SWD programming and debugging after boot; keep it free (on STM32G0/C0 it is also BOOT0)"),
                (0, 15) => Some("JTDI after reset — free only once JTAG is disabled (SWD-only remap); has an internal pull-up at reset"),
                (1, 3) => Some("JTDO/SWO after reset — free only once JTAG is disabled; also the SWO trace pin"),
                (1, 4) => Some("NJTRST after reset — free only once JTAG is disabled; has an internal pull-up at reset"),
                (1, 2) => Some("BOOT1 strap on STM32F1/F4 families — its level at reset selects the boot memory"),
                (2, 13..=15) => Some("backup-domain pin: weak drive (about 3 mA, 2 MHz) and only one of PC13–PC15 may sink current at a time — not for LEDs or fast signals"),
                (7, 0) | (7, 1) => Some("HSE crystal pins (OSC_IN/OSC_OUT) on most packages"),
                (5, 0) | (5, 1) => Some("HSE crystal pins (OSC_IN/OSC_OUT) on STM32F0/G0/C0/L0 parts"),
                _ => None,
            };
            if let Some(m) = msg {
                w.push(format!("{label} ({}): {m}", u.net));
            }
            if u.role == Role::Analog && !name.contains("ADC") {
                w.push(format!("{label} ({}): confirm this pin has an ADC channel (ADCx_INy) in the part's pinout table", u.net));
            }
        }
        Family::Nrf => {
            if name.contains("NFC") {
                w.push(format!("{label} ({}): NFC antenna pin — usable as GPIO only after UICR.NFCPINS is set to disabled in firmware; the two NFC pins leak into each other when driven to different levels", u.net));
            }
            if name.contains("RESET") {
                w.push(format!("{label} ({}): pin reset — usable as GPIO only while UICR.PSELRESET is not configured (Arduino/Zephyr images often enable it)", u.net));
            }
            if name.contains("XL1") || name.contains("XL2") {
                w.push(format!(
                    "{label} ({}): 32.768 kHz crystal pin — unavailable when the LFXO is fitted",
                    u.net
                ));
            }
            if u.role == Role::Analog && !name.contains("AIN") {
                w.push(format!(
                    "{label} ({}): not an analog input — the SAADC only reaches AIN0–AIN7 pins",
                    u.net
                ));
            }
        }
        Family::Rp => {
            if p.name == "RP2350" && u.role == Role::Button {
                w.push(format!("{label} ({}): RP2350 erratum E9 — on early silicon an input relying on the internal pull-down can latch high; wire buttons active-low with a pull-up, or fit an external pull-down of 8.2 kΩ or less", u.net));
            }
        }
        Family::Esp => {}
    }
    w
}

/// RP2040/RP2350 I2C: SDA on GPIO 4k (I2C0) or 4k+2 (I2C1), SCL on 4k+1 / 4k+3.
pub fn rp_i2c_controller(sda: u8, scl: u8) -> Result<u8, String> {
    let ctrl = |n: u8, base: u8| match n % 4 {
        x if x == base => Some(0),
        x if x == base + 2 => Some(1),
        _ => None,
    };
    match (ctrl(sda, 0), ctrl(scl, 1)) {
        (Some(a), Some(b)) if a == b => Ok(a),
        (Some(a), Some(b)) => Err(format!("SDA GPIO{sda} is on I2C{a} but SCL GPIO{scl} is on I2C{b}: both must use the same controller")),
        (None, _) => Err(format!("GPIO{sda} cannot be I2C SDA (SDA is on GPIO 0, 2, 4, 6 … — GPIO number ≡ 0 or 2 mod 4)")),
        (_, None) => Err(format!("GPIO{scl} cannot be I2C SCL (SCL is on GPIO 1, 3, 5, 7 … — GPIO number ≡ 1 or 3 mod 4)")),
    }
}

pub fn warnings_for(uses: &[PinUse], p: &Profile) -> Vec<String> {
    let mut w = Vec::new();
    for u in uses {
        let label = u.id.label(p.family);
        let on0 = |list: &[u8]| u.id.port == 0 && list.contains(&u.id.num);
        let caution = if u.id.port == 0 {
            p.caution.iter().find(|(g, _)| *g == u.id.num)
        } else {
            None
        };
        if on0(p.strapping) && u.role != Role::Button && caution.is_none() {
            w.push(format!(
                "{label} ({}) is a strapping pin: whatever drives it must leave the boot level intact at reset",
                u.net
            ));
        }
        if on0(p.input_only) && drives(u.role) {
            w.push(format!(
                "{label} ({}) is input-only and cannot drive a {}",
                u.net,
                u.role.label()
            ));
        }
        if u.role == Role::Analog && matches!(p.family, Family::Esp | Family::Rp) && !on0(p.adc1) {
            if on0(p.adc2) {
                w.push(format!("{label} ({}) is on ADC2, which is unavailable or unreliable while Wi-Fi runs — move analog sensing to an ADC1 pin", u.net));
            } else {
                w.push(format!("{label} ({}) has no ADC channel", u.net));
            }
        }
        if let Some((_, msg)) = caution {
            w.push(format!("{label} ({}): {msg}", u.net));
        }
        if on0(&p.usb.map(|(a, b)| vec![a, b]).unwrap_or_default()) && u.role != Role::UsbNative {
            w.push(format!(
                "{label} ({}) is a native-USB pin: using it as GPIO disables {}",
                u.net,
                if p.family == Family::Esp {
                    "USB flashing/CDC"
                } else {
                    "the USB peripheral"
                }
            ));
        }
        w.extend(pin_cautions(p, u));
    }
    if p.family == Family::Rp {
        let sda = uses.iter().find(|u| u.role == Role::I2cSda);
        let scl = uses.iter().find(|u| u.role == Role::I2cScl);
        if let (Some(a), Some(b)) = (sda, scl) {
            if let Err(e) = rp_i2c_controller(a.id.num, b.id.num) {
                w.push(format!("I2C ({} / {}): {e}", a.net, b.net));
            }
        }
    }
    w
}

fn pin_value(u: &PinUse, p: &Profile, framework: &str) -> String {
    match (p.family, framework) {
        (Family::Esp, "esp-idf") => format!("GPIO_NUM_{}", u.id.num),
        (Family::Stm32, "arduino") => u.id.label(Family::Stm32),
        (Family::Stm32, _) => format!("GPIO{}, GPIO_PIN_{}", (b'A' + u.id.port) as char, u.id.num),
        (Family::Nrf, _) => format!("NRF_GPIO_PIN_MAP({}, {})", u.id.port, u.id.num),
        _ => u.id.num.to_string(),
    }
}

pub fn render_header(uses: &[PinUse], p: &Profile, framework: &str, source: &str) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "// board_pins.h — generated by Voltaris from {source}\n\
         // Target: {} ({framework}). Regenerate after every schematic change.\n#pragma once\n\n",
        p.name
    ));
    match (p.family, framework) {
        (Family::Esp, "esp-idf") => s.push_str("#include \"driver/gpio.h\"\n\n"),
        (Family::Nrf, _) => s.push_str("#include <hal/nrf_gpio.h>\n\n"),
        (Family::Stm32, "stm32-hal") => s.push_str(
            "// Include after your HAL header (e.g. \"main.h\" or \"stm32g0xx_hal.h\").\n\n",
        ),
        _ => {}
    }
    let width = uses
        .iter()
        .map(|u| macro_name(&u.net).len())
        .max()
        .unwrap_or(8);
    for u in uses {
        let mut note = format!("{} — {}", u.pin_name, u.role.label());
        if u.id.port == 0 && p.strapping.contains(&u.id.num) {
            note.push_str(", strapping");
        }
        let name = macro_name(&u.net);
        if p.family == Family::Stm32 && framework == "stm32-hal" {
            let port = format!("GPIO{}", (b'A' + u.id.port) as char);
            s.push_str(&format!(
                "#define {:<w$} {:<12} // {note}\n",
                format!("{name}_PORT"),
                port,
                w = width + 5
            ));
            s.push_str(&format!(
                "#define {:<w$} GPIO_PIN_{}\n",
                format!("{name}_PIN"),
                u.id.num,
                w = width + 5
            ));
        } else {
            s.push_str(&format!(
                "#define {:<width$} {:<12} // {note}\n",
                name,
                pin_value(u, p, framework)
            ));
        }
    }
    let hal = p.family == Family::Stm32 && framework == "stm32-hal";
    let alias = |role: Role, name: &str, s: &mut String| {
        if hal {
            return;
        }
        if let Some(u) = uses.iter().find(|u| u.role == role) {
            if macro_name(&u.net) != name {
                s.push_str(&format!("#define {name} {}\n", macro_name(&u.net)));
            }
        }
    };
    s.push('\n');
    alias(Role::I2cSda, "PIN_I2C_SDA", &mut s);
    alias(Role::I2cScl, "PIN_I2C_SCL", &mut s);
    alias(Role::Pixel, "PIN_PIXEL", &mut s);
    s
}

fn role_lists(uses: &[PinUse]) -> (String, String, String, String, bool) {
    let by = |r: Role| {
        uses.iter()
            .filter(move |u| u.role == r)
            .map(|u| macro_name(&u.net))
            .collect::<Vec<_>>()
    };
    let list = |v: Vec<String>| {
        if v.is_empty() {
            "-1".to_string()
        } else {
            v.join(", ")
        }
    };
    let has_i2c =
        uses.iter().any(|u| u.role == Role::I2cSda) && uses.iter().any(|u| u.role == Role::I2cScl);
    (
        list(by(Role::Led)),
        list(by(Role::Button)),
        list(by(Role::Analog)),
        list(by(Role::Pixel)),
        has_i2c,
    )
}

pub fn render_sketch(uses: &[PinUse], p: &Profile, project: &str) -> String {
    if p.family != Family::Esp {
        return render_generic_sketch(uses, p, project);
    }
    let (leds, buttons, analog, pixels, has_i2c) = role_lists(uses);
    let mut s = format!(
        "// Bring-up sketch for {project} — generated by Voltaris.\n\
         // Arduino-ESP32 core. Board: {}. Open Serial Monitor at 115200.\n\
         // It only drives pins the schematic names as LEDs or pixel data; every other pin is read.\n\n\
         #include <Arduino.h>\n",
        p.name
    );
    if has_i2c {
        s.push_str("#include <Wire.h>\n");
    }
    s.push_str("#include \"board_pins.h\"\n\n");
    s.push_str(&format!("const int LED_PINS[] = {{{leds}}};\n"));
    s.push_str(&format!("const int BUTTON_PINS[] = {{{buttons}}};\n"));
    s.push_str(&format!("const int ANALOG_PINS[] = {{{analog}}};\n"));
    s.push_str(&format!("const int PIXEL_PINS[] = {{{pixels}}};\n\n"));
    s.push_str(
        "template <size_t N> static bool used(const int (&a)[N]) { return a[0] >= 0; }\n\n\
         static void pixel(int pin, uint8_t r, uint8_t g, uint8_t b) {\n\
         #if defined(ESP_ARDUINO_VERSION_MAJOR) && ESP_ARDUINO_VERSION_MAJOR >= 3\n  rgbLedWrite(pin, r, g, b);\n\
         #else\n  neopixelWrite(pin, r, g, b);\n#endif\n}\n\n",
    );
    s.push_str("void setup() {\n  Serial.begin(115200);\n  delay(1500);\n");
    s.push_str(&format!(
        "  Serial.println(\"\\n== {project} bring-up ==\");\n"
    ));
    s.push_str(
        "  Serial.printf(\"Chip %s rev %d, %d cores, flash %u MB, PSRAM %u KB\\n\", ESP.getChipModel(),\n\
         \x20               ESP.getChipRevision(), ESP.getChipCores(), ESP.getFlashChipSize() >> 20, ESP.getPsramSize() >> 10);\n",
    );
    if has_i2c {
        s.push_str(
            "  Wire.begin(PIN_I2C_SDA, PIN_I2C_SCL);\n  int found = 0;\n  for (uint8_t a = 1; a < 127; a++) {\n\
             \x20   Wire.beginTransmission(a);\n    if (Wire.endTransmission() == 0) { Serial.printf(\"I2C device at 0x%02X\\n\", a); found++; }\n  }\n\
             \x20 Serial.printf(\"I2C scan: %d device(s)%s\\n\", found, found ? \"\" : \" — check pull-ups and power\");\n",
        );
    }
    s.push_str(
        "  if (used(LED_PINS)) for (int pin : LED_PINS) {\n    pinMode(pin, OUTPUT);\n    for (int i = 0; i < 3; i++) { digitalWrite(pin, HIGH); delay(150); digitalWrite(pin, LOW); delay(150); }\n\
         \x20   Serial.printf(\"LED on GPIO%d blinked 3x\\n\", pin);\n  }\n\
         \x20 if (used(PIXEL_PINS)) for (int pin : PIXEL_PINS) {\n    pixel(pin, 32, 0, 0); delay(300); pixel(pin, 0, 32, 0); delay(300);\n\
         \x20   pixel(pin, 0, 0, 32); delay(300); pixel(pin, 0, 0, 0);\n    Serial.printf(\"Pixel on GPIO%d: red, green, blue\\n\", pin);\n  }\n\
         \x20 if (used(BUTTON_PINS)) for (int pin : BUTTON_PINS) pinMode(pin, INPUT_PULLUP);\n\
         \x20 Serial.println(\"Setup done. Press buttons / vary inputs; readings follow every second.\");\n}\n\n\
         void loop() {\n\
         \x20 if (used(BUTTON_PINS)) for (int pin : BUTTON_PINS) Serial.printf(\"BTN GPIO%d=%s  \", pin, digitalRead(pin) ? \"up\" : \"PRESSED\");\n\
         \x20 if (used(ANALOG_PINS)) for (int pin : ANALOG_PINS) Serial.printf(\"ADC GPIO%d=%u mV  \", pin, analogReadMilliVolts(pin));\n\
         \x20 Serial.println();\n  delay(1000);\n}\n",
    );
    s
}

/// Arduino bring-up sketch for the arduino-pico (RP2040/RP2350) and
/// STM32duino cores. nRF52 has no portable Arduino pin numbering, so it gets
/// the header only.
pub fn render_generic_sketch(uses: &[PinUse], p: &Profile, project: &str) -> String {
    if p.family == Family::Nrf {
        return String::new();
    }
    let (leds, buttons, analog, pixels, has_i2c) = role_lists(uses);
    let core = if p.family == Family::Rp {
        "arduino-pico (Earle Philhower) core"
    } else {
        "STM32duino core"
    };
    let wire = match (
        p.family,
        uses.iter().find(|u| u.role == Role::I2cSda),
        uses.iter().find(|u| u.role == Role::I2cScl),
    ) {
        (Family::Rp, Some(a), Some(b)) => match rp_i2c_controller(a.id.num, b.id.num) {
            Ok(1) => "Wire1",
            _ => "Wire",
        },
        _ => "Wire",
    };
    let mut s = format!(
        "// Bring-up sketch for {project} — generated by Voltaris.\n\
         // {core}. Target: {}. Open Serial Monitor at 115200.\n\
         // It only drives pins the schematic names as LEDs or pixel data; every other pin is read.\n\
         // Pixel test needs the Adafruit NeoPixel library; without it that test is skipped.\n\n\
         #include <Arduino.h>\n#include <stdarg.h>\n",
        p.name
    );
    if has_i2c {
        s.push_str("#include <Wire.h>\n");
    }
    s.push_str("#if __has_include(<Adafruit_NeoPixel.h>)\n#include <Adafruit_NeoPixel.h>\n#define HAVE_NEOPIXEL 1\n#endif\n#include \"board_pins.h\"\n\n");
    s.push_str(&format!("const int LED_PINS[] = {{{leds}}};\n"));
    s.push_str(&format!("const int BUTTON_PINS[] = {{{buttons}}};\n"));
    s.push_str(&format!("const int ANALOG_PINS[] = {{{analog}}};\n"));
    s.push_str(&format!("const int PIXEL_PINS[] = {{{pixels}}};\n\n"));
    s.push_str(
        "template <size_t N> static bool used(const int (&a)[N]) { return a[0] >= 0; }\n\n\
         static void say(const char *fmt, ...) {\n  char buf[160];\n  va_list ap;\n  va_start(ap, fmt);\n  vsnprintf(buf, sizeof buf, fmt, ap);\n  va_end(ap);\n  Serial.print(buf);\n}\n\n",
    );
    s.push_str("void setup() {\n  Serial.begin(115200);\n  unsigned long t0 = millis();\n  while (!Serial && millis() - t0 < 3000) {}\n");
    s.push_str(&format!("  Serial.println(\"\\n== {project} bring-up ==\");\n  say(\"CPU clock %lu MHz\\n\", (unsigned long)(F_CPU / 1000000UL));\n"));
    s.push_str("  analogReadResolution(12);\n");
    if has_i2c {
        let set = if p.family == Family::Rp {
            format!(
                "  {wire}.setSDA(PIN_I2C_SDA);\n  {wire}.setSCL(PIN_I2C_SCL);\n  {wire}.begin();\n"
            )
        } else {
            "  Wire.setSDA(PIN_I2C_SDA);\n  Wire.setSCL(PIN_I2C_SCL);\n  Wire.begin();\n"
                .to_string()
        };
        s.push_str(&set);
        s.push_str(&format!(
            "  int found = 0;\n  for (uint8_t a = 1; a < 127; a++) {{\n    {wire}.beginTransmission(a);\n    if ({wire}.endTransmission() == 0) {{ say(\"I2C device at 0x%02X\\n\", a); found++; }}\n  }}\n\
             \x20 say(\"I2C scan: %d device(s)%s\\n\", found, found ? \"\" : \" — check pull-ups and power\");\n"
        ));
    }
    s.push_str(
        "  if (used(LED_PINS)) for (int pin : LED_PINS) {\n    pinMode(pin, OUTPUT);\n    for (int i = 0; i < 3; i++) { digitalWrite(pin, HIGH); delay(150); digitalWrite(pin, LOW); delay(150); }\n\
         \x20   say(\"LED on pin %d blinked 3x\\n\", pin);\n  }\n\
         #ifdef HAVE_NEOPIXEL\n\
         \x20 if (used(PIXEL_PINS)) for (int pin : PIXEL_PINS) {\n    Adafruit_NeoPixel px(1, pin, NEO_GRB + NEO_KHZ800);\n    px.begin();\n\
         \x20   const uint32_t colours[] = {px.Color(32, 0, 0), px.Color(0, 32, 0), px.Color(0, 0, 32), 0};\n\
         \x20   for (uint32_t c : colours) { px.setPixelColor(0, c); px.show(); delay(300); }\n    say(\"Pixel on pin %d: red, green, blue\\n\", pin);\n  }\n\
         #else\n  if (used(PIXEL_PINS)) Serial.println(\"Pixel test skipped: install Adafruit NeoPixel\");\n#endif\n\
         \x20 if (used(BUTTON_PINS)) for (int pin : BUTTON_PINS) pinMode(pin, INPUT_PULLUP);\n\
         \x20 Serial.println(\"Setup done. Press buttons / vary inputs; readings follow every second.\");\n}\n\n\
         void loop() {\n\
         \x20 if (used(BUTTON_PINS)) for (int pin : BUTTON_PINS) say(\"BTN %d=%s  \", pin, digitalRead(pin) ? \"up\" : \"PRESSED\");\n\
         \x20 if (used(ANALOG_PINS)) for (int pin : ANALOG_PINS) say(\"ADC %d=%d/4095  \", pin, analogRead(pin));\n\
         \x20 Serial.println();\n  delay(1000);\n}\n",
    );
    s
}

pub fn tools() -> Vec<ToolDef> {
    vec![tool!(
        "generate_firmware_kit",
        "Generate firmware from the schematic: a board_pins.h with one #define per named net on the \
         MCU's GPIOs, and an Arduino bring-up sketch that blinks LEDs, tests WS2812 pixels, scans I2C, \
         reads buttons and ADC nets — inferred from net names. Supports ESP32 / -S3 / -C3 / -C6 \
         (arduino, esp-idf), RP2040 and RP2350 (arduino-pico, pico-sdk), STM32 (STM32duino 'PA5' \
         names, or stm32-hal _PORT/_PIN pairs) and nRF52 (nrfx NRF_GPIO_PIN_MAP; header only). \
         Audits the pin assignment: ESP32 strapping, input-only, ADC2-with-Wi-Fi, USB, flash and PSRAM \
         pins; RP I2C controller pairing and ADC pins; STM32 SWD/JTAG, BOOT1, backup-domain and HSE \
         pins; nRF52 NFC, pin-reset, 32 kHz crystal and AIN pins. Writes files only when output_dir \
         is given and never overwrites unless overwrite is true.",
        json!({
            "type": "object",
            "properties": {
                "schematic": { "type": "string", "description": "Path to the .kicad_sch holding the MCU" },
                "mcu_reference": { "type": "string", "description": "MCU reference (e.g. U3). Auto-detected when the sheet has exactly one supported MCU." },
                "framework": { "type": "string", "enum": ["arduino", "esp-idf", "pico-sdk", "stm32-hal", "nrfx"], "description": "Default: arduino (nrfx for nRF52)" },
                "output_dir": { "type": "string", "description": "Directory to write board_pins.h and bringup/bringup.ino into" },
                "overwrite": { "type": "boolean", "default": false }
            },
            "required": ["schematic"]
        }),
        |args, _ctx| async move { handle_firmware(args) }
    )]
}

fn handle_firmware(args: &Value) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let candidates: Vec<_> = instances
        .iter()
        .filter_map(|i| detect_chip(&i.lib_id, &i.value).map(|c| (i, c)))
        .collect();
    let (instance, chip) = match opt_str(args, "mcu_reference") {
        Some(r) => match instances.iter().find(|i| i.reference == r) {
            Some(i) => match detect_chip(&i.lib_id, &i.value) {
                Some(c) => (i, c),
                None => return Ok(invalid_arg("mcu_reference", &format!("{r} ({}) is not a supported MCU (ESP32, -S3, -C3, -C6, RP2040, RP2350, STM32, nRF52)", i.lib_id))),
            },
            None => return Ok(invalid_arg("mcu_reference", &format!("no symbol {r} on this sheet"))),
        },
        None => match candidates.as_slice() {
            [one] => *one,
            [] => return Ok(CallToolResult::error("No supported MCU symbol (ESP32, RP2040/RP2350, STM32, nRF52) on this sheet; pass the schematic sheet that holds the MCU")),
            many => {
                let refs: Vec<_> = many.iter().map(|(i, _)| i.reference.clone()).collect();
                return Ok(invalid_arg("mcu_reference", &format!("several MCU symbols ({}); choose one", refs.join(", "))));
            }
        },
    };
    let p = profile(chip);
    let allowed = p.family.frameworks();
    let framework = opt_str(args, "framework").unwrap_or(allowed[0]);
    if !allowed.contains(&framework) {
        return Ok(invalid_arg(
            "framework",
            &format!("{} supports {}", p.name, allowed.join(", ")),
        ));
    }
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let mut graph = net_graph_for(&tree, &wires, &labels);
    let mut by_gpio: BTreeMap<PinId, PinUse> = BTreeMap::new();
    let mut unnamed = Vec::new();
    for (inst, pins) in placed_pins_by_reference(&tree) {
        if inst.reference != instance.reference {
            continue;
        }
        for (pin, t) in pins {
            let Some(id) = gpio_of(chip, &pin.name) else {
                continue;
            };
            let (x, y) = pin_endpoint(&pin, t);
            match graph.net_at(x, y) {
                Some(net) if is_meaningful(&net) => {
                    let role = infer_role(&net, id, &p);
                    by_gpio.entry(id).or_insert(PinUse {
                        id,
                        net,
                        pin_name: pin.name.clone(),
                        role,
                    });
                }
                Some(net) if net.starts_with("Net-(") => {
                    unnamed.push(format!("{} ({net})", id.label(p.family)))
                }
                _ => {}
            }
        }
    }
    let uses: Vec<PinUse> = by_gpio.into_values().collect();
    let warnings = warnings_for(&uses, &p);
    let source = sch_path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let project = sch_path
        .file_stem()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_else(|| "board".into());
    let header = render_header(&uses, &p, framework, &source);
    let sketch = if p.family == Family::Stm32 && framework != "arduino" {
        String::new()
    } else {
        render_sketch(&uses, &p, &project)
    };
    let mut written = Vec::new();
    if let Some(dir) = opt_str(args, "output_dir") {
        let dir = std::path::PathBuf::from(dir);
        let overwrite = args["overwrite"].as_bool().unwrap_or(false);
        let mut targets = vec![(dir.join("board_pins.h"), &header)];
        if !sketch.is_empty() {
            targets.push((dir.join("bringup").join("bringup.ino"), &sketch));
            targets.push((dir.join("bringup").join("board_pins.h"), &header));
        }
        if !overwrite {
            if let Some((path, _)) = targets.iter().find(|(p, _)| p.exists()) {
                return Ok(CallToolResult::error(format!(
                    "{} already exists; pass overwrite: true to replace it",
                    path.display()
                )));
            }
        }
        for (path, text) in &targets {
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, text.as_bytes())?;
            written.push(path.display().to_string());
        }
    }
    Ok(CallToolResult::json(&json!({
        "mcu": { "reference": instance.reference, "lib_id": instance.lib_id, "part": instance.value, "chip": p.name, "framework": framework },
        "pins": uses.iter().map(|u| json!({ "pin": u.id.label(p.family), "net": u.net, "macro": macro_name(&u.net), "value": pin_value(u, &p, framework), "pin_name": u.pin_name, "role": u.role.label() })).collect::<Vec<_>>(),
        "warnings": warnings,
        "unnamed_nets": unnamed,
        "unnamed_note": if unnamed.is_empty() { Value::Null } else { json!("These GPIOs connect to unnamed nets; label them in the schematic to get a #define") },
        "board_pins_h": header,
        "bringup_ino": if sketch.is_empty() { json!(null) } else { json!(sketch) },
        "bringup_note": match (sketch.is_empty(), p.family) {
            (true, Family::Nrf) => json!("nRF52 Arduino cores number pins per board variant, so only board_pins.h (nrfx) is generated"),
            (true, _) => json!("The bring-up sketch targets STM32duino; run again with framework 'arduino' to get it"),
            _ => json!(null),
        },
        "files_written": written,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(chip: Chip, name: &str) -> Option<(u8, u8)> {
        gpio_of(chip, name).map(|p| (p.port, p.num))
    }

    #[test]
    fn pin_names_map_to_gpio_numbers() {
        assert_eq!(g(Chip::Esp32S3, "IO48"), Some((0, 48)));
        assert_eq!(g(Chip::Esp32S3, "TXD0"), Some((0, 43)));
        assert_eq!(g(Chip::Esp32S3, "USB_D+"), Some((0, 20)));
        assert_eq!(g(Chip::Esp32C3, "IO20/RXD"), Some((0, 20)));
        assert_eq!(g(Chip::Esp32C3, "IO21/TXD"), Some((0, 21)));
        assert_eq!(g(Chip::Esp32C3, "GPIO9"), Some((0, 9)));
        assert_eq!(g(Chip::Esp32C3, "MTDO"), Some((0, 7)));
        assert_eq!(g(Chip::Esp32C6, "RXD0"), Some((0, 17)));
        assert_eq!(g(Chip::Esp32, "RXD0/IO3"), Some((0, 3)));
        assert_eq!(g(Chip::Esp32, "SENSOR_VN"), Some((0, 39)));
        for n in ["3V3", "VDD"] {
            assert_eq!(g(Chip::Esp32, n), None);
        }
        assert_eq!(g(Chip::Esp32S3, "EN"), None);
        assert_eq!(g(Chip::Esp32C3, "VDD3P3"), None);
        assert_eq!(g(Chip::Esp32C3, "VDD_SPI"), None);
    }

    #[test]
    fn new_families_parse_their_pin_names() {
        assert_eq!(g(Chip::Rp2040, "GPIO26_ADC0"), Some((0, 26)));
        assert_eq!(g(Chip::Rp2040, "GPIO26/ADC0"), Some((0, 26)));
        assert_eq!(g(Chip::Rp2040, "GP15"), Some((0, 15)));
        assert_eq!(g(Chip::Rp2040, "GP26_A0"), Some((0, 26)));
        assert_eq!(g(Chip::Rp2040, "USB_DP"), None);
        assert_eq!(g(Chip::Rp2040, "QSPI_SS"), None);
        assert_eq!(g(Chip::Rp2040, "ADC_AVDD"), None);
        assert_eq!(g(Chip::Rp2350, "GPIO47_ADC7"), Some((0, 47)));
        assert_eq!(g(Chip::Stm32, "PA13"), Some((0, 13)));
        assert_eq!(g(Chip::Stm32, "PC14-OSC32_IN"), Some((2, 14)));
        assert_eq!(g(Chip::Stm32, "PH0-OSC_IN"), Some((7, 0)));
        assert_eq!(g(Chip::Stm32, "PA14-BOOT0"), Some((0, 14)));
        assert_eq!(g(Chip::Stm32, "PB8/BOOT0"), Some((1, 8)));
        for n in ["VDDA", "NRST", "BOOT0", "PDR_ON", "VBAT"] {
            assert_eq!(g(Chip::Stm32, n), None, "{n}");
        }
        assert_eq!(g(Chip::Nrf52, "P0.21/~{RESET}"), Some((0, 21)));
        assert_eq!(g(Chip::Nrf52, "P1.09"), Some((1, 9)));
        assert_eq!(g(Chip::Nrf52, "P0.02/AIN0"), Some((0, 2)));
        assert_eq!(g(Chip::Nrf52, "SWDIO"), None);
        assert_eq!(g(Chip::Nrf52, "DEC1"), None);
        assert_eq!(PinId { port: 2, num: 13 }.label(Family::Stm32), "PC13");
        assert_eq!(PinId { port: 1, num: 9 }.label(Family::Nrf), "P1.09");
    }

    #[test]
    fn chips_detected_from_library_ids() {
        assert_eq!(
            detect_chip("RF_Module:ESP32-S3-WROOM-1", ""),
            Some(Chip::Esp32S3)
        );
        assert_eq!(
            detect_chip("RF_Module:ESP32-C6-MINI-1", ""),
            Some(Chip::Esp32C6)
        );
        assert_eq!(
            detect_chip("RF_Module:ESP32-WROOM-32E", ""),
            Some(Chip::Esp32)
        );
        assert_eq!(detect_chip("RF_Module:ESP32-S2-WROVER", ""), None);
        assert_eq!(detect_chip("Device:R", "10k"), None);
        assert_eq!(
            detect_chip("MCU_RaspberryPi:RP2040", ""),
            Some(Chip::Rp2040)
        );
        assert_eq!(
            detect_chip("MCU_RaspberryPi:RP2350A", ""),
            Some(Chip::Rp2350)
        );
        assert_eq!(
            detect_chip("MCU_ST_STM32G0:STM32G030F6Px", ""),
            Some(Chip::Stm32)
        );
        assert_eq!(detect_chip("MCU_Nordic:nRF52840", ""), Some(Chip::Nrf52));
        assert_eq!(detect_chip("Custom:MDBT50Q", ""), Some(Chip::Nrf52));
    }

    fn pin(port: u8, num: u8, net: &str, name: &str, role: Role) -> PinUse {
        PinUse {
            id: PinId { port, num },
            net: net.into(),
            pin_name: name.into(),
            role,
        }
    }

    #[test]
    fn audit_flags_adc2_strapping_and_input_only() {
        let p = profile(Chip::Esp32);
        let uses = vec![
            pin(0, 25, "VBAT_SENSE", "IO25", Role::Analog),
            pin(0, 34, "STATUS_LED", "IO34", Role::Led),
            pin(0, 12, "RELAY", "IO12", Role::Other),
        ];
        let w = warnings_for(&uses, &p).join("\n");
        assert!(w.contains("ADC2"));
        assert!(w.contains("input-only"));
        assert!(
            !w.contains("GPIO12 (RELAY) is a strapping pin"),
            "caution replaces the generic note"
        );
        assert!(w.contains("flash voltage"));
    }

    #[test]
    fn audits_for_rp_stm32_and_nrf() {
        let rp = profile(Chip::Rp2040);
        let w = warnings_for(
            &[
                pin(0, 4, "I2C_SDA", "GPIO4", Role::I2cSda),
                pin(0, 7, "I2C_SCL", "GPIO7", Role::I2cScl),
                pin(0, 20, "VBAT_SENSE", "GPIO20", Role::Analog),
            ],
            &rp,
        )
        .join("\n");
        assert!(w.contains("I2C0 but SCL GPIO7 is on I2C1"), "{w}");
        assert!(w.contains("GPIO20 (VBAT_SENSE) has no ADC channel"), "{w}");
        assert_eq!(rp_i2c_controller(2, 3), Ok(1));
        assert_eq!(rp_i2c_controller(16, 17), Ok(0));
        assert!(rp_i2c_controller(1, 2).is_err());
        assert!(warnings_for(
            &[pin(0, 26, "VBAT_SENSE", "GPIO26_ADC0", Role::Analog)],
            &rp
        )
        .is_empty());
        let rp2 = profile(Chip::Rp2350);
        assert!(
            warnings_for(&[pin(0, 9, "BTN_A", "GPIO9", Role::Button)], &rp2)
                .join("")
                .contains("E9")
        );

        let st = profile(Chip::Stm32);
        let w = warnings_for(
            &[
                pin(0, 13, "STATUS_LED", "PA13", Role::Led),
                pin(2, 13, "LED2", "PC13-TAMPER", Role::Led),
                pin(0, 12, "BUZZER", "PA12", Role::Other),
                pin(0, 0, "VBAT_SENSE", "PA0", Role::Analog),
            ],
            &st,
        )
        .join("\n");
        assert!(w.contains("PA13 (STATUS_LED): SWDIO"), "{w}");
        assert!(w.contains("PC13 (LED2): backup-domain"), "{w}");
        assert!(w.contains("PA12 (BUZZER) is a native-USB pin"), "{w}");
        assert!(w.contains("PA0 (VBAT_SENSE): confirm"), "{w}");

        let nrf = profile(Chip::Nrf52);
        let w = warnings_for(
            &[
                pin(0, 9, "LED_R", "P0.09/NFC1", Role::Led),
                pin(0, 18, "BTN", "P0.18/~{RESET}", Role::Button),
                pin(0, 13, "VBAT_SENSE", "P0.13", Role::Analog),
                pin(0, 4, "ADC_IN", "P0.04/AIN2", Role::Analog),
            ],
            &nrf,
        )
        .join("\n");
        assert!(
            w.contains("P0.09 (LED_R): NFC")
                && w.contains("P0.18 (BTN): pin reset")
                && w.contains("P0.13 (VBAT_SENSE): not an analog input"),
            "{w}"
        );
        assert!(!w.contains("P0.04"), "{w}");
    }

    #[test]
    fn headers_per_framework() {
        let st = profile(Chip::Stm32);
        let uses = vec![
            pin(0, 5, "STATUS_LED", "PA5", Role::Led),
            pin(1, 7, "I2C_SDA", "PB7", Role::I2cSda),
            pin(1, 6, "I2C_SCL", "PB6", Role::I2cScl),
        ];
        let a = render_header(&uses, &st, "arduino", "x.kicad_sch");
        assert!(a.contains("#define PIN_STATUS_LED PA5"), "{a}");
        let h = render_header(&uses, &st, "stm32-hal", "x.kicad_sch");
        assert!(
            h.contains("PIN_STATUS_LED_PORT")
                && h.contains("GPIOA")
                && h.contains("#define PIN_STATUS_LED_PIN")
                && h.contains("GPIO_PIN_5"),
            "{h}"
        );
        let nrf = profile(Chip::Nrf52);
        let n = render_header(&[pin(1, 9, "LED", "P1.09", Role::Led)], &nrf, "nrfx", "x");
        assert!(n.contains("NRF_GPIO_PIN_MAP(1, 9)") && n.contains("hal/nrf_gpio.h"));
        assert!(render_sketch(&[], &nrf, "x").is_empty());
        let rp = profile(Chip::Rp2040);
        let sk = render_sketch(
            &[
                pin(0, 2, "SDA", "GPIO2", Role::I2cSda),
                pin(0, 3, "SCL", "GPIO3", Role::I2cScl),
            ],
            &rp,
            "demo",
        );
        assert!(
            sk.contains("Wire1.setSDA(PIN_I2C_SDA)") && sk.contains("analogReadResolution(12)"),
            "{sk}"
        );
    }

    #[test]
    fn roles_and_macros_from_net_names() {
        let p = profile(Chip::Esp32S3);
        let id = PinId::gpio;
        assert_eq!(infer_role("I2C_SDA", id(8), &p), Role::I2cSda);
        assert_eq!(infer_role("SPI_SCLK", id(12), &p), Role::Spi);
        assert_eq!(infer_role("LED_DATA_3V3", id(48), &p), Role::Pixel);
        assert_eq!(infer_role("STATUS_LED", id(2), &p), Role::Led);
        assert_eq!(infer_role("USB_DP_MCU", id(20), &p), Role::UsbNative);
        assert_eq!(infer_role("BOOT", id(0), &p), Role::Button);
        assert_eq!(macro_name("/power/VBAT sense"), "PIN_POWER_VBAT_SENSE");
        assert_eq!(macro_name("LED_DATA_3V3"), "PIN_LED_DATA_3V3");
        assert!(!is_meaningful("+3V3") && !is_meaningful("Net-(R1-Pad1)") && is_meaningful("SDA"));
    }
}
