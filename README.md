# Rusty-Print: Office Wireless Print Bridge

High-performance, zero-configuration wireless print bridge written in Rust. Seamlessly shares USB-connected host printers (Canon PIXMA, Brother, HP, Epson) across your office Wi-Fi, completely replacing broken Windows SMB/RPC print shares and eliminating problems caused by dynamic router DHCP IP changes.

---

## 🌟 Architecture Overview

```
                      ┌──────────────────────────────────────┐
                      │        Office Wi-Fi Router           │
                      │   (DHCP IPs can change anytime)      │
                      └──────────────────┬───────────────────┘
                                         │
                 ┌───────────────────────┴───────────────────────┐
                 │                                               │
     ┌───────────▼───────────┐                       ┌───────────▼───────────┐
     │  Windows Desktop Host │                       │ Linux Mint Host (PC2) │
     │  (Canon G2070 / USB)  │                       │   (USB Office Printer)│
     │  `rusty-print-server` │                       │  `rusty-print-server` │
     │  mDNS: _ipp._tcp      │                       │  mDNS: _ipp._tcp      │
     └───────────┬───────────┘                       └───────────┬───────────┘
                 │                                               │
                 └───────────────────────┬───────────────────────┘
                                         │
        ┌────────────────────────────────┼────────────────────────────────┐
        │                                │                                │
┌───────▼────────┐              ┌────────▼────────┐              ┌────────▼────────┐
│ Windows Laptop │              │ Zorin OS / Mint │              │  Android / iOS  │
│  Native Ctrl+P │              │  Native Ctrl+P  │              │ Scannable QR UI │
│ (Auto-injected)│              │  (Driverless)   │              │  Web Portal     │
└────────────────┘              └─────────────────┘              └─────────────────┘
```

---

## 📦 What Was Built & Where to Find Files

| Platform | File Path | Description |
| :--- | :--- | :--- |
| **Windows Host Server** | `dist\windows\rusty-print-server.exe` | Zero-console GUI control panel, system tray, and IPP responder |
| **Windows Client Utility**| `dist\windows\rusty-print-client.exe` | Discovers bridges and auto-injects printers into Windows `Ctrl + P` |
| **Debian / Ubuntu / Mint / Zorin** | `dist\rusty-print_0.1.0_amd64.deb` | Combined `.deb` package containing both `rusty-print` (client) and `rusty-print-server` (daemon) |
| **Android App** | `dist\android\rusty-print.apk` | Native Android Print Service Plugin — enables direct Share ➔ Print from any Android app |

---

## 🖥️ Setup 1: Host 1 (Windows Desktop with USB Printer)

1. Copy the `dist\windows` folder to the Windows host desktop.
2. Double-click `rusty-print-server.exe`:
   - **No dark terminal opens!** It launches directly into the modern GUI control panel.
   - Shows live server status and scannable QR code for instant mobile access.
   - Target printer is automatically detected (e.g., your Canon or Brother USB printer).
3. Check the box **"Start automatically on Windows boot"**:
   - The server will now run silently in the background on every reboot.
4. Click **"Hide to System Tray"** (or click the `X` button):
   - It will minimize safely to the system tray near the clock.

---

## 🐧 Setup 2: Host 2 (Linux Mint Desktop with USB Printer)

1. Copy `dist/rusty-print_0.1.0_amd64.deb` to the Linux Mint desktop.
2. Install the package:
   ```bash
   sudo dpkg -i rusty-print_0.1.0_amd64.deb
   sudo apt-get install -f -y
   ```
3. Enable and start the background print service:
   ```bash
   sudo systemctl enable --now rusty-print-server
   ```
   - The Linux host will automatically detect your connected USB printer via CUPS and advertise it across the office network.

---

## 💻 Setup 3: Office Client Laptops (Enabling Native Ctrl + P)

### On Windows Laptops:
To make both office printers appear in the standard Windows `Ctrl + P` print menu:
1. Open PowerShell and run:
   ```powershell
   .\rusty-print-client.exe install-printers
   ```
2. The client will scan the Wi-Fi via mDNS, discover both hosts, and register them directly with Windows Spooler using the native Microsoft IPP driver.
3. Done! Now whenever anyone presses `Ctrl + P` in Word, Chrome, Adobe, or Excel, both printers appear in the printer list.

### On Zorin OS / Linux Mint / Ubuntu Laptops:
1. Install the `.deb` package:
   ```bash
   sudo dpkg -i rusty-print_0.1.0_amd64.deb
   ```
2. Run the one-step auto-installer:
   ```bash
   rusty-print install-printers
   ```
   *(Or click "Auto-Install Office Printers" in your Application Menu!)*
3. Both printers are now registered in CUPS driverless printing. Any app (`Ctrl + P`) will display both printers immediately.

---

## 📱 Setup 4: Android Mobile Printing

### Method 1: Native Print Service App (Recommended — Zero Friction)
1. Install `dist/android/rusty-print.apk` on Android phones.
2. Open **Rusty Print** once and tap **"Turn ON in Android Settings"** to enable the print service. (Optionally enter and save your Department PIN).
3. From then on, print from ANY Android app (Chrome, WhatsApp, Photos, Drive):
   - Tap **Share ➔ Print** (or menu `⋮` ➔ **Print**).
   - Your office printers appear automatically in the printer dropdown list.
   - Tap **Print** — done!

### Method 2: Mobile Web Portal (No APK needed)
1. Open Chrome or Firefox on your phone while connected to office Wi-Fi.
2. Enter the host IP in the address bar: `http://<host-ip>:8989` (e.g. `http://192.168.1.9:8989`).
3. Select the file, enter your Department PIN, and tap **Print**. Tap `⋮` ➔ **"Add to Home Screen"** to save it as an instant web app icon.

---

## 🔄 Dynamic DHCP Resilience (Why it Never Breaks)

When the router restarts and the host machines get new IP addresses (e.g., from `.42` to `.45`):
- **Zero Configuration Needed**: The mDNS daemons dynamically broadcast their updated IPs across the subnet.
- If clients use the mDNS hostname (`http://<hostname>.local:8989`), the OS resolves the new IP immediately.
- If a client ever needs to resynchronize, running `rusty-print install-printers` updates the printer endpoints in 2 seconds.
- Printer names are **never hardcoded**: each host dynamically publishes its actual printer model name combined with its system hostname.
