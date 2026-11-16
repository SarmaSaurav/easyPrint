use crate::autostart::{is_autostart_enabled, toggle_autostart};
use crate::config::ServerConfig;
use crate::printer::list_printers;
use anyhow::Result;
use qrcode::QrCode;
use std::ffi::OsStr;
use std::net::UdpSocket;
use std::os::windows::ffi::OsStrExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tracing::info;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreateSolidBrush, DeleteObject, EndPaint, FillRect,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLOR_BTNFACE, DEFAULT_CHARSET, DEFAULT_PITCH,
    FF_DONTCARE, HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT,
};
use windows::Win32::UI::Shell::{
    ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DispatchMessageW, GetCursorPos, GetMessageW, GetWindowTextLengthW, GetWindowTextW,
    LoadIconW, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    ShowWindow, TrackPopupMenu, TranslateMessage, BM_SETCHECK, BS_AUTOCHECKBOX, CBS_DROPDOWNLIST,
    CB_ADDSTRING, CB_SETCURSEL, CS_HREDRAW, CS_VREDRAW, CW_USEDEFAULT, ES_AUTOHSCROLL,
    HMENU, IDI_APPLICATION, MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, MSG, SW_HIDE,
    SW_RESTORE, SW_SHOW, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_PAINT, WM_RBUTTONUP, WM_SETFONT,
    WM_USER, WNDCLASSW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_MINIMIZEBOX, WS_OVERLAPPED,
    WS_SYSMENU, WS_VISIBLE,
};

const WM_TRAYICON: u32 = WM_USER + 101;

const BST_CHECKED: usize = 1;
const BST_UNCHECKED: usize = 0;

// Control IDs
const ID_BTN_OPEN_WEB: usize = 1001;
const ID_BTN_SAVE_PIN: usize = 1002;
const ID_CHK_AUTOSTART: usize = 1003;
const ID_BTN_HIDE_TRAY: usize = 1004;
const ID_TXT_PIN: usize = 1005;
const ID_CMB_PRINTER: usize = 1006;

// Tray menu IDs
const ID_TRAY_OPEN_PANEL: usize = 2001;
const ID_TRAY_OPEN_WEB: usize = 2002;
const ID_TRAY_AUTOSTART: usize = 2003;
const ID_TRAY_EXIT: usize = 2004;

static SHOULD_EXIT: AtomicBool = AtomicBool::new(false);

struct GuiState {
    config: Arc<ServerConfig>,
    h_pin_input: HWND,
    _h_printer_cmb: HWND,
    h_autostart_chk: HWND,
    nid: NOTIFYICONDATAW,
    local_ip: String,
    qr_data: Option<(Vec<bool>, usize)>,
    hfont_regular: HFONT,
    hfont_bold: HFONT,
    hfont_title: HFONT,
}

unsafe impl Send for GuiState {}
unsafe impl Sync for GuiState {}

static GLOBAL_STATE: Mutex<Option<GuiState>> = Mutex::new(None);

/// Automatically discover the local network IPv4 address (e.g. 192.168.1.9)
pub fn get_local_ip() -> String {
    if let Ok(socket) = UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("8.8.8.8:80").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                let ip = addr.ip().to_string();
                if !ip.is_empty() && ip != "0.0.0.0" {
                    return ip;
                }
            }
        }
    }
    "127.0.0.1".to_string()
}

pub fn run_gui(config: Arc<ServerConfig>, start_minimized: bool) -> Result<()> {
    unsafe {
        let class_name = w!("RustyPrintModernControlPanel");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wndproc),
            hInstance: HINSTANCE::default(),
            lpszClassName: class_name,
            hCursor: windows::Win32::UI::WindowsAndMessaging::HCURSOR::default(),
            hbrBackground: HBRUSH((COLOR_BTNFACE.0 + 1) as _),
            ..Default::default()
        };

        let _ = RegisterClassW(&wc);

        let window_title = w!("Rusty-Print Bridge Control Panel");
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX;

        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            class_name,
            window_title,
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            560,
            460,
            HWND::default(),
            HMENU::default(),
            HINSTANCE::default(),
            None,
        )?;

        if hwnd.is_invalid() {
            return Err(anyhow::anyhow!("Failed to create GUI control window"));
        }

        // Setup controls with modern typography & QR code
        setup_controls(hwnd, &config);

        // Setup System Tray Icon
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAYICON,
            hIcon: LoadIconW(HINSTANCE::default(), IDI_APPLICATION).unwrap_or_default(),
            ..Default::default()
        };

        let tip_str = format!("Rusty-Print Bridge (Port {})\0", config.port);
        let tip_wide: Vec<u16> = tip_str.encode_utf16().collect();
        let copy_len = tip_wide.len().min(nid.szTip.len());
        nid.szTip[..copy_len].copy_from_slice(&tip_wide[..copy_len]);

        let _ = Shell_NotifyIconW(NIM_ADD, &nid);

        if let Ok(mut lock) = GLOBAL_STATE.lock() {
            if let Some(state) = lock.as_mut() {
                state.nid = nid;
            }
        }

        if !start_minimized {
            let _ = ShowWindow(hwnd, SW_SHOW);
        } else {
            let _ = ShowWindow(hwnd, SW_HIDE);
            info!("Server started minimized to system tray");
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, HWND::default(), 0, 0).into() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);

            if SHOULD_EXIT.load(Ordering::SeqCst) {
                break;
            }
        }

        // Clean up tray icon and fonts
        if let Ok(mut lock) = GLOBAL_STATE.lock() {
            if let Some(state) = lock.take() {
                let _ = Shell_NotifyIconW(NIM_DELETE, &state.nid);
                let _ = DeleteObject(state.hfont_regular);
                let _ = DeleteObject(state.hfont_bold);
                let _ = DeleteObject(state.hfont_title);
            }
        }
    }

    Ok(())
}

