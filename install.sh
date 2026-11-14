#!/usr/bin/env bash
# Installs (or reinstalls after formatting) numapilot and gfx906-rocm on CachyOS / Arch.
#
# Checks the system, installs what is missing, builds, enables the service, restores a backup
# (if there is one) and finishes with the diagnosis.
#
#   ./install.sh                     asks before installing the optional packages
#   ./install.sh --yes               installs everything without asking
#   ./install.sh --check             only shows what is missing, changes nothing
#   ./install.sh --minimal           only what is required (no optional packages)
#   ./install.sh --restore FILE      restores the settings from a `numapilot backup`
set -euo pipefail
cd "$(dirname "$(readlink -f "$0")")"

YES=0 CHECK=0 MINIMAL=0 RESTORE=""
while [[ $# -gt 0 ]]; do
    case $1 in
    -y | --yes) YES=1 ;;
    --check) CHECK=1 ;;
    --minimal) MINIMAL=1 ;;
    --restore)
        RESTORE=${2:-}
        shift
        ;;
    -h | --help)
        sed -n 2,11p "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        echo "unknown option: $1 (see --help)" >&2
        exit 1
        ;;
    esac
    shift
done

if [[ -t 1 ]]; then B=$'\e[1m' G=$'\e[32m' Y=$'\e[33m' R=$'\e[31m' N=$'\e[0m'; else B="" G="" Y="" R="" N=""; fi
step() { echo; echo "${B}==> $*${N}"; }
ok() { echo "  ${G}ok${N}     $*"; }
warn() { echo "  ${Y}warn${N}   $*"; }
bad() { echo "  ${R}missing${N} $*"; }
die() { echo "${R}error:${N} $*" >&2; exit 1; }
ask() {
    [[ $YES == 1 ]] && return 0
    local r
    read -rp "  $1 [Y/n] " r </dev/tty || return 1
    [[ -z $r || $r =~ ^[sSyY] ]]
}

[[ $EUID -ne 0 ]] || die "run it as your own user (the script asks for the password when needed)"
[[ -z $RESTORE || -f $RESTORE ]] || die "backup not found: $RESTORE"

PREBUILT=0
[[ -e .prebuilt && -x target/release/numapilot ]] && PREBUILT=1

# ------------------------------------------------------------------ 1. packages
step "Checking the packages"
HAS_PACMAN=0
command -v pacman >/dev/null && HAS_PACMAN=1
installed() { pacman -Qq "$1" >/dev/null 2>&1; }
available() { pacman -Si "$1" >/dev/null 2>&1; }

# package | what it is for
REQUIRED=(
    "polkit|asking for the password in the interface's actions"
    "libxkbcommon|graphical interface"
    "wayland|graphical interface"
    "mesa|graphical interface and GPU selection"
    "curl|updates"
    "python|lab scripts"
    "tar|backup"
)
if [[ $PREBUILT == 0 ]]; then
    REQUIRED+=("rust|building numapilot" "gcc|building numapilot" "pkgconf|building numapilot")
