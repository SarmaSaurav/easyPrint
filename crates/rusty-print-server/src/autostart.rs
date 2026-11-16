use anyhow::Result;
use tracing::info;

#[cfg(windows)]
use windows_registry::CURRENT_USER;

#[cfg(windows)]
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
#[cfg(windows)]
const APP_NAME: &str = "RustyPrintServer";

/// Check if autostart on Windows logon (or Linux XDG autostart) is enabled
pub fn is_autostart_enabled() -> bool {
    #[cfg(windows)]
    {
        if let Ok(key) = CURRENT_USER.open(RUN_KEY) {
            key.get_string(APP_NAME).is_ok()
        } else {
            false
        }
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path = std::path::Path::new(&home).join(".config/autostart/rusty-print-server.desktop");
            path.exists()
        } else {
            false
        }
    }
}

/// Enable autostart
pub fn enable_autostart() -> Result<()> {
    #[cfg(windows)]
    {
        let current_exe = std::env::current_exe()?;
        let path_str = current_exe.to_string_lossy();
        let run_cmd = format!("\"{}\" --minimized", path_str);

        let key = CURRENT_USER.create(RUN_KEY)?;
        key.set_string(APP_NAME, &run_cmd)?;
        info!("Autostart enabled in Windows Registry: {}", run_cmd);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let dir = std::path::Path::new(&home).join(".config/autostart");
            let _ = std::fs::create_dir_all(&dir);
            let current_exe = std::env::current_exe()?;
            let content = format!(
                "[Desktop Entry]\nType=Application\nName=Rusty-Print Server\nExec={}\nHidden=false\nNoDisplay=false\nX-GNOME-Autostart-enabled=true\n",
                current_exe.display()
            );
            std::fs::write(dir.join("rusty-print-server.desktop"), content)?;
            info!("Autostart enabled in Linux ~/.config/autostart");
        }
        Ok(())
    }
}

/// Disable autostart
pub fn disable_autostart() -> Result<()> {
    #[cfg(windows)]
    {
        if let Ok(key) = CURRENT_USER.create(RUN_KEY) {
            let _ = key.remove_value(APP_NAME);
            info!("Autostart disabled in Windows Registry");
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        if let Ok(home) = std::env::var("HOME") {
            let path = std::path::Path::new(&home).join(".config/autostart/rusty-print-server.desktop");
            let _ = std::fs::remove_file(path);
            info!("Autostart disabled on Linux");
        }
        Ok(())
    }
}

/// Toggle autostart and return new state
pub fn toggle_autostart() -> Result<bool> {
    if is_autostart_enabled() {
        disable_autostart()?;
        Ok(false)
    } else {
        enable_autostart()?;
        Ok(true)
    }
}
