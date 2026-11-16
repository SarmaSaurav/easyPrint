#!/bin/bash
set -e

PROJECT_ROOT="/mnt/d/the_printer_project"
DIST_DIR="$PROJECT_ROOT/dist"
TEMP_BUILD="/tmp/rusty_print_build"
PKG_DIR="$TEMP_BUILD/rusty-print_0.1.0_amd64"

echo "=== Building .deb package for Zorin OS / Linux Mint ==="

rm -rf "$TEMP_BUILD"
mkdir -p "$PKG_DIR/DEBIAN"
mkdir -p "$PKG_DIR/usr/bin"
mkdir -p "$PKG_DIR/usr/share/applications"
mkdir -p "$PKG_DIR/usr/share/doc/rusty-print"
mkdir -p "$PKG_DIR/lib/systemd/system"
mkdir -p "$PKG_DIR/var/lib/rusty-print"
mkdir -p "$DIST_DIR"

# 1. Copy client binary and strip debug symbols
cp "$PROJECT_ROOT/target/release/rusty-print-client" "$PKG_DIR/usr/bin/rusty-print"
strip "$PKG_DIR/usr/bin/rusty-print"
chmod 755 "$PKG_DIR/usr/bin/rusty-print"

# 2. Copy server binary (if built) and strip debug symbols
if [ -f "$PROJECT_ROOT/target/release/rusty-print-server" ]; then
    cp "$PROJECT_ROOT/target/release/rusty-print-server" "$PKG_DIR/usr/bin/rusty-print-server"
    strip "$PKG_DIR/usr/bin/rusty-print-server"
    chmod 755 "$PKG_DIR/usr/bin/rusty-print-server"
    echo "✓ Included rusty-print-server binary in package"
fi

# 3. Desktop helper script
cat > "$PKG_DIR/usr/bin/rusty-print-gui" << 'EOF'
#!/bin/bash
if [ -n "$1" ]; then
    if command -v zenity >/dev/null 2>&1; then
        PIN=$(zenity --entry --title="Rusty-Print" --text="Enter Department PIN:" --hide-text)
        if [ -n "$PIN" ]; then
            OUTPUT=$(rusty-print print "$1" --pin "$PIN" 2>&1)
            zenity --info --title="Rusty-Print" --text="$OUTPUT"
        fi
    else
        x-terminal-emulator -e bash -c "rusty-print print \"$1\"; echo; read -p 'Press Enter to exit...'"
    fi
else
    xdg-open "http://localhost:8989" || sensible-browser "http://localhost:8989"
fi
EOF
chmod 755 "$PKG_DIR/usr/bin/rusty-print-gui"

# 4. Systemd unit for rusty-print-server
cat > "$PKG_DIR/lib/systemd/system/rusty-print-server.service" << 'EOF'
[Unit]
Description=Rusty-Print Wireless Bridge Server Daemon
After=network.target cups.service
Wants=cups.service

[Service]
Type=simple
ExecStart=/usr/bin/rusty-print-server --headless
Restart=on-failure
RestartSec=3
User=root
WorkingDirectory=/var/lib/rusty-print

[Install]
WantedBy=multi-user.target
EOF
chmod 644 "$PKG_DIR/lib/systemd/system/rusty-print-server.service"

# 5. Desktop Entry for Linux Mint / Zorin OS application menu
cat > "$PKG_DIR/usr/share/applications/rusty-print.desktop" << 'EOF'
[Desktop Entry]
Name=Office Wireless Printer
Comment=Send print jobs wirelessly to the host printer
Exec=rusty-print-gui %U
Icon=printer
Terminal=false
Type=Application
Categories=Office;Utility;Printing;
Keywords=printer;print;wireless;canon;office;
MimeType=application/pdf;image/png;image/jpeg;
Actions=InstallPrinters;Status;WebPortal;

[Desktop Action InstallPrinters]
Name=Auto-Install Office Printers (Ctrl+P)
Exec=x-terminal-emulator -e bash -c "rusty-print install-printers; echo; read -p 'Press Enter to exit...'"

[Desktop Action Status]
Name=Check Printer Status
Exec=x-terminal-emulator -e bash -c "rusty-print status; echo; read -p 'Press Enter to exit...'"

[Desktop Action WebPortal]
Name=Open Web Print Portal
Exec=xdg-open http://localhost:8989
EOF
chmod 644 "$PKG_DIR/usr/share/applications/rusty-print.desktop"

# 6. Debian Control file
cat > "$PKG_DIR/DEBIAN/control" << 'EOF'
Package: rusty-print
Version: 0.1.0
Section: utils
Priority: optional
Architecture: amd64
Maintainer: Rusty-Print Project <support@office.local>
Depends: libc6 (>= 2.27), cups-client
Recommends: xdg-utils, avahi-daemon
Description: Native wireless print bridge client and server for office printers
 Rusty-Print is a high-performance cross-platform bridge designed for
 Zorin OS, Linux Mint, Ubuntu, and Windows. It auto-discovers office print
 bridges over Wi-Fi using mDNS (Zeroconf), eliminating broken Windows print shares
 and dynamic DHCP IP changes. Allows native Ctrl+P printing to host USB printers.
EOF
chmod 644 "$PKG_DIR/DEBIAN/control"

# 7. Post-install script
cat > "$PKG_DIR/DEBIAN/postinst" << 'EOF'
#!/bin/sh
set -e
if command -v systemctl >/dev/null 2>&1; then
    systemctl daemon-reload || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q || true
fi
exit 0
EOF
chmod 755 "$PKG_DIR/DEBIAN/postinst"

# 8. Set directory permissions strictly for dpkg-deb
chmod 755 "$PKG_DIR/DEBIAN"
chmod -R u=rwX,go=rX "$PKG_DIR"
chmod 755 "$PKG_DIR/DEBIAN/postinst"
chmod 755 "$PKG_DIR/usr/bin/rusty-print"
[ -f "$PKG_DIR/usr/bin/rusty-print-server" ] && chmod 755 "$PKG_DIR/usr/bin/rusty-print-server"
chmod 755 "$PKG_DIR/usr/bin/rusty-print-gui"

# 9. Build .deb package using dpkg-deb
cd "$TEMP_BUILD"
dpkg-deb --build --root-owner-group "rusty-print_0.1.0_amd64"
cp "$TEMP_BUILD/rusty-print_0.1.0_amd64.deb" "$DIST_DIR/rusty-print_0.1.0_amd64.deb"
rm -rf "$TEMP_BUILD"

echo "Package successfully generated at: $DIST_DIR/rusty-print_0.1.0_amd64.deb"