fi
RECOMMENDED=(
    "podman|lab (Jupyter, VS Code) and AI server on the MI50s"
    "vulkan-tools|diagnosis of GPU selection in Vulkan"
    "mesa-utils|OpenGL diagnosis"
    "numactl|NUMA information in the diagnosis"
    "mangohud|FPS optimizer"
    "gamescope|gamescope per game"
    "7zip|downloading OptiScaler (upscaling per game)"
    "lm_sensors|motherboard fans and sensors"
    "libnotify|hot GPU and update alerts"
    "libva-utils|Streaming tab (GPUs that encode video)"
    "scx-scheds|CachyOS sched_ext schedulers"
    "scx-tools|switching the scheduler from the interface (scxctl)"
    "smartmontools|disk health (SMART)"
    "ethtool|wake over the network (Wake-on-LAN)"
    "vkmark|load test of the automatic undervolt"
)
HAS_NVIDIA=0
for d in /sys/bus/pci/devices/*; do
    [[ $(cat "$d/vendor" 2>/dev/null) == 0x10de && $(cat "$d/class" 2>/dev/null) == 0x03* ]] && HAS_NVIDIA=1
done
if [[ $HAS_NVIDIA == 1 ]]; then
    RECOMMENDED+=("nvidia-container-toolkit|AI and lab in a container on NVIDIA")
fi
if grep -q AuthenticAMD /proc/cpuinfo && compgen -G '/sys/class/power_supply/BAT*' >/dev/null; then
    RECOMMENDED+=("ryzenadj|laptop Ryzen power limits (AUR)")
fi
if [[ -e /dev/ipmi0 ]] || lsmod 2>/dev/null | grep -q '^ipmi_si'; then
    RECOMMENDED+=("ipmitool|fans through the server board's IPMI")
fi

missing_req=() missing_rec=() unavailable=() missing_aur=()
if [[ $HAS_PACMAN == 1 ]]; then
    for e in "${REQUIRED[@]}"; do
        p=${e%%|*} why=${e#*|}
        if installed "$p"; then ok "$p"; else bad "$p ($why)"; missing_req+=("$p"); fi
    done
    for e in "${RECOMMENDED[@]}"; do
        p=${e%%|*} why=${e#*|}
        if installed "$p"; then
            ok "$p"
        elif available "$p"; then
            warn "$p not installed: $why"
            missing_rec+=("$p")
        else
            unavailable+=("$p")
        fi
    done
    # what is not in the official repositories may be in the AUR (paru/yay)
    AUR_HELPER=$(command -v paru || command -v yay || true)
    missing_aur=()
    if [[ -n $AUR_HELPER ]]; then
        for p in "${unavailable[@]}"; do
            if "$AUR_HELPER" -Si "$p" >/dev/null 2>&1; then
                warn "$p not installed (AUR)"
                missing_aur+=("$p")
            fi
        done
        if ((${#missing_aur[@]} > 0)); then
            un=()
            for p in "${unavailable[@]}"; do [[ " ${missing_aur[*]} " == *" $p "* ]] || un+=("$p"); done
            unavailable=("${un[@]}")
        fi
    fi
    ((${#unavailable[@]} == 0)) || echo "  (no package on this distro: ${unavailable[*]})"
else
    warn "not Arch/CachyOS: check by hand (Rust, gcc, pkg-config, libxkbcommon, wayland, mesa, polkit, podman)"
    command -v cargo >/dev/null || [[ $PREBUILT == 1 ]] || die "install Rust (cargo) and run again"
fi

# ------------------------------------------------------------------ 2. system
step "Checking the system"
NEED_RELOGIN=0
if grep -qw cpuset /sys/fs/cgroup/cgroup.controllers 2>/dev/null; then ok "cgroups v2"; else warn "cgroups v2 without cpuset: memory limits and exclusive cores will not work"; fi
nodes=$(compgen -G '/sys/devices/system/node/node[0-9]*' | wc -l || true)
sockets=$(sort -u /sys/devices/system/cpu/cpu*/topology/physical_package_id 2>/dev/null | wc -l || echo 1)
if ((nodes >= 2)); then ok "$nodes NUMA nodes"; elif ((sockets >= 2)); then warn "$sockets sockets but $nodes NUMA node: turn off 'Node Interleaving' in the BIOS"; else ok "1 NUMA node"; fi
if lsmod 2>/dev/null | grep -q '^amdgpu'; then ok "amdgpu driver"; else warn "amdgpu driver not loaded"; fi
if [[ -e /dev/kfd ]]; then ok "/dev/kfd (ROCm)"; else warn "/dev/kfd missing: ROCm will not see the MI50s"; fi
missing_groups=()
for g in render video; do
    # id -nG without a user only sees this session's groups; with "$USER" it reads the current /etc/group
    if id -nG | tr ' ' '\n' | grep -qx "$g"; then
        ok "you are in the $g group"
    elif id -nG "$USER" | tr ' ' '\n' | grep -qx "$g"; then
        warn "you are already in the $g group, but it only applies after logging out and in again"
    else
        bad "group $g (GPU access through ROCm)"
        missing_groups+=("$g")
    fi
done
UNCORE=0
if grep -q GenuineIntel /proc/cpuinfo && modinfo intel_uncore_frequency >/dev/null 2>&1; then
    if [[ -e /etc/modules-load.d/numapilot.conf ]] && grep -q intel_uncore_frequency /etc/modules-load.d/numapilot.conf; then
        ok "uncore module loads at boot"
    else
        warn "intel_uncore_frequency module does not load at boot (uncore control)"
        UNCORE=1
    fi
