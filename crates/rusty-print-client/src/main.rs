use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use mdns_sd::{ServiceDaemon, ServiceEvent};
use reqwest::multipart::{Form, Part};
use rusty_print_common::{JobInfo, JobStatus, PrintJobResponse, ServerStatus, MDNS_SERVICE_TYPE};
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;

#[derive(Parser)]
#[command(
    name = "rusty-print",
    version,
    about = "Cross-platform CLI client for the Rusty-Print Wireless Bridge (Windows, Ubuntu, Zorin OS, Linux Mint)"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Discover available print bridges on the local Wi-Fi / LAN via mDNS
    Discover {
        /// Search timeout in seconds
        #[arg(short, long, default_value_t = 3)]
        timeout: u64,
    },

    /// Automatically install all discovered office printers into your OS Ctrl+P print dialog
    InstallPrinters {
        /// Search timeout in seconds
        #[arg(short, long, default_value_t = 3)]
        timeout: u64,
    },

    /// Check print server status and available printers
    Status {
        /// Server URL (e.g. http://office-printer.local:8989). Auto-discovers if omitted.
        #[arg(short, long)]
        server: Option<String>,
    },

    /// Print a document (PDF or Image)
    Print {
        /// Path to the PDF or image file
        file: PathBuf,

        /// Server URL (e.g. http://office-printer.local:8989). Auto-discovers if omitted.
        #[arg(short, long)]
        server: Option<String>,

        /// Department PIN / Passkey
        #[arg(short, long)]
        pin: Option<String>,

        /// Number of copies
        #[arg(short, long, default_value_t = 1)]
        copies: u32,

        /// Color mode: 'color' or 'bw'/'grayscale'
        #[arg(long, default_value = "color")]
        color: String,

        /// Specific target printer name on the host
        #[arg(long)]
        printer: Option<String>,

        /// Page range to print, e.g. "1-3, 5"
        #[arg(long)]
        pages: Option<String>,

        /// Wait and monitor job until completed
        #[arg(short, long, default_value_t = true)]
        wait: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Discover { timeout } => {
            println!("Searching for Rusty-Print bridges on the Wi-Fi network ({}s)...", timeout);
            let servers = discover_servers(Duration::from_secs(timeout))?;
            if servers.is_empty() {
                println!("No print servers discovered via mDNS.");
                println!("Hint: Verify the server is running on the host machine.");
            } else {
                println!("\nFound {} office print bridge(s):", servers.len());
                for (name, url, printer) in servers {
                    println!("  • {} -> {}", name, url);
                    println!("    Target Hardware: {}", printer);
                }
            }
        }

        Commands::InstallPrinters { timeout } => {
            println!("Scanning Wi-Fi network for office print bridges ({}s)...", timeout);
            let servers = discover_servers(Duration::from_secs(timeout))?;
            if servers.is_empty() {
                println!("No print servers found on this Wi-Fi. Ensure the host desktop is running.");
                return Ok(());
            }

            println!("Found {} printer bridge(s). Registering with your OS...", servers.len());
            install_printers_locally(&servers)?;
        }

        Commands::Status { server } => {
            let base_url = resolve_server_url(server).await?;
            println!("Connecting to print bridge at: {}", base_url);

            let client = reqwest::Client::new();
            let res = client
                .get(format!("{}/api/v1/status", base_url))
                .send()
                .await
                .context("Failed to connect to print server")?;

            if !res.status().is_success() {
                return Err(anyhow!("Server returned error: {}", res.status()));
            }

            let status: ServerStatus = res.json().await?;
            println!("\n=== Print Bridge Status ===");
            println!("Server Name:      {}", status.server_name);
            println!("Version:          {}", status.version);
            println!(
                "Active Printer:   {}",
                status.active_printer.as_deref().unwrap_or("None")
            );
            println!("Queue Length:     {} job(s)", status.queue_length);
            println!("Total Completed:  {} job(s)", status.total_jobs_completed);
            println!("\nInstalled Printers:");
            for p in status.printers {
                let default_tag = if p.is_default { " (Default)" } else { "" };
                println!("  - {}{} [Driver: {}, Port: {}]", p.name, default_tag, p.driver_name, p.port_name);
            }
        }

        Commands::Print {
            file,
            server,
            pin,
            copies,
            color,
            printer,
            pages,
            wait,
        } => {
            if !file.exists() {
                return Err(anyhow!("File does not exist: {:?}", file));
            }

            let base_url = resolve_server_url(server).await?;
            let pin = resolve_pin(pin)?;

            let filename = file
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("document.pdf")
                .to_string();

            let file_bytes = tokio::fs::read(&file)
                .await
                .context("Failed to read document file")?;

            println!("Sending '{}' to print bridge ({})", filename, base_url);

            let mut form = Form::new()
                .text("pin", pin)
                .text("copies", copies.to_string())
                .text("color_mode", color)
                .part("file", Part::bytes(file_bytes).file_name(filename.clone()));

            if let Some(p) = printer {
                form = form.text("printer_name", p);
            }
            if let Some(pg) = pages {
                form = form.text("page_range", pg);
            }

            let client = reqwest::Client::new();
            let res = client
                .post(format!("{}/api/v1/print", base_url))
                .multipart(form)
                .send()
                .await
                .context("Failed to submit print job to server")?;

            let status_code = res.status();
            let response_body: PrintJobResponse = res.json().await?;

            if !status_code.is_success() || !response_body.success {
                return Err(anyhow!("Print rejected: {}", response_body.message));
            }

            let job_id = response_body
                .job_id
                .ok_or_else(|| anyhow!("Server did not return a job ID"))?;

            println!("✓ Job submitted successfully! (Job ID: {})", job_id);

            if wait {
                print!("Status: Spooling...");
                io::stdout().flush().ok();

                loop {
                    sleep(Duration::from_millis(800)).await;
                    let job_res = client
                        .get(format!("{}/api/v1/jobs/{}", base_url, job_id))
                        .send()
                        .await;

                    if let Ok(resp) = job_res {
                        if let Ok(Some(job)) = resp.json::<Option<JobInfo>>().await {
                            match job.status {
                                JobStatus::Queued => {
                                    print!("\rStatus: Queued in line...   ");
                                    io::stdout().flush().ok();
                                }
                                JobStatus::Processing => {
                                    print!("\rStatus: Rendering & rasterizing... ");
                                    io::stdout().flush().ok();
                                }
                                JobStatus::Printing => {
                                    print!("\rStatus: Printing to hardware...    ");
                                    io::stdout().flush().ok();
                                }
                                JobStatus::Completed => {
                                    println!("\rStatus: COMPLETED! Document printed successfully. 🎉");
                                    break;
                                }
                                JobStatus::Failed => {
                                    println!(
                                        "\rStatus: FAILED. Error: {}",
                                        job.error_message.as_deref().unwrap_or("Unknown error")
                                    );
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

/// Automatically install discovered printers into the client OS so Ctrl+P sees them
fn install_printers_locally(servers: &[(String, String, String)]) -> Result<()> {
    for (service_name, url, hardware_printer) in servers {
        let display_name = if !service_name.is_empty() {
            service_name.clone()
        } else {
            format!("{} (Office)", hardware_printer)
        };
        let ipp_url = format!("{}/ipp/print", url);

        #[cfg(windows)]
        {
            println!("Registering Windows printer '{}' -> {}...", display_name, ipp_url);
            let ps_cmd = format!(
                "Add-Printer -Name '{}' -DriverName 'Microsoft IPP Class Driver' -PortName '{}' -ErrorAction SilentlyContinue",
                display_name, ipp_url
            );
            let _ = Command::new("powershell")
                .arg("-NoProfile")
                .arg("-Command")
                .arg(&ps_cmd)
                .output();
            println!("✓ Installed '{}' in Windows!", display_name);
        }

        #[cfg(not(windows))]
        {
            let safe_queue_name = display_name
                .replace(" ", "_")
                .replace("(", "")
                .replace(")", "")
                .replace("/", "_");
            println!("Registering CUPS printer '{}' -> {}...", safe_queue_name, ipp_url);
            let _ = Command::new("lpadmin")
                .arg("-p")
                .arg(&safe_queue_name)
                .arg("-E")
                .arg("-v")
                .arg(&ipp_url)
                .arg("-D")
                .arg(&display_name)
                .arg("-m")
                .arg("everywhere")
                .output();
            println!("✓ Installed '{}' in CUPS!", display_name);
        }
    }

    println!("\nAll printers installed! You can now press Ctrl + P in any app and select them.");
    Ok(())
}

/// Prompt or fetch the department PIN
fn resolve_pin(cli_pin: Option<String>) -> Result<String> {
    if let Some(p) = cli_pin {
        return Ok(p);
    }
    if let Ok(env_pin) = std::env::var("RUSTY_PRINT_PIN") {
        if !env_pin.trim().is_empty() {
            return Ok(env_pin.trim().to_string());
        }
    }

    print!("Enter Department PIN / Passkey: ");
    io::stdout().flush()?;
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;
    let trimmed = buffer.trim().to_string();
    if trimmed.is_empty() {
        return Err(anyhow!("A PIN is required to print."));
    }
    Ok(trimmed)
}

/// Resolve server URL: custom flag, mDNS discovery, or default fallback
async fn resolve_server_url(custom: Option<String>) -> Result<String> {
    if let Some(url) = custom {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Ok(format!("http://{}", trimmed));
        }
        return Ok(trimmed);
    }

    // Try discovering via mDNS
    let discovered = tokio::task::spawn_blocking(|| {
        discover_servers(Duration::from_millis(1500))
    })
    .await??;

    if let Some((_, url, _)) = discovered.into_iter().next() {
        return Ok(url);
    }

    // Fallback default
    Ok("http://office-printer.local:8989".to_string())
}

/// Scan network for Rusty-Print instances
fn discover_servers(timeout: Duration) -> Result<Vec<(String, String, String)>> {
    let mdns = ServiceDaemon::new()?;
    let receiver = mdns.browse(MDNS_SERVICE_TYPE)?;
    let mut found = Vec::new();

    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(event) = receiver.recv_timeout(Duration::from_millis(200)) {
            if let ServiceEvent::ServiceResolved(info) = event {
                let fullname = info.get_fullname().to_string();
                let port = info.get_port();
                let printer = info
                    .get_property("printer")
                    .map(|v| v.val_str().to_string())
                    .unwrap_or_else(|| "Office Printer".to_string());

                let service_name = fullname
                    .split("._rustyprint")
                    .next()
                    .unwrap_or(&printer)
                    .to_string();

                let hostname = info.get_hostname().trim_end_matches('.');
                let mut ip_str = hostname.to_string();
                if let Some(ip) = info.get_addresses().iter().next() {
                    ip_str = ip.to_string();
                }

                let url = format!("http://{}:{}", ip_str, port);
                found.push((service_name, url, printer));
            }
        }
    }

    Ok(found)
}
