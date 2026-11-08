#!/usr/bin/env bash
# Instala (ou reinstala depois de formatar) o numapilot e o gfx906-rocm no CachyOS / Arch.
#
# Confere o sistema, instala o que falta, compila, ativa o serviço, restaura um backup
# (se houver) e termina com o diagnóstico.
#
#   ./install.sh                     pergunta antes de instalar os opcionais
#   ./install.sh --yes               instala tudo sem perguntar
#   ./install.sh --check             só mostra o que falta, não muda nada
#   ./install.sh --minimal           só o necessário (sem os opcionais)
#   ./install.sh --restore ARQUIVO   restaura as configurações de um `numapilot backup`
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
        sed -n 2,13p "$0" | sed 's/^# \{0,1\}//'
        exit 0
        ;;
    *)
        echo "opção desconhecida: $1 (veja --help)" >&2
        exit 1
        ;;
    esac
    shift
done

if [[ -t 1 ]]; then B=$'\e[1m' G=$'\e[32m' Y=$'\e[33m' R=$'\e[31m' N=$'\e[0m'; else B="" G="" Y="" R="" N=""; fi
step() { echo; echo "${B}==> $*${N}"; }
ok() { echo "  ${G}ok${N}     $*"; }
warn() { echo "  ${Y}aviso${N}  $*"; }
bad() { echo "  ${R}falta${N}  $*"; }
die() { echo "${R}erro:${N} $*" >&2; exit 1; }
ask() {
    [[ $YES == 1 ]] && return 0
    local r
    read -rp "  $1 [S/n] " r </dev/tty || return 1
    [[ -z $r || $r =~ ^[sSyY] ]]
}

[[ $EUID -ne 0 ]] || die "rode como o seu usuário (o script pede a senha quando precisa)"
[[ -z $RESTORE || -f $RESTORE ]] || die "backup não encontrado: $RESTORE"

PREBUILT=0
[[ -e .prebuilt && -x target/release/numapilot ]] && PREBUILT=1

# ------------------------------------------------------------------ 1. pacotes
step "Conferindo os pacotes"
HAS_PACMAN=0
command -v pacman >/dev/null && HAS_PACMAN=1
installed() { pacman -Qq "$1" >/dev/null 2>&1; }
available() { pacman -Si "$1" >/dev/null 2>&1; }

# pacote | para que serve
REQUIRED=(
    "polkit|pedir a senha nas ações da interface"
    "libxkbcommon|interface gráfica"
    "wayland|interface gráfica"
    "mesa|interface gráfica e seleção de GPU"
    "curl|atualizações"
    "python|scripts do laboratório"
    "tar|backup"
)
if [[ $PREBUILT == 0 ]]; then
    REQUIRED+=("rust|compilar o numapilot" "gcc|compilar o numapilot" "pkgconf|compilar o numapilot")
