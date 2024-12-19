#!/bin/bash
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'
echo -e "${GREEN}Installing Red Editor...${NC}"
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}Error: Cargo is not installed. Please install Rust and Cargo first.${NC}"
    echo "Visit https://rustup.rs/ for installation instructions."
    exit 1
fi
echo "Removing old installations..."
cargo uninstall red 2>/dev/null
sudo rm -f /usr/local/bin/red
echo "Building and installing Red Editor..."
cargo build --release && \
sudo cp target/release/red /usr/local/bin/ && \
sudo chmod +x /usr/local/bin/red
if [ $? -eq 0 ]; then
    echo -e "${GREEN}Red Editor successfully installed!${NC}"
    echo "You can now use 'red' command to launch the editor."
else
    echo -e "${RED}Installation failed.${NC}"
    exit 1
fi