fi
if pgrep -x irqbalance >/dev/null; then warn "irqbalance running: it may undo the GPU interrupt tuning (sudo systemctl disable --now irqbalance)"; fi

if [[ $CHECK == 1 ]]; then
    echo
    echo "Missing: required ${#missing_req[@]}, optional ${#missing_rec[@]}, groups ${#missing_groups[@]}."
    echo "Run ./install.sh to install."
    exit 0
fi

# ------------------------------------------------------------------ 3. install what is missing
to_install=("${missing_req[@]}")
if ((${#missing_rec[@]} > 0)) && [[ $MINIMAL == 0 ]]; then
    echo
    echo "  Missing optional packages: ${missing_rec[*]}"
    if ask "Install the optional ones too?"; then to_install+=("${missing_rec[@]}"); fi
fi
# Another package manager open (Discover, automatic update...) locks pacman.
wait_pacman() {
    local i
    for i in $(seq 1 60); do
        [[ -e /var/lib/pacman/db.lck ]] || return 0
        if ! pgrep -x pacman >/dev/null && ! pgrep -f 'packagekitd|pamac|octopi|paru|yay' >/dev/null; then
            warn "pacman is locked (/var/lib/pacman/db.lck) and no package manager is running"
            if ask "Delete the stale lock and continue?"; then sudo rm -f /var/lib/pacman/db.lck; return 0; fi
            return 1
        fi
        ((i == 1)) && echo "  waiting for another package manager to finish (up to 2 min)..."
        sleep 2
    done
    return 1
}
if ((${#to_install[@]} > 0)) || ((${#missing_aur[@]} > 0)); then
    wait_pacman || warn "pacman busy: skipping package installation (run install.sh again later)"
fi
if ((${#missing_req[@]} > 0)) && [[ -e /var/lib/pacman/db.lck ]]; then
    die "could not install the required packages (${missing_req[*]}) with pacman locked"
fi
if ((${#to_install[@]} > 0)) && [[ ! -e /var/lib/pacman/db.lck ]]; then
    step "Installing: ${to_install[*]}"
    # required ones must go in; optional ones with problems do not stop the installation
    if ((${#missing_req[@]} > 0)); then sudo pacman -S --needed --noconfirm "${missing_req[@]}"; fi
    for p in "${to_install[@]}"; do
        [[ " ${missing_req[*]} " == *" $p "* ]] && continue
        sudo pacman -S --needed --noconfirm "$p" || warn "could not install $p (going on without it)"
    done
fi
if ((${#missing_aur[@]} > 0)) && [[ $MINIMAL == 0 ]] && [[ ! -e /var/lib/pacman/db.lck ]] &&
    ask "Install from the AUR too (${missing_aur[*]})?"; then
    for p in "${missing_aur[@]}"; do
        "$AUR_HELPER" -S --needed --noconfirm "$p" || warn "could not install $p from the AUR (going on without it)"
    done
fi
if ((${#missing_groups[@]} > 0)) && ask "Add you to the groups ${missing_groups[*]}?"; then
    sudo usermod -aG "$(IFS=,; echo "${missing_groups[*]}")" "$USER"
    NEED_RELOGIN=1
fi
if [[ $UNCORE == 1 ]] && ask "Load the uncore module at every boot?"; then
    echo intel_uncore_frequency | sudo tee /etc/modules-load.d/numapilot.conf >/dev/null
    sudo modprobe intel_uncore_frequency || true
fi

if [[ $HAS_NVIDIA == 1 ]]; then
    if ! lsmod 2>/dev/null | grep -q '^nvidia '; then
        warn "official NVIDIA driver not loaded: afterwards run 'sudo numapilot driver install' (or the button in Graphics cards)"
    fi
    if command -v nvidia-ctk >/dev/null && [[ ! -e /etc/cdi/nvidia.yaml ]]; then
        sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml >/dev/null 2>&1 && ok "NVIDIA enabled for containers (CDI)"
    fi
fi

# ------------------------------------------------------------------ 4. build and install
if [[ $PREBUILT == 1 ]]; then
    step "Using the prebuilt binaries from the package"
else
    step "Building (a few minutes the first time)"
    cargo build --release
fi

step "Installing the files"
sudo install -Dm755 target/release/numapilot /usr/local/bin/numapilot
sudo install -Dm755 target/release/numapilot-gui /usr/local/bin/numapilot-gui
sudo install -Dm644 packaging/numapilot-gui.desktop /usr/local/share/applications/numapilot.desktop
sudo install -Dm755 packaging/gfx906-rocm /usr/local/bin/gfx906-rocm
# where it was installed from (the interface's "Update numapilot" uses it to fetch the new version)
if [[ -d .git ]]; then
    sudo install -d /var/lib/numapilot
    pwd | sudo tee /var/lib/numapilot/source >/dev/null
fi
sudo install -d /usr/local/share/numapilot/container
sudo install -m644 container/Containerfile container/*.txt container/*.py /usr/local/share/numapilot/container/
sudo install -m755 container/lab-start /usr/local/share/numapilot/container/
sudo install -Dm644 packaging/numapilot.service /etc/systemd/system/numapilot.service
sudo install -Dm644 packaging/numapilot-web.service /etc/systemd/user/numapilot-web.service
# Configs: yours are not overwritten.
[[ -e /etc/numapilot.toml ]] || sudo install -Dm644 packaging/numapilot.toml /etc/numapilot.toml
[[ -e /etc/gfx906-rocm.conf ]] || sudo install -Dm644 packaging/gfx906-rocm.conf /etc/gfx906-rocm.conf
sudo install -d -m755 /opt/rocm-gfx906
ok "programs in /usr/local/bin, shortcut in the menu"

# ------------------------------------------------------------------ 5. restore
if [[ -z $RESTORE ]]; then
    last=$(compgen -G "$HOME/numapilot-backup-*.tar.gz" | sort | tail -n1 || true)
    if [[ -n $last ]] && [[ $YES == 0 ]] && ask "Found the backup $(basename "$last"). Restore its settings?"; then
        RESTORE=$last
    fi
fi
if [[ -n $RESTORE ]]; then
    step "Restoring $RESTORE"
    sudo SUDO_USER="$USER" /usr/local/bin/numapilot restore "$RESTORE"
fi

# ------------------------------------------------------------------ 6. service
step "Enabling the service"
sudo systemctl daemon-reload
sudo systemctl enable --now numapilot.service
sudo systemctl restart numapilot.service
ok "numapilot.service active"

# ------------------------------------------------------------------ 7. laptop with NVIDIA
if /usr/local/bin/numapilot hybrid 2>/dev/null | grep -q '^nvidia'; then
    step "Laptop with NVIDIA"
    echo "  For the NVIDIA to sleep when not in use, what starts with the system (ZapZap, Sunshine,"
    echo "  notifiers...) and Xwayland can use only the integrated card. Games stay on the NVIDIA."
    if ask "Start everything that opens with the system on the integrated card?"; then
        /usr/local/bin/numapilot autostart igpu --if-hybrid || warn "could not adjust the startup programs"
        sudo /usr/local/bin/numapilot hybrid xwayland on || warn "could not adjust Xwayland"
        NEED_RELOGIN=1
    fi
    if systemctl is-enabled --quiet lactd 2>/dev/null; then
        warn "the LACT service (lactd) keeps the NVIDIA awake all the time"
        if ask "Turn off lactd (numapilot controls the cards)?"; then
            sudo systemctl disable --now lactd || true
        fi
    fi
fi

# ------------------------------------------------------------------ 8. diagnosis
step "Diagnosis"
# The service has just restarted: a doctor run in its first seconds can fail the service
# checks. Try again a few times before believing a failure, and never let `tail` hide one.
report=""
for _ in 1 2 3 4; do
    sleep 2
    report=$(numapilot doctor 2>&1 || true)
    grep -qE '^\[(FAIL|FALHA|FALHOU)\]' <<<"$report" || break
done
fails=$(grep -A1 -E '^\[(FAIL|FALHA|FALHOU)\]' <<<"$report" || true)
if [[ -n $fails ]]; then
    echo "$fails"
    echo "  ..."
fi
tail -n 25 <<<"$report"

echo
echo "${B}Done.${N} Open \"numapilot\" from the menu."
[[ $NEED_RELOGIN == 1 ]] && echo "${Y}Log out and in again${N} for the new groups and each program's card to apply."
echo "Before formatting again: numapilot backup (and keep the file off the system disk)."
echo "Hardware test guide: docs/TESTES.md"
