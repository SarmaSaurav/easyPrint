use anyhow::{anyhow, Context, Result};
use image::DynamicImage;
use std::collections::HashSet;
use std::path::Path;
use tracing::info;
use windows::core::HSTRING;
use windows::Data::Pdf::PdfDocument;
use windows::Storage::Streams::{DataReader, InMemoryRandomAccessStream};
use windows::Storage::StorageFile;

/// Render an arbitrary uploaded file (PDF or Image) into a list of printable page images
pub fn render_document(
    path: &Path,
    bytes: &[u8],
    filename: &str,
    page_range: Option<&str>,
) -> Result<Vec<DynamicImage>> {
    let lower_name = filename.to_lowercase();

    if lower_name.ends_with(".pdf") {
        render_pdf(path, page_range)
    } else if lower_name.ends_with(".png")
        || lower_name.ends_with(".jpg")
        || lower_name.ends_with(".jpeg")
        || lower_name.ends_with(".bmp")
        || lower_name.ends_with(".webp")
        || lower_name.ends_with(".gif")
    {
        render_image(bytes)
    } else {
        // Fallback: try decoding as an image directly
        match image::load_from_memory(bytes) {
            Ok(img) => Ok(vec![img]),
            Err(_) => Err(anyhow!(
                "Unsupported file format for '{}'. Please provide a PDF or image file.",
                filename
            )),
        }
    }
}

/// Decode single image file from memory
fn render_image(bytes: &[u8]) -> Result<Vec<DynamicImage>> {
    let img = image::load_from_memory(bytes)
        .context("Failed to decode image file. File may be corrupted.")?;
    Ok(vec![img])
}

/// Render PDF pages into DynamicImage objects using Windows native WinRT Data.Pdf engine
fn render_pdf(path: &Path, page_range: Option<&str>) -> Result<Vec<DynamicImage>> {
    let abs_path = path
        .canonicalize()
        .context("Failed to get canonical path for PDF")?;
    let path_str = abs_path
        .to_str()
        .ok_or_else(|| anyhow!("Invalid Unicode in file path"))?;

    // Strip Windows extended path prefix \\?\ if present as WinRT APIs dislike it in some versions
    let clean_path = path_str.strip_prefix(r"\\?\").unwrap_or(path_str);
    let h_path = HSTRING::from(clean_path);

    info!("Opening PDF document via Windows.Data.Pdf: {}", clean_path);

    let storage_file = StorageFile::GetFileFromPathAsync(&h_path)
        .map_err(|e| anyhow!("StorageFile::GetFileFromPathAsync failed: {:?}", e))?
        .get()
        .map_err(|e| anyhow!("Failed to resolve PDF file: {:?}", e))?;

    let pdf_doc = PdfDocument::LoadFromFileAsync(&storage_file)
        .map_err(|e| anyhow!("PdfDocument::LoadFromFileAsync failed: {:?}", e))?
        .get()
        .map_err(|e| anyhow!("Failed to load PDF document: {:?}", e))?;

    let total_pages = pdf_doc.PageCount().map_err(|e| anyhow!("Failed to read page count: {:?}", e))?;
    if total_pages == 0 {
        return Err(anyhow!("PDF document contains 0 pages"));
    }

    info!("PDF opened successfully. Total pages: {}", total_pages);
    let target_pages = parse_page_range(page_range, total_pages);

    let mut rendered_pages = Vec::with_capacity(target_pages.len());

    for page_idx in 0..total_pages {
        if !target_pages.contains(&page_idx) {
            continue;
        }

        info!("Rendering PDF page {}/{}...", page_idx + 1, total_pages);
        let pdf_page = pdf_doc.GetPage(page_idx)
            .map_err(|e| anyhow!("Failed to get page {}: {:?}", page_idx + 1, e))?;

        let stream = InMemoryRandomAccessStream::new()
            .map_err(|e| anyhow!("Failed to create InMemoryRandomAccessStream: {:?}", e))?;

        pdf_page.RenderToStreamAsync(&stream)
            .map_err(|e| anyhow!("RenderToStreamAsync failed on page {}: {:?}", page_idx + 1, e))?
        .get()
        .map_err(|e| anyhow!("RenderToStreamAsync completion failed on page {}: {:?}", page_idx + 1, e))?;

        let stream_size = stream.Size().map_err(|e| anyhow!("Failed to get stream size: {:?}", e))? as usize;
        if stream_size == 0 {
            return Err(anyhow!("Rendered page stream size was 0 on page {}", page_idx + 1));
        }

        let reader = DataReader::CreateDataReader(&stream)
            .map_err(|e| anyhow!("CreateDataReader failed: {:?}", e))?;
        reader.LoadAsync(stream_size as u32)
            .map_err(|e| anyhow!("DataReader.LoadAsync failed: {:?}", e))?
        .get()
        .map_err(|e| anyhow!("DataReader load completion failed: {:?}", e))?;

        let mut page_bytes = vec![0u8; stream_size];
        reader.ReadBytes(&mut page_bytes)
            .map_err(|e| anyhow!("DataReader.ReadBytes failed: {:?}", e))?;

        let page_img = image::load_from_memory(&page_bytes)
            .map_err(|e| anyhow!("Failed to parse rendered page bitmap on page {}: {:?}", page_idx + 1, e))?;

        rendered_pages.push(page_img);
    }

    info!("Rendered {} page(s) successfully", rendered_pages.len());
    Ok(rendered_pages)
}

/// Parse user-supplied page range string like "1-3, 5, 8" into 0-indexed set
fn parse_page_range(range_opt: Option<&str>, total_pages: u32) -> HashSet<u32> {
    let mut pages = HashSet::new();
    let range_str = match range_opt {
        Some(s) if !s.trim().is_empty() && !s.eq_ignore_ascii_case("all") => s.trim(),
        _ => {
            for i in 0..total_pages {
                pages.insert(i);
            }
            return pages;
        }
    };

    for part in range_str.split(',') {
        let trimmed = part.trim();
        if trimmed.contains('-') {
            let segs: Vec<&str> = trimmed.split('-').collect();
            if segs.len() == 2 {
                if let (Ok(start), Ok(end)) = (segs[0].trim().parse::<u32>(), segs[1].trim().parse::<u32>()) {
                    let s = start.max(1) - 1;
                    let e = (end.min(total_pages)) - 1;
                    if s <= e {
                        for p in s..=e {
                            pages.insert(p);
                        }
                    }
                }
            }
        } else if let Ok(num) = trimmed.parse::<u32>() {
            if num >= 1 && num <= total_pages {
                pages.insert(num - 1);
            }
        }
    }

    if pages.is_empty() {
        for i in 0..total_pages {
            pages.insert(i);
        }
    }

    pages
}