fi
RECOMMENDED=(
    "podman|laboratório (Jupyter, VS Code) e servidor de IA nas MI50"
    "vulkan-tools|diagnóstico da seleção de GPU no Vulkan"
    "mesa-utils|diagnóstico do OpenGL"
    "numactl|informações de NUMA no diagnóstico"
    "mangohud|otimizador de FPS"
    "gamescope|gamescope por jogo"
    "7zip|baixar o OptiScaler (upscaling por jogo)"
    "lm_sensors|ventoinhas e sensores da placa-mãe"
    "libnotify|alertas de GPU quente e de atualização"
    "libva-utils|aba Streaming (GPUs que codificam vídeo)"
    "scx-scheds|escalonadores sched_ext do CachyOS"
    "scx-tools|trocar o escalonador pela interface (scxctl)"
    "smartmontools|saúde dos discos (SMART)"
    "ethtool|ligar pela rede (Wake-on-LAN)"
    "vkmark|teste de carga do undervolt automático"
)
HAS_NVIDIA=0
for d in /sys/bus/pci/devices/*; do
    [[ $(cat "$d/vendor" 2>/dev/null) == 0x10de && $(cat "$d/class" 2>/dev/null) == 0x03* ]] && HAS_NVIDIA=1
done
if [[ $HAS_NVIDIA == 1 ]]; then
    RECOMMENDED+=("nvidia-container-toolkit|IA e laboratório em container na NVIDIA")
fi
if grep -q AuthenticAMD /proc/cpuinfo && compgen -G '/sys/class/power_supply/BAT*' >/dev/null; then
    RECOMMENDED+=("ryzenadj|limites de potência do Ryzen de notebook (AUR)")
fi
if [[ -e /dev/ipmi0 ]] || lsmod 2>/dev/null | grep -q '^ipmi_si'; then
    RECOMMENDED+=("ipmitool|ventoinhas pelo IPMI da placa de servidor")
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
            warn "$p não instalado: $why"
            missing_rec+=("$p")
        else
            unavailable+=("$p")
        fi
    done
    # o que não está nos repositórios oficiais pode estar no AUR (paru/yay)
    AUR_HELPER=$(command -v paru || command -v yay || true)
    missing_aur=()
    if [[ -n $AUR_HELPER ]]; then
        for p in "${unavailable[@]}"; do
            if "$AUR_HELPER" -Si "$p" >/dev/null 2>&1; then
                warn "$p não instalado (AUR)"
                missing_aur+=("$p")
            fi
        done
        if ((${#missing_aur[@]} > 0)); then
            un=()
            for p in "${unavailable[@]}"; do [[ " ${missing_aur[*]} " == *" $p "* ]] || un+=("$p"); done
            unavailable=("${un[@]}")
        fi
    fi
    ((${#unavailable[@]} == 0)) || echo "  (sem pacote nesta distro: ${unavailable[*]})"
else
    warn "não é Arch/CachyOS: confira à mão (Rust, gcc, pkg-config, libxkbcommon, wayland, mesa, polkit, podman)"
    command -v cargo >/dev/null || [[ $PREBUILT == 1 ]] || die "instale o Rust (cargo) e rode de novo"
fi

# ------------------------------------------------------------------ 2. sistema
step "Conferindo o sistema"
NEED_RELOGIN=0
if grep -qw cpuset /sys/fs/cgroup/cgroup.controllers 2>/dev/null; then ok "cgroups v2"; else warn "cgroups v2 sem cpuset: limites de memória e núcleos exclusivos não vão funcionar"; fi
nodes=$(compgen -G '/sys/devices/system/node/node[0-9]*' | wc -l || true)
sockets=$(sort -u /sys/devices/system/cpu/cpu*/topology/physical_package_id 2>/dev/null | wc -l || echo 1)
if ((nodes >= 2)); then ok "$nodes nós NUMA"; elif ((sockets >= 2)); then warn "$sockets sockets mas $nodes nó NUMA: desligue 'Node Interleaving' na BIOS"; else ok "1 nó NUMA"; fi
if lsmod 2>/dev/null | grep -q '^amdgpu'; then ok "driver amdgpu"; else warn "driver amdgpu não carregado"; fi
if [[ -e /dev/kfd ]]; then ok "/dev/kfd (ROCm)"; else warn "/dev/kfd ausente: o ROCm não vai enxergar as MI50"; fi
missing_groups=()
for g in render video; do
    # id -nG sem usuário só vê os grupos desta sessão; com "$USER" lê o /etc/group atual
    if id -nG | tr ' ' '\n' | grep -qx "$g"; then
        ok "você está no grupo $g"
    elif id -nG "$USER" | tr ' ' '\n' | grep -qx "$g"; then
        warn "você já está no grupo $g, mas só vale depois de sair e entrar de novo na sessão"
    else
        bad "grupo $g (acesso às GPUs pelo ROCm)"
        missing_groups+=("$g")
    fi
done
UNCORE=0
if grep -q GenuineIntel /proc/cpuinfo && modinfo intel_uncore_frequency >/dev/null 2>&1; then
    if [[ -e /etc/modules-load.d/numapilot.conf ]] && grep -q intel_uncore_frequency /etc/modules-load.d/numapilot.conf; then
        ok "módulo do uncore carrega no boot"
    else
        warn "módulo intel_uncore_frequency não carrega no boot (controle do uncore)"
        UNCORE=1
    fi
fi
if pgrep -x irqbalance >/dev/null; then warn "irqbalance rodando: pode desfazer as interrupções das GPUs (sudo systemctl disable --now irqbalance)"; fi

