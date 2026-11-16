#![cfg_attr(windows, windows_subsystem = "windows")]

mod api;
mod autostart;
mod config;
#[cfg(windows)]
mod gui;
mod ipp;
mod mdns;
mod printer;
mod queue;
mod web;

use anyhow::Result;
use api::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use config::ServerConfig;
use mdns::MdnsService;
use queue::PrintQueueManager;
use std::env;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::info;

fn main() -> Result<()> {
    // Catch any panics and log to file / show alert
    std::panic::set_hook(Box::new(|info| {
        let msg = format!("Rusty-Print PANIC: {}", info);
        let _ = std::fs::write("rusty_print_panic.log", &msg);
        #[cfg(windows)]
        unsafe {
            use windows::Win32::Foundation::HWND;
            use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};
            use windows::core::PCWSTR;
            let title: Vec<u16> = "Rusty-Print Error\0".encode_utf16().collect();
            let text: Vec<u16> = format!("{}\0", msg).encode_utf16().collect();
            let _ = MessageBoxW(
                HWND::default(),
                PCWSTR(text.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }));

    tracing_subscriber::fmt::init();

    let args: Vec<String> = env::args().collect();

    // Check CLI flags
    if args.contains(&"--install-autostart".to_string()) {
        autostart::enable_autostart()?;
        return Ok(());
    }

    if args.contains(&"--remove-autostart".to_string()) {
        autostart::disable_autostart()?;
        return Ok(());
    }

    let _start_minimized = args.contains(&"--minimized".to_string());
    let _headless = args.contains(&"--headless".to_string());

    // Create and enter Tokio multi-threaded runtime context
    let rt = tokio::runtime::Runtime::new()?;
    let _rt_guard = rt.enter();

    // Initialize Windows COM MTA for WinRT PDF rendering
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }

    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    let config_path = exe_dir.join("config.toml");
    let config = ServerConfig::load_or_create(&config_path)?;
    let config = Arc::new(config);

    info!("Starting Rusty-Print Server version {}", env!("CARGO_PKG_VERSION"));

    // Check available printers
    let default_name = printer::get_default_printer_name().unwrap_or_else(|_| "None".to_string());
    let active_printer = config
        .preferred_printer
        .clone()
        .unwrap_or_else(|| default_name.clone());

    let service_name = if config.service_name.trim().is_empty()
        || config.service_name == "Office Wireless Printer"
    {
        let hostname = mdns::get_system_hostname();
        format!("{} ({})", active_printer, hostname)
    } else {
        config.service_name.clone()
    };

    info!("Active target printer: '{}'", active_printer);
    info!("Network advertised service name: '{}'", service_name);

    // Initialize print queue manager
    let queue = PrintQueueManager::new();

    // Start mDNS advertisement (Custom + Driverless IPP)
    let _mdns = if config.mdns_enabled {
        match MdnsService::start(&service_name, config.port, &active_printer) {
            Ok(service) => Some(service),
            Err(e) => {
                tracing::warn!("Failed to start mDNS service: {:?}. Proceeding with IP access only.", e);
                None
            }
        }
    } else {
        None
    };

    let app_state = AppState {
        config: config.clone(),
        queue,
    };

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Standard IPP Endpoints (Driverless Ctrl+P from Linux CUPS, Windows, Android)
        .route("/ipp", post(api::handle_ipp))
        .route("/ipp/print", post(api::handle_ipp))
        .route("/printers/{name}", post(api::handle_ipp))
        // REST API
        .route("/api/v1/status", get(api::get_status))
        .route("/api/v1/printers", get(api::get_printers))
        .route("/api/v1/print", post(api::submit_print_job))
        .route("/api/v1/jobs", get(api::get_jobs))
        .route("/api/v1/jobs/{id}", get(api::get_job_by_id))
        // Web Portal UI
        .fallback(web::static_handler)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(app_state);

    let addr_str = format!("{}:{}", config.host, config.port);
    let addr: SocketAddr = addr_str.parse()?;

    // Bind TCP listener synchronously to ensure port is available before proceeding
    let listener = rt.block_on(async {
        tokio::net::TcpListener::bind(addr).await
    })?;
    info!("Rusty-Print Server successfully bound to http://{}", addr);

    // Spawn async HTTP server on Tokio background runtime
    let _server_handle = rt.spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("Server error: {:?}", e);
        }
    });

    #[cfg(windows)]
    if !_headless {
        // Run Windows modern GUI & System Tray event loop on main thread
        info!("Launching Windows System Tray & Modern Control Panel GUI...");
        gui::run_gui(config, _start_minimized)?;
        return Ok(());
    }

    // If headless or non-Windows, wait for termination signal
    rt.block_on(async {
        #[cfg(not(windows))]
        {
            let _ = tokio::signal::ctrl_c().await;
        }
        #[cfg(windows)]
        {
            if tokio::signal::ctrl_c().await.is_err() {
                // If running without a console, wait indefinitely
                std::future::pending::<()>().await;
            }
        }
        info!("Shutting down Rusty-Print Server...");
    });

    Ok(())
}
