//! mDNS / DNS-SD advertisement for RKSonos.
//!
//! Service type: `_rksonos._tcp.local.`
//! TXT: path=/, proto=http, ver=<crate version>

use mdns_sd::{ServiceDaemon, ServiceInfo};
use std::collections::HashMap;
use std::net::IpAddr;
use tracing::{info, warn};

const SERVICE_TYPE: &str = "_rksonos._tcp.local.";

/// Start mDNS responder. Returns Ok(()) even if disabled / no iface.
///
/// Env:
/// - `MDNS=0` / `false` / `off` → skip
/// - `MDNS_NAME` → instance name (default: hostname or `rksonos`)
/// - port taken from `BIND` (default 8080)
pub fn start(bind: &str) -> anyhow::Result<()> {
    let enabled = match std::env::var("MDNS") {
        Ok(v) => {
            let v = v.trim().to_lowercase();
            !(v == "0" || v == "false" || v == "off" || v == "no")
        }
        Err(_) => true,
    };
    if !enabled {
        info!("mDNS disabled (MDNS=0)");
        return Ok(());
    }

    let port = parse_port(bind);
    let instance = std::env::var("MDNS_NAME").unwrap_or_else(|_| default_instance_name());
    let host_label = sanitize_host_label(&instance);
    let host_name = format!("{host_label}.local.");

    let ips = collect_ipv4();
    if ips.is_empty() {
        warn!("mDNS: no non-loopback IPv4 found; skip advertise");
        return Ok(());
    }

    let mut props = HashMap::new();
    props.insert("path".to_string(), "/".to_string());
    props.insert("proto".to_string(), "http".to_string());
    props.insert("ver".to_string(), env!("CARGO_PKG_VERSION").to_string());
    // Optional: list all addresses in TXT for clients that only get one A record
    if ips.len() > 1 {
        let all = ips
            .iter()
            .map(|a| a.to_string())
            .collect::<Vec<_>>()
            .join(",");
        props.insert("addrs".to_string(), all);
    }

    // mdns-sd 0.13: AsIpAddrs is implemented for &str / String / IpAddr,
    // NOT for bare Ipv4Addr. Use first non-loopback as primary &str (docs style).
    let primary = ips[0].to_string();
    let my_service = ServiceInfo::new(
        SERVICE_TYPE,
        &instance,
        &host_name,
        primary.as_str(),
        port,
        Some(props),
    )?;

    let mdns = ServiceDaemon::new()?;
    mdns.register(my_service)?;
    info!(
        "mDNS registered: {} @ {} -> {}:{} (all={:?})",
        instance, host_name, primary, port, ips
    );

    // Keep daemon alive for process lifetime (mdns-sd runs its own thread).
    std::mem::forget(mdns);
    Ok(())
}

fn parse_port(bind: &str) -> u16 {
    bind.rsplit(':')
        .next()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080)
}

fn default_instance_name() -> String {
    hostname::get()
        .ok()
        .and_then(|h| h.into_string().ok())
        .filter(|s| !s.is_empty())
        .map(|s| sanitize_host_label(&s))
        .unwrap_or_else(|| "rksonos".to_string())
}

fn sanitize_host_label(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "rksonos".into()
    } else {
        out.chars().take(63).collect()
    }
}

fn collect_ipv4() -> Vec<IpAddr> {
    let mut out = Vec::new();
    if let Ok(list) = if_addrs::get_if_addrs() {
        for iface in list {
            if iface.is_loopback() {
                continue;
            }
            if let IpAddr::V4(v4) = iface.ip() {
                out.push(IpAddr::V4(v4));
            }
        }
    }
    out
}

mod hostname {
    pub fn get() -> std::io::Result<std::ffi::OsString> {
        if let Ok(s) = std::fs::read_to_string("/etc/hostname") {
            let t = s.trim();
            if !t.is_empty() {
                return Ok(std::ffi::OsString::from(t));
            }
        }
        std::env::var_os("HOSTNAME")
            .filter(|s| !s.is_empty())
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no hostname"))
    }
}
