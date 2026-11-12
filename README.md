# numapilot

[![CI](https://github.com/lucasfracaro/numapilot/actions/workflows/ci.yml/badge.svg)](https://github.com/lucasfracaro/numapilot/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/lucasfracaro/numapilot?label=release)](https://github.com/lucasfracaro/numapilot/releases/latest)
[![MIT license](https://img.shields.io/badge/license-MIT-blue)](LICENSE)
![Linux](https://img.shields.io/badge/Linux-CachyOS%20%2F%20Arch-1793d1?logo=archlinux&logoColor=white)
![Rust](https://img.shields.io/badge/Rust-2024-dea584?logo=rust&logoColor=white)

**English** · [Português](README.pt-BR.md)

[Install](#install) · [Screenshots](#screenshots) · [Interface](#the-interface-tab-by-tab) ·
[Command line](#command-line) · [Laptop](#laptop) · [BC-250](#bc-250-and-bc-160) ·
[Changelog](CHANGELOG.md)

Hardware and program manager for Linux (built on CachyOS): **a single app to control processor,
memory, graphics cards, programs, services and what starts with the system**, on any PC: from a
server with 2 processors and AI cards to a gaming laptop or a BC-250.

It detects the hardware and shows only what makes sense for it. It started by choosing **which
processor (NUMA socket), which memory and which GPU** each program uses on a server with
2× Xeon and 2× MI50, and grew to the rest of the PC: fans, power, undervolt, power modes, hardware
health, startup and services (like the Windows task manager or Mission Center), local AI, games
and several PCs in one panel.

English is the project's language (code, commands, messages, docs). The interface, and the
messages it shows, are also available in Portuguese (Settings → Language, or `NUMAPILOT_LANG=pt`).
The screenshots below were taken in Portuguese.

![Programs tab](docs/interface.png)

## Screenshots

| | |
|---|---|
| [![Graphics cards](docs/gpus.png)](#graphics-cards) **Graphics cards**: history, fan curve, clock and voltage | [![Map & measurements](docs/mapa.png)](#map--measurements) **Map & measurements**: each thread, temperature, disk and network |
| [![Machine](docs/maquina.png)](#machine) **Machine**: modes, energy in kWh, health, processors | [![Laptop](docs/nvidia.png)](#laptop) **Laptop**: battery, NVIDIA only when needed, displays |
| [![Startup](docs/inicializacao.png)](#startup) **Startup**: what starts with the system | [![Services](docs/servicos.png)](#services) **Services**: systemd with state, memory and log |
| [![Adjust program](docs/editor.png)](#programs) **Adjust…**: processor, memory and card per program | [![BC-250](docs/bc250.png)](#bc-250-and-bc-160) **BC-250**: community programs, CUs, GPU memory |

## What it does

| Area | Highlights |
|---|---|
| **Programs** | task-manager-style table (CPU, memory, disk, GPU and GPU memory per program, sort, search), protected End/Force stop, and per program: processor, cores, memory, priority and graphics card |
| **Startup** | everything that starts at login and at boot (programs, services, KDE session restore): turn on, turn off, remove and add (program, AppImage, script or command) |
| **Services** | every systemd service with state and memory: start, stop, restart and read the log |
| **Graphics cards** | temperatures, fan (curve or fixed), power limit, clock/voltage (AMD OverDrive, NVIDIA curve), **automatic undervolt** with a load test, history, Resizable BAR, NVIDIA driver |
| **Machine** | one-click modes (Game, AI, Quiet), governor/frequencies/uncore/power per socket, low latency in games, memory (NUMA, THP, hugepages), sched_ext, motherboard fans, mitigations and kernel parameters, laptop (battery, hybrid NVIDIA, ryzenadj, Acer), energy in kWh and cost, health (SMART, ECC, battery) |
| **Map & measurements** | usage of each thread over the last minute, temperature, memory per socket, **disks and network**, latency and bandwidth between sockets |
| **AI and lab** | llama.cpp server with an OpenAI-compatible API and a tokens/s optimizer; Jupyter, VS Code and PyTorch in a container; community ROCm for the MI50 with validated updates and automatic rollback |
| **Games** | Steam in one click, the right GPU per game, gamescope, **upscaling** (FSR 4, OptiScaler), FPS optimizer, Lossless Scaling, Windows VM with its own card |
| **Several PCs** | phone panel, all PCs in one panel, Wake-on-LAN, shared profiles, daily backup, phone notifications (ntfy), Sunshine/Moonlight |
| **Diagnosis** | `numapilot doctor`, `numapilot selftest`, Tests tab with a per-machine script, logs |

## Tested PCs

| PC | Processor | Graphics cards | Status |
|---|---|---|---|
| Server | 2× Xeon E5-2696 v4 (2 sockets, 88 threads) | 2× MI50 (gfx906) + RX 470 | where it all started |
| Acer Nitro 5 AN515-46 laptop | Ryzen 7 6800H | Radeon 680M + RTX 3070 Ti Laptop | self-test and manual script |
| BC-250 | AMD APU (Cyan Skillfish), 6 or 8 cores | integrated GPU (24 or 40 CUs) | self-test |
| CC150 PC | Intel CC150 | RX 9060 XT (RDNA 4) | self-test and doctor with no warnings |
| PC | i7-5775C | BC-160 | to be tested |

It works on any Arch/CachyOS; other distros should work when built from source, but the installer
uses pacman.

## Install

**Prebuilt binaries** (recommended): download the `.tar.gz` from the
[latest release](https://github.com/lucasfracaro/numapilot/releases/latest).

```bash
tar xzf numapilot-*-x86_64-linux.tar.gz && cd numapilot-*-x86_64-linux
./install.sh
```

**From source** (to update with `git pull` later):

```bash
sudo pacman -S --needed rust git
mkdir -p ~/Projetos && cd ~/Projetos
git clone https://github.com/lucasfracaro/numapilot && cd numapilot
./install.sh
```

The installer checks everything first (packages, cgroups v2, NUMA, driver, `/dev/kfd`, the `render`
and `video` groups, the uncore module), installs what is missing through pacman (it asks about the
optional ones, explaining what each is for, and offers the AUR ones through paru/yay), builds (or
uses the prebuilt binaries), installs to `/usr/local/bin`, enables the service and finishes with
`numapilot doctor`. Open **numapilot** from the menu. The installer's own messages are still in
Portuguese.

| Command | What it does |
|---|---|
| `./install.sh --check` | only shows what is missing, changes nothing |
| `./install.sh --yes` | installs everything without asking |
| `./install.sh --minimal` | only what is required |
| `./install.sh --restore FILE` | installs and restores a backup |
| `./uninstall.sh` | removes it |

Run the installer in a **regular terminal** (Konsole). Terminals inside other programs may block
`sudo` ("no new privileges").

**Through pacman:** `packaging/arch/PKGBUILD` builds `numapilot-git` (`cd packaging/arch && makepkg -si`
and `sudo systemctl enable --now numapilot`); the stable version is in `packaging/arch/release/`.

**In the BIOS** (server with 2 processors): NUMA enabled / "Node Interleaving" disabled. If
`numapilot topo` shows only 1 node with 2 sockets, it warns you.

**Before formatting:** `numapilot backup` (or Settings → Back up the settings) stores in a `.tar.gz`
the profiles, the machine, fan and GPU settings (including undervolt), OverDrive, the kernel
parameters, the lab, the AI server and the FPS optimizer. Afterwards:
`./install.sh --restore numapilot-backup-DATE.tar.gz`. `.gguf` models and `~/lab` are your data:
copy them separately.

## Updating (and several PCs on the same code)

```bash
numapilot self-update        # installed commit, what is missing from the folder and from GitHub
numapilot self-update run    # git pull + ./install.sh --yes (asks for the password)
numapilot selftest          # confirms nothing broke on this machine
```

Also through Settings → **Update numapilot**. Installed from the package, it updates through
pacman/paru.

If you work on the code from several PCs: GitHub's `main` is the reference. Before changing
anything, `git pull --ff-only`; afterwards, run the same as the CI, commit and push right away (do
not leave local-only commits). Every feature depends on the detected hardware, so a fix on one PC
does not break the others; run `selftest` on them after updating.

```bash
cargo fmt --check && cargo test --release \
  && cargo clippy --release --all-targets -- -D clippy::correctness -D clippy::suspicious \
  && cargo build --release --no-default-features
shellcheck -S warning install.sh uninstall.sh packaging/gfx906-rocm container/lab-start packaging/arch/numapilot.install
git pull --rebase && git push
```

To push from a new PC: `sudo pacman -S github-cli` and `gh auth login`. Each `vX.Y.Z` tag produces
a release with prebuilt binaries (`.github/workflows/release.yml`).

## The interface, tab by tab

The `numapilot` service (root) applies everything; the interface (`numapilot-gui`) and the command
line talk to it. If the service stops, what it changed goes back to what it was. Long commands run
in a window with live output, without freezing the interface. Light and dark themes; tray icon
with temperatures and shortcuts (`numapilot-gui --tray`, or Settings).

The Programs, Startup, Services and Map screenshots are from the CC150 PC. The Graphics cards,
Machine, Adjust…, laptop and BC-250 ones use simulated hardware with example values
(`tools/capturas`), to show each kind of machine.

### Programs

**Find installed app…** lists everything installed (KDE menu, Flatpak, Snap, Steam games,
AppImages) to set processor, memory and card without having to open the program first:

![Find installed app](docs/buscar-app.png)

If a program is on a different card from the chosen one, the Card column flags it (≠). To check the
choice: the "Test this card's selection" button in Graphics cards, or `numapilot gpu-test`.

A task-manager-style table, with programs grouped (Steam games by AppId) into **Applications** and
**Session and system**:

- columns **Name, PID, CPU, Memory, Disk, GPU, GPU memory and Card**, with the total in the header.
  Click a header to sort. CPU = % of the machine; memory = PSS (what is shared between processes
  counts once); disk = read + write right now; GPU and GPU memory from DRM `fdinfo` (amdgpu, Intel,
  NVIDIA with a recent driver);
- the arrow (or a double-click) opens the program's processes and the controls: **Processor**
  (automatic, the GPU's socket, one socket or both; applies right away), **Graphics card** (applies
  on start: **Reopen**, **Use in the menu** or the Steam option), **Lossless Scaling**, **Adjust…**
  and **Open at startup**;
- **End** (SIGTERM) and **Force stop** (SIGKILL, with confirmation) only for your own session
  programs; never the kernel, the system, other users, the graphical session or numapilot itself;
- **Search** by name or PID, **Only mine**, **Steam** menu (apply to all games).

**Adjust…** (also in Saved choices) is the fine control of one program:

| Option | What it does | When it applies |
|---|---|---|
| Processors | socket 0, 1, both or the GPU's | right away |
| Cores | N CPUs, with or without hyper-threading, or picked on the map | right away |
| The program sees | only the allowed cores (`WINE_CPU_TOPOLOGY` in Proton, `OMP/MKL/…_NUM_THREADS`, `GOMAXPROCS`, .NET, Java) | on start |
| Memory | socket only, prefer local, interleave or free; limit in GB | right away |
| Priority, exclusive cores, low latency | CPU/disk, cores of its own, no deep idle states | right away |
| Graphics card, GPU power profile, machine mode | GPU and use (game/AI), 3D/compute/VR, Game/AI/Quiet while it runs | on start |
| gamescope, upscaling, optimize FPS | see [Games](#games) | on start |

Ready-made profiles: **Game**, **AI**, **Rendering**, **Quiet**, among others. A preview shows what
will happen. Underneath, one cgroup per program in `numapilot.slice` (created by systemd). The
choices live in `/var/lib/numapilot/profiles.json` (`numapilot profile list|set|rm`).

![Program editor](docs/editor.png)

### Startup

Everything that starts when the system boots, like on Windows, with what is running and the
memory: **login programs** (user and system autostart, KDE's "reopen what was open"), **user
services**, **system services (boot)** and **session components**.

- Turning on/off applies to the next login or boot and is reversible: for system autostart,
  numapilot creates a copy in `~/.config/autostart` that only hides it (turning it on deletes the
  copy); user services use `disable` (or `mask`, when the package enables it for everyone); system
  services ask for the password. The essentials (Plasma, polkit, network, login, numapilot) cannot
  be turned off.
- **+ Add**: installed program (searches the menu), program open now, file (AppImage, `.sh`,
  binary) or command, with an optional wait before starting. **Remove** deletes what you created.
- **Details**: where the item comes from — the file that starts it (and the package that owns it),
  the real program (the script, when it is `node script.js` or `python x.py`), the **source**
  (repository or AUR package, Flatpak, npm, pipx, cargo, community program, AppImage or a file
  installed by hand), description, size, program folder and configuration folders, with **Open
  folder**, **View in the Programs tab** and the log (services).
- **Uninstall…**: the right way for each source (`pacman -Rns`, `flatpak uninstall`,
  `npm uninstall -g`, `pipx uninstall`, `cargo uninstall` or deleting the files), stopping the
  service and deleting the startup file that would be left behind; optionally also the settings and
  the program's folder. It shows what it will do first and runs in a terminal (pacman still asks
  for confirmation). It refuses for system packages and for numapilot itself.

```bash
numapilot startup                                        # list
numapilot startup off xdg:steam.desktop                 # on / off / remove
numapilot startup app zapzap                             # installed program
numapilot startup file ~/Apps/Program.AppImage --delay 10
numapilot startup source user:openclaw-gateway.service     # where it comes from
numapilot startup uninstall user:openclaw-gateway.service --config
numapilot startup command "Local AI" "numapilot run --gpu compute -- llama-server -m model.gguf"
```

![Startup](docs/inicializacao.png)

### Services

Every systemd service, **system** or **user**, with state (running, stopped, failed), whether it
starts at boot and memory; Running, Stopped, Failed and All filters; **Start**, **Stop**,
**Restart** (the system ones ask for the password) and **Log** (journal). The essential ones cannot
be stopped.

```bash
numapilot services --running            # or --failed, --user
numapilot services restart docker
numapilot services log docker -n 100
```

![Services](docs/servicos.png)

### Map & measurements

Usage of each thread over the last minute (or the core map per socket, with who is pinned to each
one), processor temperature, **disks** (read, write, % active) and **network** (download, upload)
over the last minute, memory and governor per socket, allocations on the other socket, and the
**latency and bandwidth** measured between the sockets (`numapilot bench` or **Measure now**).
**Measure cores** (`numapilot bench-cores`) measures cache latency per core and, when the difference
is larger than the noise, makes core choices prefer the fastest ones.

![Map & measurements](docs/mapa.png)

### Graphics cards

For each card: temperatures (edge, hotspot, memory), usage, VRAM, 1 h/6 h/24 h history, PCIe link
and Resizable BAR.

- **Fan**: the card's automatic mode, **curve** (temperature → %) or **fixed**; on RDNA 3/4 it uses
  the curve stored in the firmware. At 95 °C it goes to 100% in any mode.
- **Power**: limit in W within the card's minimum and maximum; **power saving when idle**.
- **Clock and voltage**: 3-point curve (MI50/Vega 20, RDNA 1), **voltage offset** (RDNA 2, 3 and 4)
  or, on NVIDIA, locked clock + curve offset through NVML.
- **OverDrive** (AMD, needed for clock/voltage and for the fan on RDNA 3/4): button in the tab or
  `sudo numapilot boot overdrive on`. It writes `ppfeaturemask` to `/etc/modprobe.d` (survives
  kernel updates) and rebuilds the boot image; reboot.
- **Automatic undervolt** (**Find the undervolt automatically** or `numapilot undervolt GPU`):
  lowers the voltage (AMD, 10 mV steps) or raises the curve with the clock locked (NVIDIA), with
  1 min of load (`vkmark`/`glmark2`) per step; it fails if the test crashes or the kernel logs a GPU
  reset. It goes back one step of margin, confirms for 2 min and saves. If the PC freezes, it comes
  back with the last good value.
- **NVIDIA driver**: which series fits, what is installed and the **Install official driver** and
  **Update** buttons (CachyOS's `chwd`; `numapilot driver status|install|update`).
- **Community boards** (BC-250, BC-160): see [BC-250 and BC-160](#bc-250-and-bc-160).
- Tray (and phone) alert when a GPU goes over 90 °C.

![Graphics cards](docs/gpus.png)

### Machine

- **One-click modes** (`numapilot machine mode game|ai|quiet|normal`): *Game* (the card's
  socket in `performance`, uncore at maximum, no NUMA balancing, LAVD in gaming mode), *AI* (both
  sockets in `performance`, THP `always`), *Quiet* (no turbo, power saving, limited power).
  Automatic **per program** (while it runs) and **by schedule**; on a laptop, by AC power/battery.
- **Per socket**: governor, energy bias (EPB/EPP), minimum and maximum frequency, **uncore** and
  **power limit** (RAPL, PL2), with live power draw and temperature.
- **Maximum turbo**: a program alone on a few cores of one processor so the clock reaches the
  highest turbo step (servers with 2 processors); `numapilot turbo` shows the processor's table.
- **Memory**: NUMA balancing, `zone_reclaim`, THP, hugepages per socket. **sched_ext** through
  `scxctl`.
- **Motherboard fans** (nct67xx, it87...) in automatic, curve or fixed; IPMI on servers.
- **Processor mitigations** one by one (only the ones that affect yours), with risk and gain; and
  **kernel parameters** without editing the bootloader by hand (Limine, systemd-boot, GRUB, rEFInd;
  with a backup; `sudo numapilot boot cmdline apply +pci=realloc`). They apply after a reboot.
- **Energy**: power right now, kWh and cost for the day and the month.
- **Hardware health**: disk SMART, ECC (EDAC), errors on cards with ECC, battery wear.
- **Wake over the network** (Wake-on-LAN).
- **Laptop**: see [Laptop](#laptop).

![Machine](docs/maquina.png)

### Lab, AI server and Streaming

- **Lab**: container with JupyterLab, VS Code (browser or the PC's own through SSH), PyCharm and a
  terminal, with PyTorch on the GPU and the data and ML libraries; lists the containers and the
  apps in each. Details in [AI and lab](#ai-and-lab).
- **AI server**: picks the `.gguf` model, the GPUs and the context; start/stop; chat and API address
  (OpenAI-compatible); **Optimize (tokens/s)**.
- **Streaming**: turns on Sunshine and picks the GPU and the encoder (VA-API, NVENC, Quick Sync or
  software).

![Lab](docs/laboratorio.png)

### Settings and Tests

- **Phone panel** (QR code), **My other PCs**, **phone notifications**, tray, **language**, weekly
  ROCm update, boot options, **Windows virtual machine**, backup and **Update numapilot**.
- **Tests**: manual script with only this machine's items; **Generate report** puts together the
  answers, the self-test and the logs.

## Command line

| Command | What it does |
|---|---|
| `numapilot status` / `topo` | programs managed right now / nodes, cores, RAM and GPUs |
| `numapilot run [--cpu …] [--gpu …] -- program` | starts a program with the chosen processor, memory and GPU (see below) |
| `numapilot pin PID --cpu 1` | moves a program that is already running |
| `numapilot profile list\|set\|rm` | saved choices per program |
| `numapilot startup …` / `numapilot services …` | startup / services |
| `numapilot machine mode game\|ai\|quiet\|normal` | machine modes (game, AI, quiet, normal) |
| `numapilot turbo` / `numapilot bench-cores` | turbo table and awake cores / cache latency per core |
| `numapilot undervolt GPU` | automatic undervolt |
| `numapilot gpu-test` | checks that each card choice lands on the right card |
| `numapilot boot status\|overdrive on\|gpu-memory N\|cmdline …` | boot options |
| `numapilot steam enable\|disable\|list` | numapilot in the launch options of every Steam game |
| `numapilot upscale fetch\|status\|install\|remove APPID` | OptiScaler per game |
| `numapilot tune start\|status\|apply GAME` | FPS optimizer |
| `numapilot peers add\|list\|remove\|wake\|pull\|backup` | other PCs |
| `numapilot web` | phone panel |
| `numapilot driver status\|install\|update` | NVIDIA driver |
| `numapilot hybrid …` / `numapilot autostart igpu\|normal` | laptop NVIDIA |
| `numapilot vm check\|prepare\|unprepare\|create` | Windows VM with its own card |
| `numapilot board status` / `recipes show\|update` | special boards and community recipes |
| `numapilot doctor [--report]` / `selftest` / `log` | diagnosis, self-test, logs |
| `numapilot backup` / `self-update [run]` | backup / update |
| `gfx906-rocm …` | ROCm, lab and AI server (see [AI and lab](#ai-and-lab)) |

`numapilot --help` and `numapilot COMMAND --help` show every option. Messages follow the language
chosen in the interface (or `NUMAPILOT_LANG=pt`, or a Portuguese system locale); command names and
`--help` are always in English. The Portuguese command names from older versions (`inicio`,
`servicos`, `autoteste`, `mode jogo`...) are still accepted.

## Server with 2 processors

### Automatic mode

The service watches the user's programs and, for each one:

| Situation | What it does |
|---|---|
| Matches a rule in the config | applies it right away, before much memory is allocated |
| Child of a program with a rule (game started by Steam) | inherits the decision |
| Opens a GPU | goes to the **socket connected to that GPU** |
| Uses CPU (≥ 0.5 core for 3 s) with no GPU | goes to the **least busy socket** with free RAM |
| Uses ≥ 85% of the socket's CPUs for 10 s | **expands to both sockets** and remembers it |
| Expanded, but using < 30% for 2 min | goes back to 1 socket |
| Seen before | starts in the learned place |

As root, it gets a notice from the kernel for every program started (proc connector), so memory is
born on the right socket. It does not touch system processes, anything that set its own affinity
(`taskset`) or pinned programs. `numapilot learned` shows what it learned;
`journalctl -u numapilot -f` shows the decisions.

### Choosing on start (`numapilot run`)

The GPU can only be chosen when the program starts (through environment variables):

```bash
numapilot run -- blender                                 # automatic
numapilot run -n -- llama-server -m x.gguf               # only shows what it would do, and why
numapilot run --gpu display --cpu gpu -- ./game          # graphics card + its socket
numapilot run --gpu gfx906-1 --use graphics -- %command% # game rendered on an MI50
numapilot run --gpu compute:all --cpu all -- python train.py
numapilot run --cpu all --mem interleave -- ./database
```

| Option | Values |
|---|---|
| `--cpu` | `auto`, `all`, `gpu`, `0`, `1`, `0,1`, `cpus:0-21` |
| `--gpu` | `auto`, `display`, `compute`, `compute:2`, `compute:all`, `none`, alias or PCI |
| `--mem` | `local` (default), `interleave`, `bind` |
| `--use` | `auto`, `graphics`, `compute` |
| `--fg-gpu` | GPU for Lossless Scaling frame generation |

On automatic, the card with the monitor goes to graphical programs and the compute ones (MI50) to
AI (`llama-*`, `ollama`, `comfyui`, `vllm`, `whisper*`...), always the least busy. It sets
`DRI_PRIME`, `MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE`, `ROCR_VISIBLE_DEVICES`,
`CUDA_VISIBLE_DEVICES`, NVIDIA PRIME and `LSFGVK_GPU`, and exports `NUMAPILOT_CPUS`,
`NUMAPILOT_MEMS` and `NUMAPILOT_RENDER` for scripts and containers.

### Permanent rules

`/etc/numapilot.toml` (commented example included), then `sudo systemctl restart numapilot`:

```toml
[aliases]
"0000:03:00.0" = "rx470"

[[rule]]
name = "games"
match = ["steam", "*.exe", "gamescope"]
cpu = "gpu"        # the graphics card's socket
gpu = "display"
reserve = true     # automatic mode keeps other heavy loads off this socket
```

Interrupts from GPUs, network cards and NVMe go to each device's socket (`device_irq`).

### Games on a 2-processor server

| Problem | Cause | What the **Game** profile does |
|---|---|---|
| Game does not start or the anti-cheat blocks it | the game sees 88 threads | `WINE_CPU_TOPOLOGY` makes Proton see only the allowed cores |
| Very low FPS | threads and memory spread over both sockets | 1 socket (the GPU's), physical cores, local memory, high priority, reserved cores |
| Refuses to render on one card and display on another | card without a monitor or on the other socket | only the chosen GPU stays visible and a warning asks you to connect the monitor to it |

DirectX runs through Proton (DXVK/VKD3D → Vulkan), so everything that holds for Vulkan holds for
both. To play **on an MI50**: choose "gfx906-N — game/graphics" (with the monitor on the MI50, or
PRIME to the RX 470). **Lossless Scaling** (lsfg-vk): generates the frames on the game's GPU; dual
GPU needs a fork.

## AI and lab

`gfx906-rocm` (name kept for compatibility; its messages are still in Portuguese) detects the PC's
compute stack and uses the right images, always validating before updating and rolling back
automatically:

| Stack | Cards | Lab (PyTorch) | AI server (llama.cpp) |
|---|---|---|---|
| `gfx906` | MI50, Radeon VII | community images (mixa3607/ML-gfx906) | community gfx906 build |
| `rocm` | RX 6000/7000/9000, RDNA 3.5 APUs | `rocm/pytorch` | `llama.cpp:full-rocm` |
| `cuda` | NVIDIA | `pytorch/pytorch` | `llama.cpp:full-cuda` |
| `vulkan` | BC-250, BC-160, Intel, others | PyTorch on the CPU | `llama.cpp:full-vulkan` |

```bash
sudo pacman -S podman
gfx906-rocm lab up            # lab: JupyterLab :8888, VS Code :8443, SSH :2222
gfx906-rocm lab test          # does PyTorch see the GPUs? does it compute on each one?
gfx906-rocm llm up model.gguf    # AI server (models in ~/models)
gfx906-rocm llm tune model.gguf  # finds the fastest combination (tokens/s)
gfx906-rocm check             # is there a new version? (changes nothing)
gfx906-rocm lab update        # builds alongside, validates on the GPUs and only switches if it passes
gfx906-rocm history           # each update with its result and performance
```

- **Lab**: numpy, pandas, polars, duckdb, scikit-learn, xgboost, lightgbm, transformers, datasets,
  accelerate, peft, lightning, timm, JupyterLab, code-server, ruff, pytest; projects in `~/lab`,
  models in the host's Hugging Face cache. The image's PyTorch stays pinned.
- **Updates that do not break things**: each new version is validated in a separate container (GPU
  results equal to the CPU's, VRAM, performance; more than 10% slower is refused) before switching.
  `gfx906-rocm hold` pins a component; the weekly check can just notify or install right away.
- **Native ROCm for the MI50** (side by side in `/opt/rocm-gfx906`, without touching the system's
  ROCm): `sudo gfx906-rocm update|rollback|list|prune`, `eval "$(gfx906-rocm env)"`.
- With numapilot, the container gets only the chosen GPUs and the cores and memory of their socket:
  `numapilot run --gpu gfx906-1 --use compute -- gfx906-rocm lab up`.
- To start it at boot, use the **Startup** tab → + Add → Command.

![Updates](docs/atualizacoes.png)

## Laptop

- Games go to the dedicated card on automatic; the integrated one shows the image.
- **Battery**: charge, power draw, health, stop charging at 80%, power profile, automatic mode on
  AC power and on battery.
- **NVIDIA only when needed** (`numapilot hybrid auto|hybrid|integrated|leave`): always
  available (sleeps when nobody uses it), off (the service removes the card from the system; KDE
  carries on with the integrated GPU without leaving the session) or automatic (AC power/battery).
  It shows who keeps the card awake and which card each display is on.
- **Everything that starts with the system on the integrated GPU** (`numapilot autostart igpu`),
  Xwayland on the integrated GPU only, and programs on the integrated GPU that do not load the
  NVIDIA driver.
- **Ryzen**: power limits through `ryzenadj` (reapplied when the firmware changes them). **Acer
  Predator/Nitro** with [Linuwu-Sense](https://github.com/0x7375646F/Linuwu-Sense): fans, charge
  limit, keyboard light, RGB.

![NVIDIA and laptop](docs/nvidia.png)

## BC-250 and BC-160

The community recipes (`recipes/default.toml`, updatable with `sudo numapilot recipes update`)
teach numapilot about special boards.

**BC-250** (Graphics cards → community tweaks):

- **★ BC250 Control Center** featured: the
  [community program](https://github.com/movacx/bc250-control-center) that manages **cores, CUs,
  frequencies and voltages** (GPU governor and voltage lab, CPU overclock, fan, 40 CUs live,
  8 cores, TTM). One button installs it from the author's `-git` package and then becomes **Manage
  cores, CUs, frequencies and voltages**.
- **Other programs** (umr, bc250-cu-live-manager, bc250-core-unlock, bc250-core-cu-unlock,
  bc250_smu_oc, BC250-Telemetry), always on the newest version: from the AUR `-git` or by git in
  `~/.local/share/numapilot/comunidade` (commands in `~/.local/bin`); **Update** rebuilds or runs
  `git pull`. The unlock EFI and the memory temperature only have a link (they touch the boot or
  the SMU).
- **Now**: cores (6 or 8) and CPU clock, CUs (24 or 40) and GPU clock. The CUs come from BC250
  Control Center's live reading, from the kernel that already unlocks all 40
  (`bc250_cc_write_mode=3`) or from boot; it says whether the 8 cores survive unplugging
  (biosmod/EFI) or not.
- **Diagnosis**: kernel (versions that break the GPU), minimum Mesa, clock governor,
  `amd_iommu=off`; scrambled telemetry with 8 cores (with the right fix for the running kernel);
  reliable clock from hwmon.
- **Governor**: install, enable and edit the configuration (with a backup and automatic rollback).
- **Memory for the GPU (AI)**: `sudo numapilot boot gpu-memory 14` frees up to ~14 GB for the GPU
  (models up to ~14B); the AI server sets `GGML_VK_PREFER_HOST_MEMORY`.
- Sunshine with software encoding (no video encoder).

**BC-160**: no video output; render on it and display through the other card.

![BC-250](docs/bc250.png)

## Games

- **Steam on every game** (`numapilot steam enable` or the Steam menu in the Programs tab): puts
  `numapilot run --` before `%command%` keeping what was already there; Steam must be closed.
- **gamescope per game**: game and output resolution, FPS limit, FSR, on the same chosen GPU.
- **Upscaling per game** (in Adjust…), depending on the card:

| Mode | For | How |
|---|---|---|
| FSR 4 | RDNA 4 (RX 9000) | `PROTON_FSR4_UPGRADE=1` |
| FSR 4 for RDNA 3 | RX 7000 | `PROTON_FSR4_RDNA3_UPGRADE=1` |
| Newest FSR 3.1 / DLSS / XeSS | whatever the game already supports | `PROTON_*_UPGRADE` |
| OptiScaler | any card | `dxgi.dll` in the game's folder (the original is restored on removal); experimental FSR 4 INT8 |

  Do not use OptiScaler in online games (anti-cheat).
- **FPS optimizer**: tests settings (cores, sockets, cards) each time the game starts, measures 60 s
  with MangoHud and picks the best (favoring the 1% low).
- **From the phone**: lists the Steam games with **Open** and **Start Sunshine**; with Wake-on-LAN
  you can turn the PC on, start the game and connect with Moonlight from the couch.
- **Windows VM with its own card** (games with anti-cheat that do not run on Proton):
  `numapilot vm check`, `sudo numapilot vm prepare GPU`, `numapilot vm create NAME --gpu GPU --iso Win11.iso`.

![FPS optimizer](docs/otimizar.png)

## Several PCs and phone

- **Phone panel** (Settings → Phone panel, QR code): temperatures, usage, programs with one-tap
  profiles, lab and AI. Protected by a token; away from home, use a VPN (Tailscale) instead of
  opening port 8577. The panel follows the language chosen on the PC.
- **My PCs** (`numapilot peers add "http://IP:8577/?t=..." NAME`): all PCs in one panel, 24 h chart,
  each one's version, **Wake** (Wake-on-LAN), copy profiles, daily backup in
  `~/numapilot-backups/NAME/`; `/metrics` in Prometheus format.
- **Phone notifications** through [ntfy](https://ntfy.sh): hot GPU, disk/memory problem, AI server
  that stopped, PC that left the network, undervolt done.

## Diagnosis and tests

- `numapilot doctor` tests on the real hardware (NUMA, cgroups, service, each GPU, Vulkan/OpenGL,
  groups, tools, Steam, special boards) and gives the command that fixes each problem.
  `numapilot doctor --report` saves a report to send.
- `numapilot selftest` checks everything without changing anything and writes
  `~/numapilot-autoteste-MACHINE-DATE.txt`.
- **Tests** tab and [docs/TESTES.md](docs/TESTES.md) (still in Portuguese): manual script per machine.
- `numapilot log`: what the service, the interface and the commands did.

## Limitations

- The service changes the processor and memory of running programs, but the GPU only on start.
- Vulkan compute programs choose the GPU by their own index (`GGML_VK_VISIBLE_DEVICES`); numapilot
  does not set that yet.
- sched_ext and ananicy-cpp coexist with numapilot (they respect the affinity); it is worth
  measuring with and without.
- RAM frequency and timings can only be changed in the BIOS.
- Network per program is not shown (the kernel does not report it without eBPF).
- Still in Portuguese, being translated: code comments, the installer, `gfx906-rocm`, the test
  guide and the older changelog entries.

## Development

Rust (2024 edition). The interface uses egui; the tray, ksni. Contributions are welcome, in
English.

```
src/main.rs        command line
src/daemon.rs      service: automatic mode, rules, applying the settings
src/ctl.rs         control socket (interface and CLI talk to the service)
src/topo.rs        NUMA nodes, cores, RAM, GPUs
src/decide.rs      GPU + socket decision (numapilot run)
src/procfs.rs      /proc, affinity, memory policy, migration
src/procevents.rs  instant notice of new programs (proc connector)
src/procctl.rs     protected end; GPU per process (fdinfo)
src/iostat.rs      disk and network (diskstats, /sys/class/net, /proc/PID/io)
src/startup.rs     startup (autostart, KDE session, systemd)
src/services.rs    systemd services
src/profiles.rs    choices per program;  src/cgroup.rs  cgroups per program
src/gpuctl.rs      sensors, fan, power, OverDrive, ReBAR;  src/nvctl.rs, nvml.rs  NVIDIA
src/undervolt.rs   automatic undervolt
src/cpuctl.rs      C-states, turbo, frequency, EPB/EPP, uncore, RAPL
src/turbo.rs, corebench.rs   turbo table and cache latency per core
src/machine.rs     machine modes and settings;  src/fans.rs  motherboard fans
src/cmdline.rs     kernel parameters and mitigations;  src/bootcfg.rs  modprobe.d
src/notebook.rs, hybrid.rs, ryzen.rs, acer.rs, autostart.rs   laptop
src/boards.rs, recipes.rs   special boards and recipes (recipes/default.toml)
src/health.rs, energy.rs, history.rs, notify.rs   health, energy, history, notifications
src/web.rs, peers.rs        phone panel and other PCs
src/steam.rs, tuner.rs, upscale.rs   games
src/containers.rs, labcfg.rs        lab and AI server (packaging/gfx906-rocm)
src/vm.rs          VM with VFIO
src/doctor.rs, selftest.rs, log.rs   diagnosis, self-test, logs
src/i18n.rs, i18n_pt.rs  language and Portuguese texts (service and CLI)
src/bin/numapilot-gui/   interface (one tab per file), tray and its Portuguese table (i18n_pt.rs)
container/         lab image
```

English is the language of the code. Text shown to the user goes through `tr("text")` with its
pair in the Portuguese table, or `tf!("text {}", "texto {}", value)` when it has values. Tests fail
when a `tr()` text has no Portuguese pair.

`cargo test` runs the tests, with several machines simulated by a fake sysfs (`NUMAPILOT_SYSFS`).
The README screenshots come from `tools/capturas` (fake server, laptop and BC-250, on a virtual
display); see the README there. The CI (GitHub Actions) runs formatting, tests, clippy, a build
without the interface and shellcheck on every push. Changes are in the [CHANGELOG](CHANGELOG.md).

## License and credits

[MIT](LICENSE) © 2026 Lucas Fracaro. The [Inter](https://github.com/rsms/inter) font, bundled in
the interface, is by The Inter Project Authors, under the
[SIL Open Font License 1.1](assets/fonts/OFL.txt).

BC-250 support uses community knowledge, especially from
[BC250 Control Center](https://github.com/movacx/bc250-control-center) (MIT, Herlin Chavarria):
the SMU metrics table, CUs unlocked by the kernel and persistence of the 8 cores. ROCm and the
gfx906 images come from [mixa3607/ML-gfx906](https://github.com/mixa3607/ML-gfx906) and TheRock.