unsafe fn create_segoe_font(size: i32, bold: bool) -> HFONT {
    let font_name = w!("Segoe UI");
    CreateFontW(
        -size,
        0,
        0,
        0,
        if bold { 700 } else { 400 },
        0,
        0,
        0,
        DEFAULT_CHARSET.0 as _,
        OUT_DEFAULT_PRECIS.0 as _,
        CLIP_DEFAULT_PRECIS.0 as _,
        CLEARTYPE_QUALITY.0 as _,
        (DEFAULT_PITCH.0 | FF_DONTCARE.0) as _,
        font_name,
    )
}

unsafe fn setup_controls(hwnd: HWND, config: &Arc<ServerConfig>) {
    let inst = HINSTANCE::default();

    let local_ip = get_local_ip();
    let direct_url = format!("http://{}:{}", local_ip, config.port);

    // Generate QR Code for instant phone access
    let qr_data = if let Ok(qr) = QrCode::new(direct_url.as_bytes()) {
        let width = qr.width();
        let colors: Vec<bool> = qr.into_colors().into_iter().map(|c| c == qrcode::Color::Dark).collect();
        Some((colors, width))
    } else {
        None
    };

    // Create modern Segoe UI fonts
    let hfont_title = create_segoe_font(18, true);
    let hfont_bold = create_segoe_font(13, true);
    let hfont_regular = create_segoe_font(13, false);

    // 1. Title Label
    let title_wide = to_wide("Rusty-Print Wireless Bridge\0");
    let h_title = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(title_wide.as_ptr()),
        WS_VISIBLE | WS_CHILD,
        22,
        16,
        340,
        26,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_title, WM_SETFONT, WPARAM(hfont_title.0 as _), LPARAM(1));

    // 2. Status Label (Green Active indicator)
    let status_text = format!("● Online on port {}\0", config.port);
    let status_wide = to_wide(&status_text);
    let h_status = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(status_wide.as_ptr()),
        WS_VISIBLE | WS_CHILD,
        24,
        45,
        340,
        20,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_status, WM_SETFONT, WPARAM(hfont_bold.0 as _), LPARAM(1));

    // 3. Android / Phone Direct Address Label
    let phone_label_text = format!("📱 Phone URL (Android/iOS): {}\0", direct_url);
    let phone_label_wide = to_wide(&phone_label_text);
    let h_phone_url = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(phone_label_wide.as_ptr()),
        WS_VISIBLE | WS_CHILD,
        24,
        74,
        350,
        20,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_phone_url, WM_SETFONT, WPARAM(hfont_bold.0 as _), LPARAM(1));

    // 4. Laptop / PC mDNS URL Label
    let m_text = format!("💻 Laptop URL (PC/Mac): http://office-printer.local:{}\0", config.port);
    let m_wide = to_wide(&m_text);
    let h_mdns = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        PCWSTR(m_wide.as_ptr()),
        WS_VISIBLE | WS_CHILD,
        24,
        96,
        350,
        20,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_mdns, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    // 5. Button: Open Web Portal
    let h_btn_web = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("🌐 Open Web Portal in Browser"),
        WS_VISIBLE | WS_CHILD,
        24,
        125,
        330,
        34,
        hwnd,
        HMENU(ID_BTN_OPEN_WEB as _),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_btn_web, WM_SETFONT, WPARAM(hfont_bold.0 as _), LPARAM(1));

    // 6. Target Printer Label
    let h_lbl_p = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("Target Printer:"),
        WS_VISIBLE | WS_CHILD,
        24,
        175,
        110,
        20,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_lbl_p, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    // 7. Target Printer ComboBox
    let h_printer_cmb = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("COMBOBOX"),
        PCWSTR::null(),
        WS_VISIBLE | WS_CHILD | WS_BORDER | WINDOW_STYLE(CBS_DROPDOWNLIST as _),
        140,
        172,
        375,
        200,
        hwnd,
        HMENU(ID_CMB_PRINTER as _),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_printer_cmb, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    // Populate printers
    if let Ok(printers) = list_printers() {
        let default_wide = to_wide("(Auto-Detect Default Printer)\0");
        let _ = SendMessageW(
            h_printer_cmb,
            CB_ADDSTRING,
            WPARAM::default(),
            LPARAM(default_wide.as_ptr() as _),
        );

        let mut sel_idx = 0;
        for (i, p) in printers.iter().enumerate() {
            let label = if p.is_default {
                format!("{} (Default)\0", p.name)
            } else {
                format!("{}\0", p.name)
            };
            let wide = to_wide(&label);
            let _ = SendMessageW(
                h_printer_cmb,
                CB_ADDSTRING,
                WPARAM::default(),
                LPARAM(wide.as_ptr() as _),
            );

            if let Some(pref) = &config.preferred_printer {
                if pref.eq_ignore_ascii_case(&p.name) {
                    sel_idx = i + 1;
                }
            }
        }
        let _ = SendMessageW(h_printer_cmb, CB_SETCURSEL, WPARAM(sel_idx), LPARAM::default());
    }

    // 8. Department PIN Label
    let h_lbl_pin = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("Department PIN:"),
        WS_VISIBLE | WS_CHILD,
        24,
        220,
        110,
        20,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_lbl_pin, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    // 9. PIN Text Input
    let pin_wide = to_wide(&format!("{}\0", config.pin));
    let h_pin_input = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("EDIT"),
        PCWSTR(pin_wide.as_ptr()),
        WS_VISIBLE | WS_CHILD | WS_BORDER | WINDOW_STYLE(ES_AUTOHSCROLL as _),
        140,
        217,
        230,
        28,
        hwnd,
        HMENU(ID_TXT_PIN as _),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_pin_input, WM_SETFONT, WPARAM(hfont_bold.0 as _), LPARAM(1));

    // 10. Button: Save PIN
    let h_btn_pin = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("Save PIN"),
        WS_VISIBLE | WS_CHILD,
        385,
        216,
        130,
        30,
        hwnd,
        HMENU(ID_BTN_SAVE_PIN as _),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_btn_pin, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    // 11. Checkbox: Autostart on Boot
    let h_autostart_chk = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("Start automatically on Windows boot (minimized to tray)"),
        WS_VISIBLE | WS_CHILD | WINDOW_STYLE(BS_AUTOCHECKBOX as _),
        24,
        265,
        490,
        25,
        hwnd,
        HMENU(ID_CHK_AUTOSTART as _),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_autostart_chk, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    let is_auto = is_autostart_enabled();
    let check_state = if is_auto { BST_CHECKED } else { BST_UNCHECKED };
    let _ = SendMessageW(h_autostart_chk, BM_SETCHECK, WPARAM(check_state), LPARAM::default());

    // 12. Button: Minimize to Tray
    let h_btn_tray = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("BUTTON"),
        w!("Hide to System Tray (Keep running in background)"),
        WS_VISIBLE | WS_CHILD,
        24,
        310,
        490,
        36,
        hwnd,
        HMENU(ID_BTN_HIDE_TRAY as _),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_btn_tray, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    // 13. QR Caption Label
    let h_qr_cap = CreateWindowExW(
        WINDOW_EX_STYLE::default(),
        w!("STATIC"),
        w!("Scan with phone:"),
        WS_VISIBLE | WS_CHILD,
        390,
        16,
        130,
        18,
        hwnd,
        HMENU::default(),
        inst,
        None,
    ).unwrap_or_default();
    SendMessageW(h_qr_cap, WM_SETFONT, WPARAM(hfont_regular.0 as _), LPARAM(1));

    if let Ok(mut lock) = GLOBAL_STATE.lock() {
        *lock = Some(GuiState {
            config: config.clone(),
            h_pin_input,
            _h_printer_cmb: h_printer_cmb,
            h_autostart_chk,
            nid: NOTIFYICONDATAW::default(),
            local_ip,
            qr_data,
            hfont_regular,
            hfont_bold,
            hfont_title,
        });
    }
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            let mut ps = PAINTSTRUCT::default();
            let hdc = BeginPaint(hwnd, &mut ps);

            // Draw QR code if available
            if let Ok(lock) = GLOBAL_STATE.lock() {
                if let Some(state) = lock.as_ref() {
                    if let Some((modules, width)) = &state.qr_data {
                        draw_qr_code(hdc, 390, 36, 120, modules, *width);
                    }
                }
            }

            let _ = EndPaint(hwnd, &ps);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xffff) as usize;
            match id {
                ID_BTN_OPEN_WEB | ID_TRAY_OPEN_WEB => {
                    let url = w!("http://localhost:8989");
                    let op = w!("open");
                    let _ = ShellExecuteW(
                        hwnd,
                        op,
                        url,
                        PCWSTR::null(),
                        PCWSTR::null(),
                        windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
                    );
                }
                ID_TRAY_OPEN_PANEL => {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                }
                ID_BTN_HIDE_TRAY => {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                }
                ID_CHK_AUTOSTART | ID_TRAY_AUTOSTART => {
                    if let Ok(new_val) = toggle_autostart() {
                        if let Ok(lock) = GLOBAL_STATE.lock() {
                            if let Some(state) = lock.as_ref() {
                                let check_state = if new_val { BST_CHECKED } else { BST_UNCHECKED };
                                let _ = SendMessageW(
                                    state.h_autostart_chk,
                                    BM_SETCHECK,
                                    WPARAM(check_state),
                                    LPARAM::default(),
                                );
                            }
                        }
                    }
                }
                ID_BTN_SAVE_PIN => {
                    if let Ok(lock) = GLOBAL_STATE.lock() {
                        if let Some(state) = lock.as_ref() {
                            let len = GetWindowTextLengthW(state.h_pin_input) as usize;
                            if len > 0 {
                                let mut buf = vec![0u16; len + 1];
                                let _ = GetWindowTextW(state.h_pin_input, &mut buf);
                                let new_pin = String::from_utf16_lossy(&buf[..len]).trim().to_string();

                                // Update config file
                                let mut cfg = (*state.config).clone();
                                cfg.pin = new_pin.clone();
                                if let Ok(toml_str) = toml::to_string_pretty(&cfg) {
                                    let _ = std::fs::write("config.toml", toml_str);
                                    info!("PIN updated to '{}' and saved to config.toml", new_pin);
                                }
                            }
                        }
                    }
                }
                ID_TRAY_EXIT => {
                    SHOULD_EXIT.store(true, Ordering::SeqCst);
                    let _ = ShowWindow(hwnd, SW_HIDE);
                    PostQuitMessage(0);
                }
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAYICON => {
            let event = (lparam.0 & 0xffff) as u32;
            if event == WM_LBUTTONUP {
                // Left click toggles control panel window
                let is_visible = windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(hwnd).as_bool();
                if is_visible {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                    let _ = SetForegroundWindow(hwnd);
                }
            } else if event == WM_RBUTTONUP {
                // Right click opens tray popup menu
                let mut pt = POINT::default();
                let _ = GetCursorPos(&mut pt);

                let hmenu = CreatePopupMenu().unwrap_or_default();
                let autostart_flag = if is_autostart_enabled() { MF_CHECKED } else { MF_UNCHECKED };

                let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN_PANEL, w!("Open Control Panel"));
                let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_OPEN_WEB, w!("Open Web Portal in Browser"));
                let _ = AppendMenuW(hmenu, MF_STRING | autostart_flag, ID_TRAY_AUTOSTART, w!("Autostart on Boot"));
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
                let _ = AppendMenuW(hmenu, MF_STRING, ID_TRAY_EXIT, w!("Exit Rusty-Print"));

                let _ = SetForegroundWindow(hwnd);
                let _ = TrackPopupMenu(
                    hmenu,
                    TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                    pt.x,
                    pt.y,
                    0,
                    hwnd,
                    None,
                );
                let _ = DestroyMenu(hmenu);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            // Intercept close button: hide to tray instead of quitting!
            let _ = ShowWindow(hwnd, SW_HIDE);
            LRESULT(0)
        }
        WM_DESTROY => {
            SHOULD_EXIT.store(true, Ordering::SeqCst);
            PostQuitMessage(0);
            LRESULT(0)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

/// Paint the QR code onto the HDC
unsafe fn draw_qr_code(hdc: HDC, x: i32, y: i32, size: i32, modules: &[bool], width: usize) {
    let white_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
    let black_brush = CreateSolidBrush(COLORREF(0x00000000));

    // Draw white background
    let bg_rect = RECT {
        left: x,
        top: y,
        right: x + size,
        bottom: y + size,
    };
    FillRect(hdc, &bg_rect, white_brush);

    let cell_size = (size as f32 / width as f32).max(1.0);

    for row in 0..width {
        for col in 0..width {
            if modules[row * width + col] {
                let cell_rect = RECT {
                    left: x + (col as f32 * cell_size) as i32,
                    top: y + (row as f32 * cell_size) as i32,
                    right: x + ((col + 1) as f32 * cell_size).ceil() as i32,
                    bottom: y + ((row + 1) as f32 * cell_size).ceil() as i32,
                };
                FillRect(hdc, &cell_rect, black_brush);
            }
        }
    }

    let _ = DeleteObject(white_brush);
    let _ = DeleteObject(black_brush);
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}
