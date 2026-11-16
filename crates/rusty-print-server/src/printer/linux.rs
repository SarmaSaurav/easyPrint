use anyhow::{anyhow, Result};
use image::DynamicImage;
use rusty_print_common::{ColorMode, PrinterInfo};
use std::process::Command;
use tracing::info;

/// List installed printers on Linux via CUPS lpstat command
pub fn list_printers() -> Result<Vec<PrinterInfo>> {
    let output = Command::new("lpstat")
        .arg("-p")
        .arg("-d")
        .output()
        .map_err(|e| anyhow!("Failed to execute lpstat: {:?}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let default_printer = get_default_printer_name().unwrap_or_default();

    let mut printers = Vec::new();
    for line in stdout.lines() {
        if line.starts_with("printer ") {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 {
                let name = parts[1].to_string();
                let is_default = name.eq_ignore_ascii_case(&default_printer);
                printers.push(PrinterInfo {
                    name: name.clone(),
                    driver_name: "CUPS Linux Driver".to_string(),
                    port_name: "USB/Local".to_string(),
                    is_default,
                    is_online: true,
                });
            }
        }
    }

    if printers.is_empty() {
        printers.push(PrinterInfo {
            name: "Default-Printer".to_string(),
            driver_name: "Generic-CUPS".to_string(),
            port_name: "Default".to_string(),
            is_default: true,
            is_online: true,
        });
    }

    Ok(printers)
}

/// Retrieve default CUPS printer name
pub fn get_default_printer_name() -> Result<String> {
    let output = Command::new("lpstat")
        .arg("-d")
        .output()
        .map_err(|e| anyhow!("Failed to execute lpstat -d: {:?}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    // Typical output: "system default destination: Brother_DCP"
    if let Some(pos) = stdout.find(':') {
        let name = stdout[pos + 1..].trim().to_string();
        if !name.is_empty() {
            return Ok(name);
        }
    }

    Err(anyhow!("No default printer found on Linux host"))
}

/// Print rendered pages or document on Linux using CUPS `lp`
pub fn print_pages(
    printer_name: &str,
    pages: &[DynamicImage],
    doc_title: &str,
    copies: u32,
    _color_mode: ColorMode,
) -> Result<()> {
    info!(
        "Linux CUPS: Spooling print job '{}' with {} page(s) to printer '{}' ({} copies)",
        doc_title,
        pages.len(),
        printer_name,
        copies
    );
    for (i, page) in pages.iter().enumerate() {
        let temp_img_path = std::env::temp_dir().join(format!(
            "rusty_print_page_{}_{}.png",
            std::process::id(),
            i
        ));
        page.save(&temp_img_path)?;
        let res = print_file_linux(printer_name, &temp_img_path, copies);
        let _ = std::fs::remove_file(&temp_img_path);
        res?;
    }
    Ok(())
}

/// Print an arbitrary file on Linux directly via CUPS `lp` command
pub fn print_file_linux(
    printer_name: &str,
    file_path: &std::path::Path,
    copies: u32,
) -> Result<()> {
    info!("Running CUPS lp command on Linux for {:?}", file_path);
    let mut cmd = Command::new("lp");
    cmd.arg("-d").arg(printer_name);
    cmd.arg("-n").arg(copies.to_string());
    cmd.arg(file_path);

    let output = cmd.output().map_err(|e| anyhow!("Failed to execute lp: {:?}", e))?;
    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("lp failed with error: {}", err));
    }

    info!("CUPS accepted print job for {:?}", file_path);
    Ok(())
}

/// Fallback renderer on Linux
pub fn render_document(
    _path: &std::path::Path,
    bytes: &[u8],
    _filename: &str,
    _page_range: Option<&str>,
) -> Result<Vec<DynamicImage>> {
    let img = image::load_from_memory(bytes)?;
    Ok(vec![img])
}
