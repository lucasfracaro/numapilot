#!/usr/bin/env bash
# Removes numapilot (keeps /etc/numapilot.toml, /etc/gfx906-rocm.conf and /opt/rocm-gfx906).
set -euo pipefail
sudo systemctl disable --now numapilot.service 2>/dev/null || true
systemctl --user disable --now numapilot-web.service 2>/dev/null || true
sudo rm -f /etc/systemd/system/numapilot.service /etc/systemd/user/numapilot-web.service /usr/local/bin/numapilot /usr/local/bin/numapilot-gui \
    /usr/local/share/applications/numapilot.desktop /usr/local/bin/gfx906-rocm
sudo rm -rf /usr/local/share/numapilot
sudo systemctl daemon-reload
echo "Removed. Configs and ROCm versions were kept (delete them by hand if you want)."
