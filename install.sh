#!/bin/bash
set -e

REPO="theasmat/deckdriod"
BIN_NAME="deckdriod"
INSTALL_DIR="/usr/local/bin"

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

sudo mv "$TMP_DIR/$BIN_NAME" "$INSTALL_DIR/"
chmod +x "$INSTALL_DIR/$BIN_NAME"

echo "Successfully installed $BIN_NAME to $INSTALL_DIR"
$BIN_NAME --version || true
