use anyhow::{anyhow, Result};
use image::DynamicImage;
use rusty_print_common::{ColorMode, PrinterInfo};
use std::ffi::OsString;
use std::os::windows::ffi::OsStringExt;
use tracing::{info, warn};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Graphics::Gdi::{
    CreateDCW, DeleteDC, GetDeviceCaps, SetStretchBltMode, StretchDIBits, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, COLORONCOLOR, DIB_RGB_COLORS, HDC, HORZRES, SRCCOPY, VERTRES,
};
use windows::Win32::Graphics::Printing::{
    EnumPrintersW, GetDefaultPrinterW, PRINTER_ENUM_CONNECTIONS, PRINTER_ENUM_LOCAL,
    PRINTER_INFO_2W,
};
use windows::Win32::Storage::Xps::{EndDoc, EndPage, StartDocW, StartPage, DOCINFOW};

/// Get the list of all printers installed on this Windows machine
pub fn list_printers() -> Result<Vec<PrinterInfo>> {
    let default_printer = get_default_printer_name().unwrap_or_default();
    let mut printers = Vec::new();

    let flags = PRINTER_ENUM_LOCAL | PRINTER_ENUM_CONNECTIONS;
    let mut bytes_needed = 0u32;
    let mut num_printers = 0u32;

    unsafe {
        let _ = EnumPrintersW(
            flags,
            PCWSTR::null(),
            2,
            None,
            &mut bytes_needed,
            &mut num_printers,
        );

        if bytes_needed == 0 {
            return Ok(printers);
        }

        let mut buffer = vec![0u8; bytes_needed as usize];
        let success = EnumPrintersW(
            flags,
            PCWSTR::null(),
            2,
            Some(&mut buffer),
            &mut bytes_needed,
            &mut num_printers,
        );

        if success.is_ok() {
            let p_info = buffer.as_ptr() as *const PRINTER_INFO_2W;
            for i in 0..num_printers as usize {
                let info_ptr = p_info.add(i);
                let info = &*info_ptr;

                let name = pwstr_to_string(info.pPrinterName);
                let driver = pwstr_to_string(info.pDriverName);
                let port = pwstr_to_string(info.pPortName);
                let is_default = name.eq_ignore_ascii_case(&default_printer);

                printers.push(PrinterInfo {
                    name,
                    driver_name: driver,
                    port_name: port,
                    is_default,
                    is_online: true,
                });
            }
        } else {
            warn!("EnumPrintersW failed to fetch printer buffer");
        }
    }

    Ok(printers)
}

/// Retrieve the system's default printer name
pub fn get_default_printer_name() -> Result<String> {
    unsafe {
        let mut size = 0u32;
        let _ = GetDefaultPrinterW(PWSTR::null(), &mut size);
        if size == 0 {
            return Err(anyhow!("No default printer found"));
        }

        let mut buffer = vec![0u16; size as usize];
        let res = GetDefaultPrinterW(PWSTR(buffer.as_mut_ptr()), &mut size);
        if res.as_bool() {
            let len = buffer.iter().position(|&c| c == 0).unwrap_or(buffer.len());
            let name = OsString::from_wide(&buffer[..len])
                .to_string_lossy()
                .into_owned();
            Ok(name)
        } else {
            Err(anyhow!("Failed to query default printer"))
        }
    }
}

/// Helper struct that ensures HDC and StartDoc/EndDoc are always cleaned up properly
struct PrinterDC {
    hdc: HDC,
    doc_started: bool,
}

impl PrinterDC {
    pub fn new(printer_name: &str, doc_title: &str) -> Result<Self> {
        let wide_driver: Vec<u16> = "WINSPOOL\0".encode_utf16().collect();
        let wide_device: Vec<u16> = format!("{}\0", printer_name).encode_utf16().collect();

        unsafe {
            let hdc = CreateDCW(
                PCWSTR(wide_driver.as_ptr()),
                PCWSTR(wide_device.as_ptr()),
                PCWSTR::null(),
                None,
            );

            if hdc.is_invalid() {
                return Err(anyhow!("Failed to create DC for printer '{}'", printer_name));
            }

            let wide_title: Vec<u16> = format!("{}\0", doc_title).encode_utf16().collect();
            let doc_info = DOCINFOW {
                cbSize: std::mem::size_of::<DOCINFOW>() as i32,
                lpszDocName: PCWSTR(wide_title.as_ptr()),
                lpszOutput: PCWSTR::null(),
                lpszDatatype: PCWSTR::null(),
                fwType: 0,
            };

            let doc_id = StartDocW(hdc, &doc_info);
            if doc_id <= 0 {
                let _ = DeleteDC(hdc);
                return Err(anyhow!("StartDocW failed for printer '{}'", printer_name));
            }

            Ok(Self {
                hdc,
                doc_started: true,
            })
        }
    }

