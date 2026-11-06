#!/usr/bin/env bash
# Instala o numapilot e o gfx906-rocm no CachyOS / Arch.
set -euo pipefail
cd "$(dirname "$0")"

if ! command -v cargo >/dev/null; then
    echo "==> instalando Rust (pacman -S rust)"
    sudo pacman -S --needed --noconfirm rust
fi

echo "==> compilando"
cargo build --release

echo "==> instalando arquivos"
sudo install -Dm755 target/release/numapilot /usr/local/bin/numapilot
sudo install -Dm755 target/release/numapilot-gui /usr/local/bin/numapilot-gui
sudo install -Dm644 packaging/numapilot-gui.desktop /usr/local/share/applications/numapilot.desktop
sudo install -Dm755 packaging/gfx906-rocm /usr/local/bin/gfx906-rocm
sudo install -Dm644 packaging/numapilot.service /etc/systemd/system/numapilot.service
# Configs: não sobrescreve as suas.
[[ -e /etc/numapilot.toml ]] || sudo install -Dm644 packaging/numapilot.toml /etc/numapilot.toml
[[ -e /etc/gfx906-rocm.conf ]] || sudo install -Dm644 packaging/gfx906-rocm.conf /etc/gfx906-rocm.conf
sudo install -d -m755 /opt/rocm-gfx906

echo "==> ativando o serviço"
sudo systemctl daemon-reload
sudo systemctl enable --now numapilot.service

echo
numapilot topo
echo
echo "Pronto. Veja o que o automático está fazendo com:  numapilot status"
echo "Logs:  journalctl -u numapilot -f"