if [[ $CHECK == 1 ]]; then
    echo
    echo "Faltam: obrigatórios ${#missing_req[@]}, opcionais ${#missing_rec[@]}, grupos ${#missing_groups[@]}."
    echo "Rode ./install.sh para instalar."
    exit 0
fi

# ------------------------------------------------------------------ 3. instalar o que falta
to_install=("${missing_req[@]}")
if ((${#missing_rec[@]} > 0)) && [[ $MINIMAL == 0 ]]; then
    echo
    echo "  Opcionais que faltam: ${missing_rec[*]}"
    if ask "Instalar os opcionais também?"; then to_install+=("${missing_rec[@]}"); fi
fi
# Outro gerenciador de pacotes aberto (Discover, atualização automática...) trava o pacman.
wait_pacman() {
    local i
    for i in $(seq 1 60); do
        [[ -e /var/lib/pacman/db.lck ]] || return 0
        if ! pgrep -x pacman >/dev/null && ! pgrep -f 'packagekitd|pamac|octopi|paru|yay' >/dev/null; then
            warn "o pacman está travado (/var/lib/pacman/db.lck) e nenhum gerenciador está rodando"
            if ask "Apagar a trava antiga e continuar?"; then sudo rm -f /var/lib/pacman/db.lck; return 0; fi
            return 1
        fi
        ((i == 1)) && echo "  esperando outro gerenciador de pacotes terminar (até 2 min)..."
        sleep 2
    done
    return 1
}
if ((${#to_install[@]} > 0)) || ((${#missing_aur[@]} > 0)); then
    wait_pacman || warn "pacman ocupado: pulando a instalação de pacotes (rode o install.sh de novo depois)"
fi
if ((${#missing_req[@]} > 0)) && [[ -e /var/lib/pacman/db.lck ]]; then
    die "não deu para instalar os obrigatórios (${missing_req[*]}) com o pacman travado"
fi
if ((${#to_install[@]} > 0)) && [[ ! -e /var/lib/pacman/db.lck ]]; then
    step "Instalando: ${to_install[*]}"
    # obrigatórios têm que entrar; opcionais com problema não param a instalação
    if ((${#missing_req[@]} > 0)); then sudo pacman -S --needed --noconfirm "${missing_req[@]}"; fi
    for p in "${to_install[@]}"; do
        [[ " ${missing_req[*]} " == *" $p "* ]] && continue
        sudo pacman -S --needed --noconfirm "$p" || warn "não consegui instalar $p (seguindo sem ele)"
    done
fi
if ((${#missing_aur[@]} > 0)) && [[ $MINIMAL == 0 ]] && [[ ! -e /var/lib/pacman/db.lck ]] &&
    ask "Instalar do AUR também (${missing_aur[*]})?"; then
    for p in "${missing_aur[@]}"; do
        "$AUR_HELPER" -S --needed --noconfirm "$p" || warn "não consegui instalar $p do AUR (seguindo sem ele)"
    done
fi
if ((${#missing_groups[@]} > 0)) && ask "Adicionar você aos grupos ${missing_groups[*]}?"; then
    sudo usermod -aG "$(IFS=,; echo "${missing_groups[*]}")" "$USER"
    NEED_RELOGIN=1
fi
if [[ $UNCORE == 1 ]] && ask "Carregar o módulo do uncore em todo boot?"; then
    echo intel_uncore_frequency | sudo tee /etc/modules-load.d/numapilot.conf >/dev/null
    sudo modprobe intel_uncore_frequency || true
fi

if [[ $HAS_NVIDIA == 1 ]]; then
    if ! lsmod 2>/dev/null | grep -q '^nvidia '; then
        warn "driver oficial da NVIDIA não carregado: depois rode 'sudo numapilot driver install' (ou o botão em Placas de vídeo)"
    fi
    if command -v nvidia-ctk >/dev/null && [[ ! -e /etc/cdi/nvidia.yaml ]]; then
        sudo nvidia-ctk cdi generate --output=/etc/cdi/nvidia.yaml >/dev/null 2>&1 && ok "NVIDIA liberada para os containers (CDI)"
    fi
fi

# ------------------------------------------------------------------ 4. compilar e instalar
if [[ $PREBUILT == 1 ]]; then
    step "Usando os binários prontos do pacote"
else
    step "Compilando (alguns minutos na primeira vez)"
    cargo build --release
fi

step "Instalando os arquivos"
sudo install -Dm755 target/release/numapilot /usr/local/bin/numapilot
sudo install -Dm755 target/release/numapilot-gui /usr/local/bin/numapilot-gui
sudo install -Dm644 packaging/numapilot-gui.desktop /usr/local/share/applications/numapilot.desktop
sudo install -Dm755 packaging/gfx906-rocm /usr/local/bin/gfx906-rocm
# de onde instalou (o "Atualizar o numapilot" da interface usa para buscar a versão nova)
if [[ -d .git ]]; then
    sudo install -d /var/lib/numapilot
    pwd | sudo tee /var/lib/numapilot/source >/dev/null
fi
sudo install -d /usr/local/share/numapilot/container
sudo install -m644 container/Containerfile container/*.txt container/*.py /usr/local/share/numapilot/container/
sudo install -m755 container/lab-start /usr/local/share/numapilot/container/
sudo install -Dm644 packaging/numapilot.service /etc/systemd/system/numapilot.service
sudo install -Dm644 packaging/numapilot-web.service /etc/systemd/user/numapilot-web.service
# Configs: não sobrescreve as suas.
[[ -e /etc/numapilot.toml ]] || sudo install -Dm644 packaging/numapilot.toml /etc/numapilot.toml
[[ -e /etc/gfx906-rocm.conf ]] || sudo install -Dm644 packaging/gfx906-rocm.conf /etc/gfx906-rocm.conf
sudo install -d -m755 /opt/rocm-gfx906
ok "programas em /usr/local/bin, atalho no menu"

# ------------------------------------------------------------------ 5. restaurar
if [[ -z $RESTORE ]]; then
    last=$(compgen -G "$HOME/numapilot-backup-*.tar.gz" | sort | tail -n1 || true)
    if [[ -n $last ]] && [[ $YES == 0 ]] && ask "Achei o backup $(basename "$last"). Restaurar as configurações dele?"; then
        RESTORE=$last
    fi
fi
if [[ -n $RESTORE ]]; then
    step "Restaurando $RESTORE"
    sudo SUDO_USER="$USER" /usr/local/bin/numapilot restore "$RESTORE"
fi

# ------------------------------------------------------------------ 6. serviço
step "Ativando o serviço"
sudo systemctl daemon-reload
sudo systemctl enable --now numapilot.service
sudo systemctl restart numapilot.service
ok "numapilot.service ativo"

# ------------------------------------------------------------------ 7. notebook com NVIDIA
if /usr/local/bin/numapilot hybrid 2>/dev/null | grep -q '^nvidia'; then
    step "Notebook com NVIDIA"
    echo "  Para a NVIDIA dormir quando não está em uso, o que abre com o sistema (ZapZap, Sunshine,"
    echo "  notificadores...) e o Xwayland podem usar só a placa integrada. Jogos continuam na NVIDIA."
    if ask "Abrir tudo que inicia com o sistema na placa integrada?"; then
        /usr/local/bin/numapilot autostart igpu --if-hybrid || warn "não consegui ajustar a inicialização"
        sudo /usr/local/bin/numapilot hybrid xwayland on || warn "não consegui ajustar o Xwayland"
        NEED_RELOGIN=1
    fi
    if systemctl is-enabled --quiet lactd 2>/dev/null; then
        warn "o serviço do LACT (lactd) mantém a NVIDIA acordada o tempo todo"
        if ask "Desligar o lactd (o numapilot faz o controle das placas)?"; then
            sudo systemctl disable --now lactd || true
        fi
    fi
fi

# ------------------------------------------------------------------ 8. diagnóstico
step "Diagnóstico"
sleep 2
numapilot doctor | tail -n 25 || true

echo
echo "${B}Pronto.${N} Abra \"numapilot\" no menu."
[[ $NEED_RELOGIN == 1 ]] && echo "${Y}Saia e entre de novo na sessão${N} para valer os grupos novos e a placa de cada programa."
echo "Antes de formatar de novo: numapilot backup (e guarde o arquivo fora do disco do sistema)."
echo "Roteiro de testes no hardware: docs/TESTES.md"