    pub fn print_page(&self, img: &DynamicImage, color_mode: ColorMode) -> Result<()> {
        unsafe {
            if StartPage(self.hdc) <= 0 {
                return Err(anyhow!("StartPage failed"));
            }

            // Set high quality stretch mode
            let _ = SetStretchBltMode(self.hdc, COLORONCOLOR);

            // Get printable area dimensions in device pixels
            let page_w = GetDeviceCaps(self.hdc, HORZRES);
            let page_h = GetDeviceCaps(self.hdc, VERTRES);

            let img_w = img.width() as i32;
            let img_h = img.height() as i32;

            if img_w <= 0 || img_h <= 0 || page_w <= 0 || page_h <= 0 {
                let _ = EndPage(self.hdc);
                return Err(anyhow!("Invalid dimensions for printing"));
            }

            // Calculate fitted aspect ratio on the printable page
            let scale_x = page_w as f64 / img_w as f64;
            let scale_y = page_h as f64 / img_h as f64;
            let scale = scale_x.min(scale_y);

            let dest_w = (img_w as f64 * scale) as i32;
            let dest_h = (img_h as f64 * scale) as i32;

            // Center image on the page
            let dest_x = (page_w - dest_w) / 2;
            let dest_y = (page_h - dest_h) / 2;

            // Prepare DIB bitmap buffer
            // Windows GDI expects BGR or BGRA format (or top-down negative height)
            let (dib_data, bmi) = match color_mode {
                ColorMode::Monochrome | ColorMode::Grayscale => {
                    let gray_img = img.to_luma8();
                    let rgba = DynamicImage::ImageLuma8(gray_img).to_rgba8();
                    build_bgra_dib(&rgba, img_w, img_h)
                }
                ColorMode::Color => {
                    let rgba = img.to_rgba8();
                    build_bgra_dib(&rgba, img_w, img_h)
                }
            };

            let lines = StretchDIBits(
                self.hdc,
                dest_x,
                dest_y,
                dest_w,
                dest_h,
                0,
                0,
                img_w,
                img_h,
                Some(dib_data.as_ptr() as *const _),
                &bmi,
                DIB_RGB_COLORS,
                SRCCOPY,
            );

            if lines == 0 {
                warn!("StretchDIBits returned 0 lines drawn");
            }

            let end_res = EndPage(self.hdc);
            if end_res <= 0 {
                return Err(anyhow!("EndPage failed"));
            }
        }

        Ok(())
    }
}

impl Drop for PrinterDC {
    fn drop(&mut self) {
        unsafe {
            if self.doc_started {
                let _ = EndDoc(self.hdc);
            }
            if !self.hdc.is_invalid() {
                let _ = DeleteDC(self.hdc);
            }
        }
    }
}

/// Convert an RGBA image buffer to a BGRA top-down DIB buffer and BITMAPINFO header
fn build_bgra_dib(rgba: &image::RgbaImage, width: i32, height: i32) -> (Vec<u8>, BITMAPINFO) {
    let mut bgra_data = Vec::with_capacity((width * height * 4) as usize);
    for pixel in rgba.pixels() {
        // RGBA -> BGRA
        bgra_data.push(pixel[2]); // B
        bgra_data.push(pixel[1]); // G
        bgra_data.push(pixel[0]); // R
        bgra_data.push(pixel[3]); // A
    }

    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            biHeight: -height, // Negative height indicates top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            biSizeImage: (width * height * 4) as u32,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        },
        bmiColors: [windows::Win32::Graphics::Gdi::RGBQUAD::default()],
    };

    (bgra_data, bmi)
}

/// Print a set of rendered page images to the specified printer
pub fn print_pages(
    printer_name: &str,
    pages: &[DynamicImage],
    doc_title: &str,
    copies: u32,
    color_mode: ColorMode,
) -> Result<()> {
    if pages.is_empty() {
        return Err(anyhow!("No pages to print"));
    }

    info!(
        "Starting print job '{}': {} pages, {} copies on printer '{}'",
        doc_title,
        pages.len(),
        copies,
        printer_name
    );

    let effective_copies = copies.max(1);
    for copy_idx in 0..effective_copies {
        info!("Printing copy {}/{}...", copy_idx + 1, effective_copies);
        let dc = PrinterDC::new(printer_name, doc_title)?;
        for (page_idx, page) in pages.iter().enumerate() {
            info!("Spooling page {}/{}...", page_idx + 1, pages.len());
            dc.print_page(page, color_mode)?;
        }
    }

    info!("Print job '{}' successfully spooled to driver", doc_title);
    Ok(())
}

fn pwstr_to_string(pwstr: PWSTR) -> String {
    if pwstr.is_null() {
        return String::new();
    }
    unsafe {
        let mut len = 0;
        while *pwstr.0.add(len) != 0 {
            len += 1;
        }
        let slice = std::slice::from_raw_parts(pwstr.0, len);
        OsString::from_wide(slice).to_string_lossy().into_owned()
    }
}
