use crate::config::ServerConfig;
use crate::ipp::{
    build_print_job_response, build_printer_attributes_response, build_validate_job_response,
    IppRequest, OP_GET_PRINTER_ATTRIBUTES, OP_PRINT_JOB, OP_VALIDATE_JOB,
};
use crate::printer::{get_default_printer_name, list_printers};
use crate::queue::{PrintQueueManager, QueueJob};
use axum::{
    body::Bytes,
    extract::{Multipart, Path as AxumPath, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use rusty_print_common::{
    ColorMode, JobInfo, PrintJobResponse, ServerStatus,
};
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<ServerConfig>,
    pub queue: PrintQueueManager,
}

pub async fn get_status(State(state): State<AppState>) -> impl IntoResponse {
    let printers = list_printers().unwrap_or_default();
    let default_name = get_default_printer_name().ok();

    let active_printer = state
        .config
        .preferred_printer
        .clone()
        .or_else(|| default_name.clone());

    let queue_length = state.queue.get_queue_length().await;
    let total_completed = state.queue.get_total_completed().await;

    let status = ServerStatus {
        version: env!("CARGO_PKG_VERSION").to_string(),
        server_name: state.config.service_name.clone(),
        active_printer,
        printers,
        queue_length,
        total_jobs_completed: total_completed,
    };

    Json(status)
}

pub async fn get_printers() -> impl IntoResponse {
    let printers = list_printers().unwrap_or_default();
    Json(printers)
}

pub async fn submit_print_job(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let mut file_data: Option<(String, std::path::PathBuf)> = None;
    let mut pin = String::new();
    let mut copies: u32 = 1;
    let mut color_mode = ColorMode::Color;
    let mut page_range: Option<String> = None;
    let mut printer_name: Option<String> = None;

    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();

        match name.as_str() {
            "file" => {
                let filename = field
                    .file_name()
                    .unwrap_or("document.pdf")
                    .to_string();

                let temp_dir = std::env::temp_dir();
                let temp_path = temp_dir.join(format!("rusty_print_{}_{}", Uuid::new_v4(), filename));

                let data = match field.bytes().await {
                    Ok(b) => b,
                    Err(e) => {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(PrintJobResponse {
                                success: false,
                                job_id: None,
                                message: format!("Failed to read uploaded file: {}", e),
                            }),
                        );
                    }
                };

                if let Err(e) = std::fs::write(&temp_path, &data) {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(PrintJobResponse {
                            success: false,
                            job_id: None,
                            message: format!("Failed to write temporary file: {}", e),
                        }),
                    );
                }

                file_data = Some((filename, temp_path));
            }
            "pin" => {
                if let Ok(text) = field.text().await {
                    pin = text.trim().to_string();
                }
            }
            "copies" => {
                if let Ok(text) = field.text().await {
                    copies = text.trim().parse::<u32>().unwrap_or(1).clamp(1, 99);
                }
            }
            "color_mode" => {
                if let Ok(text) = field.text().await {
                    color_mode = match text.to_lowercase().as_str() {
                        "grayscale" | "monochrome" | "bw" => ColorMode::Grayscale,
                        _ => ColorMode::Color,
                    };
                }
            }
            "page_range" => {
                if let Ok(text) = field.text().await {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        page_range = Some(trimmed);
                    }
                }
            }
            "printer_name" => {
                if let Ok(text) = field.text().await {
                    let trimmed = text.trim().to_string();
                    if !trimmed.is_empty() {
                        printer_name = Some(trimmed);
                    }
                }
            }
            _ => {}
        }
    }

    // Verify PIN
    if pin != state.config.pin {
        warn!("Print attempt rejected: Invalid PIN provided");
        if let Some((_, temp_path)) = file_data {
            let _ = std::fs::remove_file(temp_path);
        }
        return (
            StatusCode::UNAUTHORIZED,
            Json(PrintJobResponse {
                success: false,
                job_id: None,
                message: "Unauthorized: Invalid department PIN".to_string(),
            }),
        );
    }

    let (filename, temp_path) = match file_data {
        Some(fd) => fd,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(PrintJobResponse {
                    success: false,
                    job_id: None,
                    message: "No document file was provided in the request".to_string(),
                }),
            );
        }
    };

    // Determine target printer
    let target_printer = printer_name
        .or_else(|| state.config.preferred_printer.clone())
        .or_else(|| get_default_printer_name().ok());

    let target_printer = match target_printer {
        Some(p) => p,
        None => {
            let _ = std::fs::remove_file(temp_path);
            return (
                StatusCode::BAD_REQUEST,
                Json(PrintJobResponse {
                    success: false,
                    job_id: None,
                    message: "No target printer found on this host".to_string(),
                }),
            );
        }
    };

    let job_id = Uuid::new_v4();
    let queue_job = QueueJob {
        id: job_id,
        filename: filename.clone(),
        temp_path,
        printer_name: target_printer,
        copies,
        color_mode,
        page_range,
    };

    match state.queue.submit_job(queue_job).await {
        Ok(id) => {
            info!("Queued job '{}' with ID {}", filename, id);
            (
                StatusCode::OK,
                Json(PrintJobResponse {
                    success: true,
                    job_id: Some(id),
                    message: format!("Print job #{} queued successfully", id),
                }),
            )
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(PrintJobResponse {
                    success: false,
                    job_id: None,
                    message: format!("Failed to queue job: {}", e),
                }),
            )
        }
    }
}

