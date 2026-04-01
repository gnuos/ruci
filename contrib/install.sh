#!/bin/bash
# Ruci CI Installation Script
set -e

echo "Installing Ruci CI..."

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo "Please run as root or with sudo"
    exit 1
fi

# Determine binary location
BINARY_PATH="${BINARY_PATH:-/usr/local/bin/rucid}"
CONFIG_PATH="${CONFIG_PATH:-/etc/ruci/ruci.yaml}"
DATA_DIR="/var/lib/ruci"
RUN_DIR="/var/run/ruci"
LOG_DIR="/var/log/ruci"

# Create directories
echo "Creating directories..."
mkdir -p "$DATA_DIR" "$RUN_DIR" "$LOG_DIR"

# Create user if it doesn't exist
if ! id -u ruci &>/dev/null; then
    echo "Creating user 'ruci'..."
    useradd -r -s /sbin/nologin -d "$DATA_DIR" ruci
fi

# Set ownership
echo "Setting ownership..."
chown -R ruci:ruci "$DATA_DIR" "$RUN_DIR" "$LOG_DIR"

# Install binary
echo "Installing binary to $BINARY_PATH..."
cp "$(which rucid)" "$BINARY_PATH" 2>/dev/null || cp "target/release/rucid" "$BINARY_PATH" 2>/dev/null || {
    echo "Error: rucid binary not found. Run 'cargo build --release' first."
    exit 1
}
chmod +x "$BINARY_PATH"

# Install systemd service (if systemd is available)
if command -v systemctl &> /dev/null; then
    echo "Installing systemd service..."
    cp contrib/rucid.service /etc/systemd/system/
    systemctl daemon-reload
    echo "Run 'systemctl enable --now rucid' to start the daemon"
else
    echo "systemd not found, skipping service installation"
fi

# Install config (if it doesn't exist)
if [ ! -f "$CONFIG_PATH" ]; then
    echo "Installing example config to $CONFIG_PATH..."
    mkdir -p "$(dirname $CONFIG_PATH)"
    cp contrib/ruci.yaml.example "$CONFIG_PATH"
    chown ruci:ruci "$CONFIG_PATH"
else
    echo "Config already exists at $CONFIG_PATH, skipping"
fi

echo ""
echo "Installation complete!"
echo ""
echo "To start rucid:"
if command -v systemctl &> /dev/null; then
    echo "  sudo systemctl enable --now rucid"
else
    echo "  sudo $BINARY_PATH --config $CONFIG_PATH"
fi
echo ""
echo "To check status:"
if command -v systemctl &> /dev/null; then
    echo "  sudo systemctl status rucid"
    echo "  journalctl -u rucid -f"
else
    echo "  ps aux | grep rucid"
fi
