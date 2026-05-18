#!/bin/bash
set -e

REPO="theasmat/deckdriod"
BIN_NAME="deckdriod"

# Detect active installation path
EXISTING_PATH=$(which $BIN_NAME || true)
if [[ -n "$EXISTING_PATH" ]]; then
    INSTALL_DIR=$(dirname "$EXISTING_PATH")
    echo "Detected existing installation at $EXISTING_PATH. Updating in $INSTALL_DIR..."
else
    INSTALL_DIR="/usr/local/bin"
    echo "No existing installation found. Defaulting to $INSTALL_DIR..."
fi

# Detect OS and Architecture
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)

if [[ "$OS" == "darwin" ]]; then
    if [[ "$ARCH" == "arm64" ]]; then
        TARGET="aarch64-apple-darwin"
    else
        TARGET="x86_64-apple-darwin"
    fi
elif [[ "$OS" == "linux" ]]; then
    TARGET="x86_64-unknown-linux-musl"
else
    echo "Unsupported OS: $OS"
    exit 1
fi

# Get the latest release tag
TAG=$(curl -s "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [[ -z "$TAG" ]]; then
    echo "Error: Could not find latest release for $REPO"
    exit 1
fi

echo "Installing $BIN_NAME $TAG for $TARGET..."

URL="https://github.com/$REPO/releases/download/$TAG/deckdriod-$TARGET.tar.gz"

# Download and install
TMP_DIR=$(mktemp -d)
curl -L -sSf "$URL" | tar xz -C "$TMP_DIR"

if [[ ! -f "$TMP_DIR/$BIN_NAME" ]]; then
    echo "Error: Downloaded package does not contain $BIN_NAME"
    exit 1
fi

# Try to move without sudo first, fallback to sudo if needed
if mv "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/" 2>/dev/null; then
    chmod +x "$INSTALL_DIR/$BIN_NAME"
else
    sudo mv "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/"
    sudo chmod +x "$INSTALL_DIR/$BIN_NAME"
fi

echo "Successfully installed $BIN_NAME to $INSTALL_DIR"
$BIN_NAME -v || true
