use anyhow::Result;
use mdns_sd::{ServiceDaemon, ServiceInfo};
use rusty_print_common::MDNS_SERVICE_TYPE;
use std::collections::HashMap;
use tracing::info;

pub struct MdnsService {
    _daemon: ServiceDaemon,
    fullname: String,
    ipp_fullname: String,
}

pub fn get_system_hostname() -> String {
    #[cfg(windows)]
    {
        std::env::var("COMPUTERNAME").unwrap_or_else(|_| "windows-desktop".to_string())
    }
    #[cfg(not(windows))]
    {
        std::fs::read_to_string("/etc/hostname")
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| std::env::var("HOSTNAME").unwrap_or_else(|_| "linux-desktop".to_string()))
    }
}

pub fn get_sanitized_hostname() -> String {
    let raw = get_system_hostname().to_lowercase();
    let sanitized: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c } else { '-' })
        .collect();
    let trimmed = sanitized.trim_matches('-').to_string();
    if trimmed.is_empty() {
        "office-printer".to_string()
    } else {
        trimmed
    }
}

impl MdnsService {
    pub fn start(service_name: &str, port: u16, active_printer: &str) -> Result<Self> {
        let daemon = ServiceDaemon::new()?;
        let clean_host = get_sanitized_hostname();
        let host_name = format!("{}.local.", clean_host);

        // 1. Custom Rusty-Print protocol advertisement
        let mut properties = HashMap::new();
        properties.insert("printer".to_string(), active_printer.to_string());
        properties.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());

        let service_info = ServiceInfo::new(
            MDNS_SERVICE_TYPE,
            service_name,
            &host_name,
            "",
            port,
            properties,
        )?;
        let fullname = service_info.get_fullname().to_string();
        daemon.register(service_info)?;

        // 2. Standard IPP (Internet Printing Protocol / AirPrint / CUPS / Mopria) advertisement
        let mut ipp_props = HashMap::new();
        ipp_props.insert("txtvers".to_string(), "1".to_string());
        ipp_props.insert("qtotal".to_string(), "1".to_string());
        ipp_props.insert("rp".to_string(), "ipp/print".to_string());
        ipp_props.insert("note".to_string(), service_name.to_string());
        ipp_props.insert("ty".to_string(), service_name.to_string());
        ipp_props.insert(
            "pdl".to_string(),
            "application/pdf,image/pwg-raster,image/jpeg,image/png".to_string(),
        );
        ipp_props.insert("printer-state".to_string(), "3".to_string());
        ipp_props.insert("printer-type".to_string(), "0x809046".to_string());

        let ipp_service = ServiceInfo::new(
            "_ipp._tcp.local.",
            service_name,
            &host_name,
            "",
            port,
            ipp_props,
        )?;
        let ipp_fullname = ipp_service.get_fullname().to_string();
        daemon.register(ipp_service)?;

        info!(
            "mDNS broadcast active: Custom '{}' and Driverless IPP '{}' on port {}",
            fullname, ipp_fullname, port
        );

        Ok(Self {
            _daemon: daemon,
            fullname,
            ipp_fullname,
        })
    }

    #[allow(dead_code)]
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    #[allow(dead_code)]
    pub fn ipp_fullname(&self) -> &str {
        &self.ipp_fullname
    }
}