pub async fn get_jobs(State(state): State<AppState>) -> Json<Vec<JobInfo>> {
    let history = state.queue.get_history().await;
    Json(history)
}

pub async fn get_job_by_id(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<Uuid>,
) -> impl IntoResponse {
    match state.queue.get_job(id).await {
        Some(job) => (StatusCode::OK, Json(Some(job))),
        None => (StatusCode::NOT_FOUND, Json(None)),
    }
}

/// Standard IPP (Internet Printing Protocol / AirPrint / Mopria / CUPS) Handler
pub async fn handle_ipp(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let req = match IppRequest::parse(&body) {
        Ok(r) => r,
        Err(e) => {
            warn!("Failed to parse incoming IPP packet: {:?}", e);
            return (
                StatusCode::BAD_REQUEST,
                [(header::CONTENT_TYPE, "text/plain")],
                Bytes::from("Invalid IPP Request"),
            );
        }
    };

    let default_name = get_default_printer_name().unwrap_or_else(|_| "Office-Printer".to_string());
    let active_printer = state
        .config
        .preferred_printer
        .clone()
        .unwrap_or_else(|| default_name.clone());

    let service_name = if state.config.service_name.trim().is_empty()
        || state.config.service_name == "Office Wireless Printer"
    {
        let hostname = crate::mdns::get_system_hostname();
        format!("{} ({})", active_printer, hostname)
    } else {
        state.config.service_name.clone()
    };

    let host_header = headers
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            format!(
                "{}.local:{}",
                crate::mdns::get_sanitized_hostname(),
                state.config.port
            )
        });

    let printer_uri = format!("ipp://{}/ipp/print", host_header);

    match req.operation_id {
        OP_GET_PRINTER_ATTRIBUTES => {
            info!("IPP: Handling Get-Printer-Attributes query for '{}'", service_name);
            let resp = build_printer_attributes_response(&req, &service_name, &printer_uri);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/ipp")],
                resp,
            )
        }
        OP_VALIDATE_JOB => {
            info!("IPP: Handling Validate-Job query");
            let resp = build_validate_job_response(&req);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/ipp")],
                resp,
            )
        }
        OP_PRINT_JOB => {
            info!(
                "IPP: Received native Ctrl+P Print-Job! Document size: {} bytes",
                req.document_data.len()
            );

            let job_id = Uuid::new_v4();

            if !req.document_data.is_empty() {
                let temp_dir = std::env::temp_dir();
                let temp_path = temp_dir.join(format!("rusty_print_ipp_{}.pdf", job_id));

                if let Err(e) = std::fs::write(&temp_path, &req.document_data) {
                    warn!("Failed to write IPP spool file: {:?}", e);
                } else {
                    let queue_job = QueueJob {
                        id: job_id,
                        filename: format!("IPP_Document_{}.pdf", &job_id.to_string()[..8]),
                        temp_path,
                        printer_name: active_printer,
                        copies: 1,
                        color_mode: ColorMode::Color,
                        page_range: None,
                    };
                    let _ = state.queue.submit_job(queue_job).await;
                }
            }

            let resp = build_print_job_response(&req, 1);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/ipp")],
                resp,
            )
        }
        _ => {
            info!("IPP: Unsupported operation 0x{:04x}, acknowledging ok", req.operation_id);
            let resp = build_validate_job_response(&req);
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, "application/ipp")],
                resp,
            )
        }
    }
}
