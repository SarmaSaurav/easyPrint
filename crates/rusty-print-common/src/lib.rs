use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const DEFAULT_PORT: u16 = 8989;
pub const MDNS_SERVICE_TYPE: &str = "_rustyprint._tcp.local.";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterInfo {
    pub name: String,
    pub driver_name: String,
    pub port_name: String,
    pub is_default: bool,
    pub is_online: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorMode {
    Color,
    Grayscale,
    Monochrome,
}

impl Default for ColorMode {
    fn default() -> Self {
        Self::Color
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Orientation {
    Portrait,
    Landscape,
    Auto,
}

impl Default for Orientation {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJobRequest {
    pub printer_name: Option<String>,
    pub copies: u32,
    pub color_mode: ColorMode,
    pub orientation: Orientation,
    pub page_range: Option<String>,
    pub pin: String,
}

impl Default for PrintJobRequest {
    fn default() -> Self {
        Self {
            printer_name: None,
            copies: 1,
            color_mode: ColorMode::Color,
            orientation: Orientation::Auto,
            page_range: None,
            pin: String::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Processing,
    Printing,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobInfo {
    pub id: Uuid,
    pub filename: String,
    pub printer_name: String,
    pub copies: u32,
    pub color_mode: ColorMode,
    pub status: JobStatus,
    pub submitted_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrintJobResponse {
    pub success: bool,
    pub job_id: Option<Uuid>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerStatus {
    pub version: String,
    pub server_name: String,
    pub active_printer: Option<String>,
    pub printers: Vec<PrinterInfo>,
    pub queue_length: usize,
    pub total_jobs_completed: usize,
}
