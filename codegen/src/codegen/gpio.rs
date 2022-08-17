use crate::cubemx::ip::gpio;
use anyhow::{Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;
use std::fmt::Write;

struct Port<'a> {
    id: char,
    pins: Vec<&'a gpio::Pin>,
}

pub fn gen_mappings(gpio_ips: &[gpio::Ip]) -> Result<()> {
    let mut all_macros = Vec::<PortMacro>::new();
    for ip in gpio_ips.iter() {
        println!();
        for m in gen_gpio_ip(ip)?.into_iter() {
            let mut same = None;
            for (i, am) in all_macros.iter().enumerate() {
                if m.string == am.string {
                    same = Some(i);
                    break;
                }
            }
            if let Some(i) = same {
                all_macros[i].features.extend(m.features);
            } else {
                all_macros.push(m);
            }
        }
    }
    let mut allmacros = Vec::<PortMacro>::new();
    for m in all_macros.into_iter() {
        let mut same = None;
        for (i, am) in allmacros.iter().enumerate() {
            if m.features == am.features {
                same = Some(i);
                break;
            }
        }
        if let Some(i) = same {
            allmacros[i].string.push('\n');
            allmacros[i].string.push_str(&m.string);
        } else {
            allmacros.push(m);
        }
    }
    for m in allmacros {
        println!("{m}");
    }
    Ok(())
}

fn gen_gpio_ip(ip: &gpio::Ip) -> Result<Vec<PortMacro>> {
    let feature = ip_version_to_feature(&ip.version)?;
    let ports = merge_pins_by_port(&ip.pins)?;

    let mut macros = Vec::new();
    for port in &ports {
        for string in gen_port(port)?.into_iter() {
            macros.push(PortMacro {
                features: vec![feature.clone()],
                string,
            });
        }
    }
    Ok(macros)
}

fn ip_version_to_feature(ip_version: &str) -> Result<String> {
    static VERSION: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"^STM32(?P<version>\w+)_gpio_v1_0$").unwrap());

    let captures = VERSION
        .captures(ip_version)
        .with_context(|| format!("invalid GPIO IP version: {}", ip_version))?;

    let version = captures.name("version").unwrap().as_str();
    let feature = format!("gpio-{}", version.to_lowercase());
    Ok(feature)
}

fn merge_pins_by_port(pins: &[gpio::Pin]) -> Result<Vec<Port>> {
    let mut pins_by_port = HashMap::new();
    for pin in pins.iter() {
        pins_by_port
            .entry(pin.port()?)
            .and_modify(|e: &mut Vec<_>| e.push(pin))
            .or_insert_with(|| vec![pin]);
    }

    let mut ports = Vec::new();
    for (id, mut pins) in pins_by_port {
        pins.retain(|p| {
            p.name != "PDR_ON" && p.name != "PC14OSC32_IN" && p.name != "PC15OSC32_OUT"
        });
        pins.sort_by_key(|p| p.number().unwrap_or_default());
        pins.dedup_by_key(|p| p.number().unwrap_or_default());
        ports.push(Port { id, pins });
    }
    ports.sort_by_key(|p| p.id);

    Ok(ports)
}

pub struct PortMacro {
    features: Vec<String>,
    string: String,
}

use core::fmt;

impl fmt::Display for PortMacro {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.features.len() == 1 {
            writeln!(f, r#"#[cfg(feature = "{}")]"#, self.features[0])?;
        } else {
            writeln!(
                f,
                "#[cfg(any(\n    {}\n))]",
                self.features
                    .iter()
                    .map(|s| format!(r#"feature = "{s}""#))
                    .collect::<Vec<_>>()
                    .join(",\n    ")
            )?;
        }
        f.write_str("channel_impl! {\n")?;
        f.write_str(&self.string)?;
        f.write_str("\n}\n")?;
        Ok(())
    }
}

fn gen_port(port: &Port) -> Result<Vec<String>> {
    let port_upper = port.id;
    let port_lower = port.id.to_ascii_lowercase();
    let mut strings = Vec::new();
    for pin in &port.pins {
        strings.extend(gen_pin(port_upper, port_lower, pin)?);
    }

    Ok(strings)
}

fn gen_pin(port_upper: char, port_lower: char, pin: &gpio::Pin) -> Result<Vec<String>> {
    let nr = pin.number()?;
    let reset_mode = get_pin_reset_mode(pin)?;
    let af_numbers = get_pin_af_numbers(pin)?;
    let mut strings = Vec::new();
/*
    writeln!(
        string,
        "    P{0}{2}: (p{1}{2}, {2}, {3:?}{4}),",
        port_upper,
        port_lower,
        nr,
        af_numbers,
        if let Some(rst) = reset_mode {
            format!(", {}", rst)
        } else {
            String::new()
        }
    )?;
*/

    for (af, func) in af_numbers.into_iter() {
        let pos = func.bytes().position(|b| b == b'_').expect(&format!("Unsupported func {func}"));
        let per = &func[..pos];
        let pn = &func[pos+1..];
        strings.push(format!(
            "    {pn}<{per}>, P{port_upper}{nr}, {af};",
        ));
    }
    Ok(strings)
}

fn get_pin_reset_mode(pin: &gpio::Pin) -> Result<Option<&'static str>> {
    // Debug pins default to their debug function (AF0), everything else
    // defaults to floating input or analog.
    let mode = match (pin.port()?, pin.number()?) {
        ('A', 13) | ('A', 14) | ('A', 15) | ('B', 3) | ('B', 4) => Some("super::Debugger"),
        _ => None,
    };
    Ok(mode)
}

fn get_pin_af_numbers(pin: &gpio::Pin) -> Result<Vec<(u8, String)>> {
    let mut numbers = Vec::new();
    for signal in &pin.pin_signals {
        match signal.af() {
            Ok(af) => numbers.push(af),
            Err(_) => {}
        }
    }

    numbers.sort_unstable();
    numbers.dedup();

    Ok(numbers)
}
