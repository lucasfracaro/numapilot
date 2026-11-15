# Mudanças

## Ainda não lançado

- **Interface no longer freezes on the BC-250** (switching tabs took ~2 s, scrolling stuttered).
  Measured on the board: the voltage regulators (`bc250_vrm`) are read over I2C, 20-47 ms per
  file and ~0.3-0.5 s for all of them, and the interface read them on screen once a second,
  plus twice per frame on the GPU tab. Other readings done while drawing: the system services
  list (~0.8 s every 5 s), the startup list (~0.9 s every 5 s), the board's first setup
  (~0.15 s), systemctl for the clock governor, the EFI core unlock check and each process's
  memory (PSS, ~70 ms every second). All of them now run in threads and the screen shows the
  last result. The service and the command line reuse a VRM reading under 1 s old.
- The interface logs a frame that took more than 150 ms, with its tab (`numapilot log`).

- Service log: the automatic machine mode line is in English on every system (it came out as
  "regra automática" on a pt_BR one).

## 0.5.2 — 2026-11-15

- QA round on the Nitro 5 laptop:
  - **Core order: measured cache latency where it matters, the firmware's preferred cores where
    it does not.** When the cache measurement is reliable (the 2x Xeon's ring) it decides; when it
    is not (a single-CCX Ryzen, where every core sees the L3 the same) the firmware's ranking
    decides (`amd_pstate_prefcore_ranking`, or ACPI CPPC `highest_perf` on Intel Turbo Boost Max /
    hybrid); otherwise the system order. On the Nitro 5: 2 cores → CPUs 6, 10; 6 cores → without
    the two the firmware ranks last. `run --dry-run` and `bench-cores` say which order is in use.
  - **Cache order per core measured before it is needed.** A profile that picks some of the
    cores (a count, no hyper-threading, or a processor limit as cores) on a processor never
    measured: `numapilot run` measures first and then picks the cores (dry-run says it will);
    the interface measures before saving such a profile; the service measures as soon as the PC
    is idle and applies the order to the programs it placed. A measurement deleted with
    `bench-cores --clear` counts as a choice (system order) and is not redone.
  - **The order must repeat to be used.** On the Nitro 5 a measurement gave 8.8% between cores
    with 1.2% noise and was accepted, but the fastest core changed at every new measurement (the
    firmware's preferred-core ranking even put that "fastest" core last). The even and the odd
    rounds are now judged apart and must agree (rank correlation ≥ 0.8); 8 rounds instead of 4;
    automatic measurements are skipped while the PC is busy, and `bench-cores` warns about it.
  - **Processor limit works with sched_ext (LAVD).** The kernel ignored `cpu.max` (measured: a
    150% limit ran at 16 cores). Under a sched_ext scheduler the limit now becomes a number of
    physical cores, rounded up (150% → 2 cores; measured: 2 cores' worth), in `run`, in the
    service and in the interface, which says how it was applied.
  - **"N CPUs" and hyper-threading:** with HT on, 4 CPUs are 2 physical cores with both threads,
    half the speed of 4 cores (measured 27 k vs 51 k). Turning on "limit to" in the interface
    now starts with HT off and shows how many physical cores the number means; `run --dry-run`
    says it too. The meaning of HT on is unchanged (the maximum-turbo panel relies on it).
  - Exclusive cores reserve whole physical cores: nothing else runs on the other thread of a
    reserved core.
  - **Exclusive cores, maximum turbo and the per-program machine mode did nothing for programs
    opened by `numapilot run`** (which is how Steam games start with numapilot): the service
    marks them "manual (numapilot run)" and those features only looked at programs it had placed
    itself. Measured on the Nitro 5: opened directly, a profile with exclusive cores 0-3 moved
    the rest of the system to 4-15; through `run`, nothing. They now count both, like low latency
    already did.
  - A profile **without a CPU choice** (only the machine mode, for example) never took effect
    on a one-socket PC: the service only "places" a program when there is something to place,
    and those features waited for that. Profiles now take effect while the program is followed
    at all (placed or watched) or opened by `run`.
  - Saving a profile right after the program closed reported "1 process adjusted": the process
    was still in the service's table. Exited processes are skipped.
  - **The NVIDIA "off" mode hung the laptop card at login.** Right when the old KWin quit, the
    service saw the card free and removed it; the new KWin opened it during the removal and the
    driver waited for it until a reboot (it also froze the service's main loop). The removal now
    needs a running KWin started without the NVIDIA (the service reads its environment) and the
    card free for 6 s in a row, and it runs in its own thread. The login script also sets Mesa's
    EGL for the session: with `KWIN_DRM_DEVICES` alone KWin's EGL still loaded the NVIDIA driver.
  - **Desktop only on the integrated GPU** (KDE): Machine → Laptop or
    `numapilot hybrid desktop integrated|all`. KWin opens `/dev/nvidia0`, `nvidiactl`,
    `nvidia-modeset`, the render node and the card, so the NVIDIA never sleeps; with the numapilot
    service stopped and Dynamic Boost off it still stayed awake, and taking it off the PCI bus with
    KDE open hangs (NVRM waits for KWin forever). A Plasma env script now limits KWin to the
    integrated GPU at each login unless a screen is connected to an NVIDIA output (HDMI, or the
    laptop screen with the MUX on the NVIDIA). Cards are found by PCI address at login; turning it
    off also clears the variable from the systemd user manager (linger). The doctor points to it.
    The README no longer says KDE carries on after the card is removed with the desktop holding it.
  - **Laptop NVIDIA that never sleeps was reported as "nothing holding it".** KWin runs with
    `cap_sys_nice`, so its `/proc/PID/fd` is hidden from the user: doctor, selftest and
    `hybrid status` (run as the user) could not see that KWin keeps a context with VRAM on the
    NVIDIA (the card never suspended in 26 h). The service now publishes in `dgpu.json` who keeps
    the card awake, in every mode and on AC too; the user-side tools read it. The doctor shows
    "desktop holding the NVIDIA" with the `KWIN_DRM_DEVICES` fix and names the NVIDIA outputs
    that stop working with it (HDMI on the Nitro 5); a warning on battery. `hybrid status` shows
    the card's state and holders, and no longer says Xwayland "does not hold the NVIDIA".
  - The doctor said "acer_wmi predator_v4: on" because of a `modprobe.d` line, even with
    acer_wmi blacklisted and not loaded. It now reads what the kernel loaded: acer_wmi and its
    parameter, linuwu_sense, its model parameter and whether it exposed anything, the blacklist,
    and whether any power profile exists. When linuwu_sense is loaded but did not recognize the
    laptop it says so and gives the `nitro_v4=1`/`predator_v4=1` option for the model.
  - "Other controllers running" now knows DAMX (Div Acer Manager Max, which reloads
    linuwu_sense by itself) and LACT, and gives the `systemctl disable --now` for their service.
  - Rules no longer match words that ordinary commands name (`man make`, `nvim steam`,
    `which blender`): the first arguments only count as the program after a launcher or
    interpreter (`python`, `env`, `flatpak run`, `gamemoderun`, `bash script`...).

- **Laptop with NVIDIA: `run --gpu nvidia-0` opened Vulkan programs on the integrated card.**
  The NVIDIA branch set `MESA_VK_DEVICE_SELECT*`/`DRI_PRIME` and also listed them to be removed;
  `run` removed after setting, so Mesa's device-select layer left the Radeon first and programs
  that take Vulkan device 0 (native games, vkmark) ran on it. `gpu-test` applied the lists in the
  opposite order and reported "right". A variable is now either set or removed, both apply the
  lists in the same order, and tests check the final environment on a Radeon + NVIDIA laptop.

- QA round on the BC-250:
  - A profile saved with an exact CPU list (`profile set KEY --cpu cpus:4-7`) got the whole
    socket: the service turned the list into its node. It now keeps the list.
  - Machine modes on a PC without cpufreq (BC-250) no longer log 16 governor errors and a turbo
    warning: each setting is only applied where the system exposes it.
  - `run --cpu cpus:0-99` on a 16-thread PC is cut to the CPUs that exist (and fails when none do).
  - `run --compute` on a PC with no ROCm card and a single GPU uses that GPU, as `--gpu compute`
    already did.
  - Tools that only name a game (`pgrep -f Game.exe`, `pkill`, `strace`...) no longer match the
    games rule through its `*.exe` glob.
  - Free RAM in the web panel and in the automatic node choice counts the file cache as free
    (the panel showed "1 of 15 GB free" with 10 GB available).
  - `profile list` shows cores, exclusive, limits, priority and mode; `vm check` suggests a card
    of this PC (it named the server's `rx470-0` everywhere) and nothing when there is only one.

- **Maximum turbo now delivers the clock.** While a program with this profile runs, its socket
  goes to the `performance` governor (and back afterwards). Measured on the 2× Xeon server
  (schedutil + scx_lavd), the same 4 reserved cores did 256 M iterations/s per thread before
  and 419 with `performance`.
- A program with exclusive cores or maximum turbo **reopened right after closing** (or started
  while another one held the cores) lost its profile: the kernel refused the affinity while the
  process was still in the rest of the system's control group, and the service gave up. It now
  goes into its own group first.
- **Awake-core count (Turbo line, `numapilot turbo`)**: a core that slept through the whole
  interval was counted as awake (a socket with 4 busy cores and 18 asleep read "21 awake"),
  because the kernel only credits idle time when the core wakes up.
- Profiles with **exclusive cores, maximum turbo or a machine mode** now need the same
  permission as changing the machine (`admin_groups`): they affect every user of the PC.
- Adjust…: the **processor limit** warns when a sched_ext scheduler is running; with scx_lavd
  the kernel did not enforce it (`cpu.max` at 25%, program at 400%, never throttled).
- `profile rm` answers "profile deleted" (it said "saved"); `web --bind 127.0.0.1 --url` gives a
  link with that address, not the LAN one; the last Portuguese words left `startup --help`.
- **Automatic mode expands to both sockets on time.** Each program's CPU usage is now divided
  by its own sampling interval. Before, every ~10 samples the reading swung by ±17% (70 then 50
  cores for a steady 60-core load), which reset the saturation counter: on the 2× Xeon server a
  program at 99% of a socket took 47 s to expand instead of the configured 10.
- **Lab → apps inside the container now open.** Terminal apps (htop, nvtop, vim, python) open in
  a terminal on the PC, and the lab's JupyterLab/Jupyter Notebook shortcuts open the server that
  is already running in the browser; before, they were started detached and showed nothing.
- `gfx906-rocm llm update`: the validation accepts the cards by their commercial name ("AMD
  Radeon Pro VII"), which is how llama.cpp lists the MI50s; it failed with "listed no gfx906".
- `doctor` and `selftest` agree on cards whose driver gives no clock/voltage table with OverDrive
  already on (MI50): no more advice to turn OverDrive on.
- `boot status` says when its file was not created yet and which other file sets OverDrive.
- `selftest`: core and CU unlock checks only on boards that have them (BC-250), not on MI50s.
- Lab: when the image has no PyCharm the button is **PyCharm (install)**: it saves
  `LAB_PYCHARM=1` and rebuilds the image, instead of running a command that could only fail.
- Interface: links and programs it opens no longer stay behind as zombie processes.
- Installer: the final diagnosis waits for the service that has just restarted (it could report
  one failure in its first seconds) and shows the failed lines, which the 25-line tail hid.
- Service: no more empty "GPU …:" log lines; it exits right after restoring everything (it
  once stayed alive until systemd killed it 10 s later).
- **English is now the project's language**, to make collaboration easier. The code's texts, the
  command line, `--help`, logs and reports are in English; the interface (and the messages it
  shows) is available in English and Portuguese (Settings → Language, or `NUMAPILOT_LANG=pt`;
  without a choice, Portuguese only on a `pt_*` system). `README.md` is English and
  `README.pt-BR.md` is the Portuguese version.
- **Command names in English**: `startup` (was `inicio`), `services` (`servicos`), `selftest`
  (`autoteste`), options `--user`, `--running`, `--failed`, `--lines`, `--delay`, `--yes`,
  `--out`, `--quick`, `--show`, `--clear`, and values `machine mode game|ai|quiet`,
  `hybrid hybrid|integrated|leave`, `gpu-memory default`. The old names still work.
- Log levels are now `ERROR`/`WARN`; self-test statuses are `WARN`/`FAILED`/`SKIPPED`.
- The community recipes (`recipes/default.toml`) are in English; their Portuguese texts moved
  to the translation table. The file format version is now 2.
- Code comments, internal names and the installer (`install.sh`, `uninstall.sh`) are in English.
- **Latência do cache por núcleo** (`numapilot bench-cores` e botão "Medir núcleos" no Mapa):
  mede o acesso ao cache L3 a partir de cada núcleo e, quando a diferença é maior que o ruído da
  medição, a escolha de núcleos (perfis com número de núcleos e o turbo máximo) passa a preferir
  os mais rápidos, em ordem. Vale também em PCs de um processador. Se a diferença for pequena,
  nada muda. `--mostrar` exibe a última medição e `--apagar` volta à ordem do sistema.
- **Turbo máximo** (perfil pronto e opção no Ajustar…, só com 2 processadores): o programa fica
  em poucos núcleos e o resto do sistema sai do socket inteiro, para o clock subir ao degrau mais
  alto do turbo. Com a opção marcada, o Ajustar… deixa escolher 2 a 16 núcleos e mostra o clock
  máximo de cada escolha pela tabela do processador.
- **Limite de processador** por programa em % de um núcleo (`CPUQuota`), e perfis prontos novos:
  Edição de vídeo e Segundo plano.
- **Histórico por programa**: o serviço guarda um ponto por minuto (6 h) de processador e memória;
  o Ajustar… mostra média, pico e um gráfico, para escolher limites com dados.
- **Conferência de memória** (Máquina → Saúde): pentes, velocidade em uso contra a suportada,
  pentes diferentes e desequilíbrio entre os processadores.
- **Exportar/Importar escolhas**: aba Escolhas salvas e `numapilot profile export|import`; ao
  importar de outro PC, núcleos e placas que não existem aqui voltam para o automático.
- **Idioma**: Configurações → Português / English (parcial: menu, títulos e barra da aba
  Programas; `src/bin/numapilot-gui/i18n.rs`).
- **Turbo por núcleos acordados** (Intel): o Mapa mostra, por processador, quantos núcleos estão
  acordados e até que clock o turbo vai com isso; passe o mouse para ver a tabela inteira. Avisa
  quando os estados profundos (C3/C6) estão desligados. Também em `numapilot turbo`.
- **Buscar app instalado…** (aba Programas): lista atalhos do menu, Flatpak, Snap, jogos da Steam e
  AppImages, com busca, e abre o Ajustar… sem o programa estar aberto. A escolha fica salva para a
  próxima abertura (`src/apps.rs`).
- **Conferir a escolha de placa**: `numapilot gpu-test [APELIDO]` e o botão "Testar a escolha desta
  placa" rodam vulkaninfo e glxinfo como um jogo rodaria e mostram qual placa respondeu. A aba
  Programas avisa quando o programa está numa placa diferente da escolhida.
- **DirectX**: reserva pelo filtro do DXVK/VKD3D (`DXVK_FILTER_DEVICE_NAME`,
  `VKD3D_FILTER_DEVICE_NAME`) com o nome Vulkan confirmado da placa, quando o modelo é único no PC.
  `NUMAPILOT_NO_DX_FILTER=1` desliga.
- **Grupo de controle desde o início**: o `numapilot run` manda a chave do programa ao pedir os
  limites, e o programa já nasce dentro do grupo certo (antes o pedido usava o perfil do próprio
  numapilot).
- **Permissões do serviço**: mudar a máquina inteira (modos, ventoinhas, clock/voltagem, avisos)
  só para root ou grupos de `daemon.admin_groups` (padrão `wheel`, `sudo`, `numapilot`; `["*"]`
  libera). Escolhas por programa continuam por usuário. Painel web: token criado já com 0600 e
  espera de 2 s depois de 10 tokens errados seguidos.
- **Desinstalar…** mostra sempre os caminhos que serão apagados, os pacotes que saem junto
  (`pacman -Rns`) e os comandos exatos, e grava tudo no registro.
- **Abas do menu**: Configurações → desmarcar as que este PC não usa.
- **BC-250, primeira configuração**: passos em ordem (pronto / falta / opcional) na aba Placas e em
  `numapilot board setup`.
- **Desinstalar…** nunca apaga pastas de ferramentas ou de trabalho como "pasta do programa"
  (`~/go`, `~/.cargo`, `~/.conda`, `~/.dotnet`, `~/flutter`, `~/Projetos/...`, `~/Downloads`...).
- Inicialização e Serviços: acesso remoto (`sshd`, Tailscale, ZeroTier), bluetooth, contas e rtkit
  entram nos protegidos (desligar o ssh num PC sem tela tranca você fora).
- Cartão de VRAM das integradas mostra "0.4 / 0.5 GB" em vez de "/ 0 GB".
- README com capturas novas (tema atual), selos, galeria e `tools/capturas` para refazê-las
  (`NUMAPILOT_GUI_TAB`, `NUMAPILOT_GUI_SIZE`, `NUMAPILOT_PROC_STAT`, `NUMAPILOT_MEMINFO`).

## 0.5.1 — 2026-11-09

- Inicialização: **Detalhes** mostra de onde cada item vem (arquivo de inicialização e pacote dono,
  programa de verdade mesmo atrás de `node`/`python`/`sh -c`, origem: repositório, AUR, Flatpak,
  npm, pipx, cargo, comunidade, AppImage ou instalado à mão; tamanho, pasta e configurações; abrir
  pasta, ver na aba Programas, log) e **Desinstalar…** remove do jeito certo para a origem, parando
  o serviço e apagando a inicialização que ficaria para trás (opcionalmente as configurações e a
  pasta). Também `numapilot inicio origem|desinstalar ID`.

## 0.5.0 — 2026-11-09

- Aba **Inicialização** (e `numapilot inicio`): tudo que abre quando o sistema liga, como no
  Windows: programas do login (autostart do usuário e do sistema, restauração de sessão do KDE),
  serviços do usuário e do sistema, componentes da sessão; com o que está rodando e a memória.
  Ligar/desligar (reversível, só vale para o próximo login/boot), remover o que você criou e
  adicionar programa instalado, aberto agora, arquivo (AppImage, .sh, binário) ou comando, com
  espera opcional. Essenciais protegidos.
- Aba Programas: **Abrir ao iniciar** em cada programa.
- Aba **Serviços** (e `numapilot servicos`): todos os serviços do sistema e do usuário com estado,
  se abrem no boot e memória; filtros (rodando, parados, com falha); Iniciar, Parar, Reiniciar e
  Log (journal). Serviços do sistema pedem a senha; os essenciais não podem ser parados.
- **Disco e rede**: coluna Disco (leitura + escrita por programa) na aba Programas, com o total no
  cabeçalho; seção "Discos e rede" no Mapa com leitura, escrita e % ativo de cada disco e
  download/upload de cada placa de rede, com o último minuto.
- README reescrito: o que faz, PCs testados, instalar e atualizar, cada aba da interface com
  capturas novas, linha de comando, guias por hardware (servidor, IA, notebook, BC-250), jogos.
- BC-250: o botão do BC250 Control Center diz o que faz ("Gerenciar núcleos, CUs, frequências e
  voltagens" / "Instalar o BC250 Control Center").

## 0.4.0 — 2026-11-09

### BC-250 (com ideias do [BC250 Control Center](https://github.com/movacx/bc250-control-center), MIT)
- Telemetria embaralhada com 8 núcleos: a dica de correção agora depende do kernel em uso
  (tabela de 8 núcleos → ligar o "SMU Reporting Patch"; `cs_legacy_8core_metrics=1` → tirar,
  porque saiu do kernel; kernel comum → desligar o patch ou usar o linux-cachyos-bc250).
- CUs liberados pelo próprio kernel (`amdgpu.bc250_cc_write_mode=3`) aparecem no doctor,
  no autoteste e na aba Placas, com o aviso para não usar o desbloqueio ao vivo junto.
- 8 núcleos: diz se o desbloqueio fica ao desligar da tomada (biosmod ou EFI shim) ou não
  (bc250-core-unlock); detecta o bc250-efi-core-unlock.
- Clock da GPU acima de 3000 MHz é descartado na BC-250 (leitura da tabela errada).
- Mais programas da comunidade na aba Placas: bc250-core-unlock, bc250-efi-core-unlock,
  bc250-cu-live-manager, BC250-Telemetry e bc250-memory-temperature.

- Aba Placas da BC-250: resumo **agora** com núcleos e clock da CPU, CUs e clock da GPU. Os CUs
  vêm da leitura ao vivo do BC250 Control Center quando existe (antes era só o que o driver
  contou no boot, que não muda com o desbloqueio ao vivo).
- Programas da comunidade com **Instalar/Atualizar** na versão mais nova: BC250 Control Center
  em destaque (pacote `bc250-control-center-git`; o nome antigo na receita não existia no AUR),
  `umr-git`, e por git (com `git pull` para atualizar) o bc250-cu-live-manager, bc250-core-unlock,
  bc250-core-cu-unlock, bc250_smu_oc (pipx) e BC250-Telemetry.

### Correções
- O serviço gravava "modo da máquina: ... (0 erro(s))" como ERRO.

## 0.3.0 — 2026-11-09

Primeira versão com release no GitHub (a 0.2.0 ficou sem tag). Junta tudo desde a 0.2.0:

### Vários PCs
- Suporte além do servidor: NVIDIA (driver oficial, sensores, PRIME, limites, deslocamento de
  clock), notebook, Intel, AMD de qualquer geração e placas da comunidade (BC-250, BC-160).
- Painel com todos os PCs, Wake-on-LAN, perfis compartilhados, backup diário de todos no PC
  principal, versão de cada PC no painel e atualização pela interface.
- `numapilot self-update` mostra o commit instalado e avisa quando a pasta ou o GitHub têm
  versão mais nova; fluxo "Vários PCs no mesmo código" no README.

### Interface
- Visual novo: menu lateral, cartões de medida, tema claro e escuro.
- **Programas** em tabela, no estilo de um monitor do sistema: colunas ordenáveis (nome, PID, CPU,
  memória, GPU, memória da GPU, placa) com totais, seções Aplicativos e Sessão e sistema, linha
  que abre com os processos, busca, e botões **Encerrar** e **Forçar parada** protegidos (nunca
  o kernel, o sistema, outros usuários, a sessão gráfica nem o próprio numapilot).
- Mapa com o último minuto por thread e a temperatura do processador.
- Aba **Testes** com o roteiro desta máquina e relatório.

### Placas de vídeo
- Undervolt automático com teste de carga (NVIDIA, RDNA2+, Vega20/RDNA1).
- OverDrive sem editar o kernel (`numapilot boot overdrive on`).
- RDNA3/RDNA4 (RX 7000/9000): ventoinha pela curva do firmware; o doctor explica quando falta
  ligar o OverDrive.
- Upscaling por jogo (FSR 4, DLSS e XeSS pelo Proton, OptiScaler).
- Memória da GPU integrada para IA (`ttm.pages_limit`).

### Máquina
- Modos (Jogo, IA, Silencioso) automáticos por programa e por horário; energia em kWh e R$.
- CPU por socket, uncore, RAPL, sched_ext, ventoinhas da placa-mãe, mitigações e parâmetros do
  kernel.
- Saúde do hardware (SMART, ECC, RAS das MI50, bateria).
- Notebook: ryzenadj, Acer, NVIDIA só quando precisar (desliga sozinha), programas do login na
  integrada.
- Máquina virtual Windows com placa própria (VFIO).

### IA, laboratório e streaming
- Otimizador do servidor de IA (`gfx906-rocm llm tune`), computação por arquitetura (gfx906,
  ROCm oficial, CUDA, Vulkan).
- Avisos no celular (ntfy), `/metrics`, Sunshine pelo painel (codificação por software na BC-250).

### Diagnóstico e instalação
- `numapilot autoteste`, registros em arquivo (`numapilot log`) e `numapilot doctor` mais completo.
- Instalador confere e instala o que falta (pacman e AUR), backup e restauração.

### Testado em
- Notebook Acer Nitro 5 (Ryzen 7 6800H + RTX 3070 Ti), BC-250, PC CC150 + RX 9060 XT.
