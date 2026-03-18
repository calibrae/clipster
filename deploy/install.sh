#!/usr/bin/env bash
set -euo pipefail

BINARY="clipster-server"
INSTALL_DIR="/usr/local/bin"
CONFIG_DIR="/etc/clipster"
DATA_DIR="/var/lib/clipster"
SERVICE_FILE="/etc/systemd/system/clipster-server.service"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Check root
if [ "$(id -u)" -ne 0 ]; then
    echo "Error: must run as root" >&2
    exit 1
fi

# Find binary
BINARY_PATH="${1:-$REPO_ROOT/target/release/$BINARY}"
if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: binary not found at $BINARY_PATH" >&2
    echo "Build first: cargo build --release -p clipster-server" >&2
    echo "Or pass binary path: $0 /path/to/$BINARY" >&2
    exit 1
fi

echo "==> Creating clipster user/group"
if ! id -u clipster &>/dev/null; then
    useradd --system --no-create-home --shell /usr/sbin/nologin clipster
fi

echo "==> Creating directories"
install -d -o clipster -g clipster -m 750 "$DATA_DIR"
install -d -o clipster -g clipster -m 750 "$DATA_DIR/images"
install -d -m 755 "$CONFIG_DIR"

echo "==> Installing binary"
install -m 755 "$BINARY_PATH" "$INSTALL_DIR/$BINARY"

echo "==> Installing config"
if [ ! -f "$CONFIG_DIR/server.toml" ]; then
    install -m 644 "$SCRIPT_DIR/clipster-server.conf" "$CONFIG_DIR/server.toml"
else
    echo "    Config already exists, skipping (see $SCRIPT_DIR/clipster-server.conf for reference)"
fi

echo "==> Installing systemd service"
install -m 644 "$SCRIPT_DIR/clipster-server.service" "$SERVICE_FILE"
systemctl daemon-reload

echo "==> Enabling and starting service"
systemctl enable --now clipster-server

echo ""
echo "==> Done. Status:"
systemctl status clipster-server --no-pager
