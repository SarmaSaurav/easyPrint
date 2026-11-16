use crate::printer::{print_pages, render_document};
use anyhow::Result;
use chrono::Utc;
use rusty_print_common::{ColorMode, JobInfo, JobStatus};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info};
use uuid::Uuid;

pub struct QueueJob {
    pub id: Uuid,
    pub filename: String,
    pub temp_path: PathBuf,
    pub printer_name: String,
    pub copies: u32,
    pub color_mode: ColorMode,
    pub page_range: Option<String>,
}

#[derive(Clone)]
pub struct PrintQueueManager {
    sender: mpsc::Sender<QueueJob>,
    history: Arc<Mutex<VecDeque<JobInfo>>>,
    active_jobs_count: Arc<Mutex<usize>>,
    completed_jobs_count: Arc<Mutex<usize>>,
}

impl PrintQueueManager {
    pub fn new() -> Self {
        let (sender, mut receiver) = mpsc::channel::<QueueJob>(100);
        let history: Arc<Mutex<VecDeque<JobInfo>>> = Arc::new(Mutex::new(VecDeque::with_capacity(50)));
        let active_jobs_count = Arc::new(Mutex::new(0));
        let completed_jobs_count = Arc::new(Mutex::new(0));

        let hist_clone = history.clone();
        let active_clone = active_jobs_count.clone();
        let completed_clone = completed_jobs_count.clone();

        let handle = tokio::runtime::Handle::current();

        // Dedicated OS worker thread for serial printer spooling
        std::thread::Builder::new()
            .name("print-worker".to_string())
            .spawn(move || {
                while let Some(job) = receiver.blocking_recv() {
                    info!("Worker picked up job ID: {}", job.id);

                    // Update job status to Processing
                    handle.block_on(async {
                        let mut hist = hist_clone.lock().await;
                        if let Some(item) = hist.iter_mut().find(|j| j.id == job.id) {
                            item.status = JobStatus::Processing;
                        }
                    });

                    let job_id = job.id;
                    let filename = job.filename.clone();
                    let temp_path = job.temp_path.clone();
                    let printer_name = job.printer_name.clone();
                    let copies = job.copies;
                    let color_mode = job.color_mode;
                    let page_range = job.page_range.clone();

                    let process_result = (|| -> Result<()> {
                        #[cfg(not(windows))]
                        {
                            let lower = filename.to_lowercase();
                            if lower.ends_with(".pdf") || lower.ends_with(".ps") {
                                crate::printer::print_file_linux(&printer_name, &temp_path, copies)?;
                                return Ok(());
                            }
                        }

                        // Read file bytes
                        let bytes = std::fs::read(&temp_path)?;
                        info!("Rendering document '{}' for printing...", filename);

                        // Render document into pages
                        let pages = render_document(&temp_path, &bytes, &filename, page_range.as_deref())?;

                        // Update status to Printing
                        handle.block_on(async {
                            let mut hist = hist_clone.lock().await;
                            if let Some(item) = hist.iter_mut().find(|j| j.id == job_id) {
                                item.status = JobStatus::Printing;
                            }
                        });

                        // Send to Windows Spooler
                        print_pages(&printer_name, &pages, &filename, copies, color_mode)?;

                        Ok(())
                    })();

                    // Cleanup temp file
                    let _ = std::fs::remove_file(&temp_path);

                    // Record outcome
                    handle.block_on(async {
                        let mut hist = hist_clone.lock().await;
                        let mut active = active_clone.lock().await;
                        if *active > 0 {
                            *active -= 1;
                        }

                        if let Some(item) = hist.iter_mut().find(|j| j.id == job_id) {
                            item.completed_at = Some(Utc::now());
                            match process_result {
                                Ok(_) => {
                                    item.status = JobStatus::Completed;
                                    let mut comp = completed_clone.lock().await;
                                    *comp += 1;
                                    info!("Job {} completed successfully!", job_id);
                                }
                                Err(e) => {
                                    item.status = JobStatus::Failed;
                                    item.error_message = Some(e.to_string());
                                    error!("Job {} failed: {:?}", job_id, e);
                                }
                            }
                        }
                    });
                }
            })
            .expect("Failed to spawn background print worker thread");

        Self {
            sender,
            history,
            active_jobs_count,
            completed_jobs_count,
        }
    }

    pub async fn submit_job(&self, job: QueueJob) -> Result<Uuid> {
        let job_id = job.id;
        let info = JobInfo {
            id: job.id,
            filename: job.filename.clone(),
            printer_name: job.printer_name.clone(),
            copies: job.copies,
            color_mode: job.color_mode,
            status: JobStatus::Queued,
            submitted_at: Utc::now(),
            completed_at: None,
            error_message: None,
        };

        {
            let mut hist = self.history.lock().await;
            if hist.len() >= 50 {
                hist.pop_back();
            }
            hist.push_front(info);

            let mut active = self.active_jobs_count.lock().await;
            *active += 1;
        }

        self.sender
            .send(job)
            .await
            .map_err(|_| anyhow::anyhow!("Print worker channel closed"))?;

        Ok(job_id)
    }

    pub async fn get_history(&self) -> Vec<JobInfo> {
        let hist = self.history.lock().await;
        hist.iter().cloned().collect()
    }

    pub async fn get_job(&self, id: Uuid) -> Option<JobInfo> {
        let hist = self.history.lock().await;
        hist.iter().find(|j| j.id == id).cloned()
    }

    pub async fn get_queue_length(&self) -> usize {
        *self.active_jobs_count.lock().await
    }

    pub async fn get_total_completed(&self) -> usize {
        *self.completed_jobs_count.lock().await
    }
}
