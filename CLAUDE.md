# numapilot — contexto para o Claude

Leia antes de mexer. Resume o que já foi feito e combinado com o Lucas em sessões anteriores.

## O dono e como ele trabalha

- Lucas, Brasil, fala **português**: responda a ele em português do Brasil.
- **O projeto é em inglês** (decisão de 2026-11-12, para receber colaboração): código, textos no
  código, nomes de comandos e opções, mensagens, registros, comentários novos, commits, README e
  CHANGELOG em inglês. Só a interface (e as mensagens que ela mostra) tem os dois idiomas.
- Todos os PCs rodam **CachyOS** (Arch). Notebook com **KDE Plasma Wayland**.
- Gosta de "pode fazer todos": implementar, testar, commitar e dar push direto na `main`.
- Regras combinadas:
  - **Não desligar o segundo socket** do servidor (nunca sugerir isso).
  - Mudanças no **boot**, **undervolt** e **mitigações** só com a confirmação dele (pkexec/senha).
  - Manter o nome **numapilot**.
  - Commits terminam com:
    as linhas `Co-Authored-By:` e `Claude-Session:` que a sessão atual indicar (o modelo e o link
    mudam de sessão para sessão).
  - **GitHub e pastas locais sempre na mesma versão** (vários PCs testando ao mesmo tempo):
    `git pull --ff-only` antes de mexer; depois de mexer, CI completa, commit e push na hora
    (nunca deixar commit só local). Nada pode quebrar os outros PCs: todo recurso depende do
    hardware detectado. Se mudar o fluxo, atualize "Vários PCs no mesmo código" no README.
  - O `sudo` às vezes é bloqueado nos terminais do Claude Code ("sem novos privilégios"; no CC150
    falhou antes de reiniciar e funcionou depois). Se acontecer, o Lucas roda o `install.sh` e o
    `self-update run` no Konsole. O Bash do próprio Claude nunca tem sudo.
  - Versões: subir em `Cargo.toml` e `packaging/arch/release/PKGBUILD`, anotar no `CHANGELOG.md`,
    e a tag `vX.Y.Z` gera a release no GitHub (binários prontos). Última: **v0.5.7** (2026-11-17).

## O que é

App em Rust (edição 2024) que controla o hardware de todos os PCs dele:
- escolhe socket NUMA, núcleos, memória e GPU de cada programa (regras, perfis salvos, Steam);
- placas de vídeo: ventoinha, potência, clock/voltagem (OverDrive AMD, deslocamento NVML na
  NVIDIA), undervolt automático com teste de carga;
- máquina: modos (Jogo, IA, Silencioso), horários, energia/custo, saúde (SMART/ECC/bateria);
- notebook: ryzenadj, Acer (acer_wmi predator_v4 / Linuwu-Sense), NVIDIA híbrida (desliga a
  dGPU removendo do PCI quando nada usa; auto por tomada/bateria), programas do login na iGPU;
- servidor de IA (llama.cpp), laboratório (containers ROCm/Jupyter), Sunshine/Moonlight,
  outros PCs (painel web, Wake-on-LAN, cópia das configurações), avisos ntfy, VM com VFIO.

Partes:
- `numapilot daemon` (root, systemd `numapilot.service`) com socket de controle JSON;
- `numapilot-gui` (egui) com menu lateral, tema próprio (fonte Inter), ícone na bandeja
  (`--tray`), uma janela só;
- CLI `numapilot …` (veja `numapilot --help`); painel web `numapilot web`.

## Hardware dele

| PC | CPU | GPUs |
|---|---|---|
| Servidor | 2× Xeon E5-2696 v4 | 2× MI50 (gfx906) + RX 470 |
| Notebook Acer Nitro 5 AN515-46 | Ryzen 7 6800H | Radeon 680M + RTX 3070 Ti Laptop |
| **BC-250** | APU AMD (Cyan Skillfish), 6 núcleos de fábrica; a do Lucas tem 8 (biosmod) | GPU integrada da própria placa (24 CUs) |
| PC CC150 | CC150 | RX 9060 XT |
| PC | i7-5775C | BC-160 |

### BC-250 (próximo a testar)
- Kernel recente do CachyOS já funciona: **não precisa mais de kernel com patch**.
- Apps da comunidade aparecem na aba Placas (BC250 Control Center, umr,
  bc250-core-cu-unlock, bc250_smu_oc) — receitas em `recipes/default.toml` (id `bc250`).
- Sem codificador de vídeo: Sunshine com codificação por software (`no_video_encoder`).
- Memória da GPU (TTM `pages_limit` em modprobe.d) em Placas → memória da GPU integrada;
  llama.cpp com `GGML_VK_PREFER_HOST_MEMORY`.
- `boards::active_cores`/`active_cu` (journal `active_cu_number`) para conferir o desbloqueio ou, se a mensagem já saiu
  do buffer do kernel, a topologia do KFD (`simd_count / simd_per_cu`).
- Telemetria com 8 núcleos: o kernel linux-cachyos-bc250 escolhe a tabela de métricas da SMU
  pelo número de núcleos (`cs_eight_core_map`). O biosmod do Lucas libera 8 núcleos **sem** o
  "SMU Reporting Patch" (MeiMeiDXE v3), então temperatura e potência do hwmon/gpu_metrics vêm
  embaralhadas (0 °C, ~1470 W) e `pp_dpm_sclk` também. Certos: clock da GPU no hwmon
  `freq1_input`, MHz da CPU no `/proc/cpuinfo`, temperatura da CPU no `k10temp`.
  `boards::smu_table_mismatch` explica isso no autoteste e no doctor. O
  cyan-skillfish-governor-smu com `fix-metrics = true` monta um gpu_metrics corrigido por cima
  (é dele que o MangoHud lê durante os testes); hoje o serviço está desligado.
  Diagnóstico do kernel: `amdgpu.cs_full_telemetry=1` faz o `pp_dpm_socclk` mostrar a tabela.
- Ideias do BC250 Control Center (movacx, MIT; clone de referência é só para ler):
  `boards::metrics_decode`/`mismatch_fix` (dica de correção por kernel: `cs_eight_core_map` →
  ligar o SMU Reporting Patch; `cs_legacy_8core_metrics=Y` → tirar, saiu do kernel em
  2026-09-17; kernel comum → desligar o patch), `cu_unlock_by_kernel` (`bc250_cc_write_mode=3`:
  40 CUs do boot, não usar umr junto), `efi_core_unlock`/`core_unlock_persistence`
  (biosmod/EFI ficam; bc250-core-unlock some ao desligar da tomada), clock > 3000 MHz é lixo
  (só na 1002:13fe), `pp_dpm_fclk` da Cyan Skillfish repete o clock da memória.
- CUs ao vivo: `boards::cu_now()` lê o que o BC250 Control Center deixa
  (`/run/bc250-control-center/cu-live-state.json` do helper do Decky, conferindo boot_id e
  máscaras; `~/.local/share/bc250-control-center/Data/cu_dashboard_live.txt` se for deste boot,
  "CUs active & routed : N / 40"), senão kernel (`bc250_cc_write_mode=3`), senão o boot.
  Driver e RADV (`num_cu`) contam só as do boot; as liberadas ao vivo pelo SPI (umr) só aparecem
  lendo registradores como root. Se o `bc250-cu-live-manager.service` falhou no boot, a tabela
  salva não foi aplicada: `boards::failed_board_services` avisa no first_setup.
- Telemetria certa sem a tabela da SMU (2026-11-14): hwmon **`bc250_vrm`** (driver do kernel
  linux-cachyos-bc250, I2C nos reguladores): potência/tensão/corrente de CPU e GPU, entrada 12 V
  e temperatura dos VRMs (`boards::vrm`). `gpuctl::read_sensors` usa ele na 1002:13fe
  (`power_w`, `voltage_mv`, `vrm_c`) e o `k10temp` como temperatura do chip (`apu_c`, CPU e GPU
  no mesmo die; vale para a curva de ventoinha). Sem RAPL, o `PowerMeter` usa a potência da CPU
  do VRM. A ventoinha é do chip nct6687 da placa (Máquina → ventoinhas), não da GPU.
- Serviços do BC250 Control Center que mexem no mesmo que o numapilot: `bc250-memory-setup`
  (TTM a cada boot, estado em `/var/lib/bc250-control-center/system-setup/memory.json`,
  `bootcfg::cc_ttm`: a interface manda mudar por ele), `bc250-smu-oc` (overclock da CPU),
  `bc250-cu-live-manager` (CUs). Control Center instalado por usuário (`~/.local/share/...`):
  "Atualizar" roda o `scripts/install-local.sh` dele (`local_dir`/`local_setup` na receita), não
  o AUR (que criava uma segunda cópia em /usr).
- **Não ler `pp_od_clk_voltage` na BC-250** (`gpuctl::od_by_governor`): cada leitura gera 4
  linhas "Unsupported clock type" no kernel; a interface lia por quadro (~4500/min, 59 mil num
  boot) e o journal rotacionava, apagando o log dos serviços. Clock/tensão da GPU ficam com o
  governor da comunidade; undervolt e "economia quando parada" ficam desligados nela.
- Ventoinha da BC-250: canal do nct6687 (no Lucas, `pwm2` "Pump Fan"). Testado pelo socket:
  fixa e curva (cpu/gpu/max) mudam o PWM e o RPM. A aba Placas mostra a ventoinha e leva para
  Máquina → Ventoinhas; fonte "vrm" (`fans::vrm_temp`). Canais a 0 RPM ficam num grupo dobrado.
- `umr` do AUR quebra quando o LLVM do sistema muda (22 → 23 em 2026-11): o
  bc250-cu-live-manager falha e as CUs voltam a 24. `boards::broken_apps` (ldd) avisa no
  first_setup, doctor, autoteste e bandeja, com "Recompilar" (+ reaplica as CUs).
  CUs ao vivo também vêm do `/etc/bc250-cu-live-manager.conf` quando o serviço deu certo no boot.
- Programas da comunidade (`CommunityApp` na receita): `aur` (preferir `-git`), ou `git` +
  `scripts` (links em ~/.local/bin) + `aur_deps` + `setup`; clones em
  ~/.local/share/numapilot/comunidade/<nome>; Instalar/Atualizar roda no terminal. O AUR do
  Control Center é `bc250-control-center-git` (não existe sem -git).
- Sem cpufreq: frequência da CPU só para leitura (`cpuctl::cur_mhz_all`); controle pelo bc250_smu_oc.
- Ler `pp_od_clk_voltage` (e `pp_dpm_dcefclk`/`pp_dpm_pcie`) faz o amdgpu escrever
  "Unsupported clock type" no log do kernel. É do driver e inofensivo; evite ler em loop.

## Código

- `src/` (biblioteca + `main.rs` da CLI), `src/bin/numapilot-gui/` (interface),
  `recipes/`, `packaging/` (serviços, PKGBUILD), `install.sh`, `docs/TESTES.md`.
- Testes com sysfs falso: `NUMAPILOT_SYSFS=/caminho` e `util::test_sysfs_lock`.
- CI (GitHub Actions) roda:
  ```
  cargo fmt --check
  cargo test --release
  cargo clippy --release --all-targets -- -D clippy::correctness -D clippy::suspicious
  cargo build --release --no-default-features
  shellcheck -S warning install.sh uninstall.sh packaging/gfx906-rocm container/lab-start packaging/arch/numapilot.install
  ```
  Rode tudo antes de commitar. Depois de `cargo fmt`, scripts de patch que procuram texto exato
  costumam falhar: releia o arquivo formatado.

## Inicialização (aba e `numapilot inicio`)

`src/startup.rs`: autostart XDG (sistema + usuário; desligar o do sistema = cópia com
`Hidden=true` em ~/.config/autostart, ligar apaga a cópia se só diferir no Hidden), restauração de
sessão do KDE (`ksmserverrc` loginMode), `systemctl [--user] enable|disable` (usuário: `mask`
quando o pacote liga para todos em /etc/systemd/user; sistema: `pkexec`, rodado pela interface),
unidades desligadas lembradas em ~/.config/numapilot/startup-off.txt. Atalhos criados levam
`X-Numapilot-Startup=true`. Lista de essenciais protegidos em `ESSENTIAL_XDG`/`ESSENTIAL_UNITS`.
O "control center" que o Lucas usa de referência para Programas/Inicialização é o **Mission
Center** (gerenciador de tarefas), não o BC250 Control Center.

`src/origin.rs`: `trace` (de onde vem: `program_of` acha o programa de verdade atrás de
env/`sh -c`/intérpretes/flatpak; `source_from_path` npm/pipx/flatpak/comunidade; senão
`pacman -Qqo` e `-Qqm` para AUR; senão cargo ou arquivo solto; `app_dir_of` = pasta própria em
~) e `uninstall_plan` (script para o terminal; bloqueia pacotes essenciais e o próprio numapilot).

## Serviços, disco e rede

`src/services.rs` (aba Serviços, `numapilot servicos`): list-units + list-unit-files + show
(MainPID, MemoryCurrent); sistema por `pkexec`, usuário direto; essenciais via
`startup::essential_unit`; unidades `app-…@hash.service` são programas (ficam fora). `src/iostat.rs`:
/proc/diskstats (só discos inteiros), /sys/class/net (física ou VPN), /proc/PID/io por processo;
`IoMonitor` mede as velocidades entre duas leituras.

## Buscar app, teste de placa, permissões (2026-11-10)

- `src/apps.rs`: programas instalados (atalhos .desktop, Flatpak, Snap, Steam, AppImage) com a
  mesma chave de perfil que o serviço calcula com o programa rodando; janela "Buscar app
  instalado…" na aba Programas (`Finder` em `programs.rs`).
- `src/gputest.rs`: `numapilot gpu-test`, nome Vulkan para o filtro do DXVK/VKD3D
  (`decide::dx_filter`, cache em ~/.cache/numapilot/vk-names.json por um dia).
  `procctl::wrong_gpu` avisa placa diferente da escolhida (fdinfo; a NVIDIA não publica).
- `Request::Adopt` leva `key`/`label` do programa que o `run` vai abrir.
- `daemon.admin_groups` + `util::groups_of_uid`: quem pode mudar a máquina inteira.
- `boards::first_setup` (BC-250) e abas escondíveis (`~/.config/numapilot/gui-tabs`).
- `src/turbo.rs`: tabela de turbo dos MSRs 0x1AD–0x1AF (serviço publica em hw.json) e núcleos
  acordados pelo cpuidle; linha "Turbo" no Mapa e `numapilot turbo`. O 2696 v4 não tem o "turbo
  unlock" dos v3; o ganho vem de deixar poucos núcleos acordados por socket com C6 ligado.
- 2026-11-11: `Profile.turbo_max` (reserva o socket inteiro via `cgroup::set_reserved`, só com
  >1 nó e nunca todos os sockets) e `cpu_max_pct` (`CPUQuota`); `src/proghist.rs` (histórico por
  programa em /run/numapilot/prog-history.json); `health::parse_dimms`/`mem_notes` (dmidecode
  pelo serviço); `profiles::export_text`/`import_text`; `i18n.rs` na interface (ver abaixo). O Otimizador de FPS já compara escolhas (não foi feito outro).
- `src/corebench.rs` (`numapilot bench-cores`, botão no Mapa): corrida de ponteiros num bloco
  entre o L2 e o L3, preso em cada núcleo; guarda em `state_dir()/core-rank.json` pelo
  `Request::SetCoreRank`. `topo::select_cpus` ordena os núcleos por essa medição
  (`corebench::sort_units`, degraus de 1%) só se `useful` (diferença ≥ 2% e ≥ 3× o ruído) e se o
  modelo da CPU for o mesmo. **No Nitro 5 (2026-11-15) não há ordem real**: a primeira medição
  deu 8,8% (ruído 1,2%) e foi aceita, mas com o PC ocioso o núcleo mais rápido mudava a cada
  medição (1,7–4,4%), e o ranking de núcleos preferidos do firmware (`amd_pstate_prefcore_ranking`:
  CPUs 6=10 > 4 > 0 > 8 > 12 > 2 > 14) pôs o "mais rápido" (14) por último. Desde então a ordem
  só vale se as rodadas pares e ímpares concordarem (Spearman ≥ 0,8, `agreement`), 8 rodadas, e
  medições automáticas não rodam com o PC ocupado. Perfil que escolhe parte dos núcleos sem
  medição: o `run` e a interface medem antes, o serviço mede quando ocioso e reaplica
  (`RANK_WANTED`). Mecânica conferida: com uma ordem útil, 2 núcleos → os 2 mais rápidos, com HT
  → núcleos inteiros, o limite de CPU também segue. Diferenças < 1% são empate. A lista manual
  (`cpu_list`) não usa a ordem. No servidor (anel do Broadwell, 22 núcleos) ainda não medido.
  Ordem usada (`corebench::order_source`): a medição de cache se for confiável (`useful`); senão
  os núcleos preferidos do firmware (`amd_pstate_prefcore_ranking`, ou `acpi_cppc/highest_perf`
  se variar entre núcleos); senão a do sistema. No Nitro 5 vale a do firmware (conferido: 2
  núcleos → 6,10; 4 → 0,4,6,10; HT → 6,7,10,11). Pedido do Lucas: latência de cache quando
  tiver impacto medido (Xeon), firmware quando não tiver.
- Idiomas (2026-11-12): **inglês é o idioma base do código**; o português é a tradução.
  Texto fixo mostrado ao usuário = `crate::i18n::tr("text")` com o par em `src/i18n_pt.rs`
  (biblioteca/CLI) ou `src/bin/numapilot-gui/i18n_pt.rs` (interface, que cai na da biblioteca);
  texto com valores = `crate::tf!("text {}", "texto {}", v)` (`numapilot::` no `main.rs`); duas
  formas em português para o mesmo inglês = `t2!("on", "ligada")`; tabelas `const` guardam o
  inglês e o `tr()` fica onde o texto é mostrado. Os testes `every_tr_has_a_pair` falham se um
  `tr()` ficar sem par. Registros (`nlog!`), relatórios, `--help` e conteúdo de arquivos gravados
  são só em inglês. Idioma em uso (`src/i18n.rs`): `NUMAPILOT_LANG`, `~/.config/numapilot/lang`,
  para o root `/var/lib/numapilot/lang`, senão português só se o sistema for `pt_*`; cada pedido
  ao serviço leva `"lang"`. Nos testes é sempre inglês. Painel web: `PAGE_PT` em `src/web.rs`.
- Comandos em inglês: `startup`, `services`, `selftest`, `--user/--running/--failed/--delay/…`,
  `machine mode game|ai|quiet|normal`, `hybrid auto|hybrid|integrated|leave`. Os nomes antigos
  (`inicio`, `servicos`, `autoteste`, `jogo`…) continuam aceitos como apelidos. **Os valores
  guardados não mudaram** (modo `jogo`/`ia`/`silencioso`, `hibrido`/`integrada`, perfil `padrao`,
  estados `falhou`/`pulei` da aba Testes, pastas do backup): mudar exige migração dos arquivos
  dos PCs. Níveis do registro agora `ERROR`/`WARN` (a leitura aceita os antigos).
- `README.md` é o inglês (principal) e `README.pt-BR.md` a tradução: mudou um, mude o outro.
- Já em inglês: comentários e nomes internos do código, `install.sh`, `uninstall.sh`.
- `recipes/default.toml` é só em inglês (versão 2); o português dos textos (`about`, `notes`)
  fica em `src/i18n_pt.rs` e é aplicado por `i18n::tr_text()` onde o texto é mostrado. O teste
  `recipe_texts_have_a_portuguese_pair` falha se um texto novo ficar sem par.
- Falta passar para inglês: `packaging/gfx906-rocm` (~1500 linhas), comentários de `packaging/numapilot.toml` e `gfx906-rocm.conf`,
  `container/lab-start`, `docs/TESTES.md`, `tools/capturas/README.md`, entradas antigas do
  CHANGELOG, este arquivo; e testar tudo isso em hardware real.

## Apresentação em outra placa (2026-11-20)

`Profile.present` (`profiles::Present`) e `src/present.rs`: modelo de
upscaling (`UPSCALERS`: fsr1/nis prontos pelo gamescope; anime4k/srvgg planejados, caem no FSR 1),
geração de quadros (`multiplier` 1–4, `ENGINES`: lsfg pronto; blockmatch/rife planejados), FPS
mínimo de base e `measure`. A placa de saída é a do gamescope (`Gamescope.output_gpu` /
`run --out-gpu`, PR #2): `decide::gamescope_split` dá ao gamescope só a placa de saída (separa as
duas MI50 pelo PCI) e devolve ao jogo as variáveis da placa dele (`env` depois do `--`);
`present::plan` só acrescenta o filtro (`-F`), as variáveis do jogo (lsfg, MangoHud) e a medição. Medição: MangoHud só no jogo (log em `~/.local/state/numapilot/present-logs/`), resultado
em `present.json`; o mostrado é **calculado** (real × multiplicador enquanto a base fica acima do
mínimo), não contado na saída. Plano completo no PDF do projeto (etapas M0–M4). Falta testar no
servidor: a medição real × mostrado numa sessão de jogo. Testado (2026-11-20): o lsfg-vk oficial 2.0 não
tem escolha de placa e ignora `LSFGVK_GPU`; a geração 2× roda na placa do jogo, não na de saída.
Geração na placa de saída (2026-11-20, provado no servidor): o lsfg-vk carregado **só no gamescope**
(perfil com `active_in = ["gamescope","gamescope-wl"]`) e o gamescope com `--backend sdl` geram na
placa do gamescope; no modo aninhado padrão a camada não dispara. Motor `lsfg-out` do bloco
(`present::lsfg_out_config`/`write_lsfg_out`): config própria em `state_dir()/lsfg-vk-output.toml`
via `LSFGVK_CONFIG` no ambiente do gamescope (o `[global]` do usuário fica, os perfis dele saem para
o jogo não gerar também); nunca escrever o `conf.toml` do usuário. O fork dual GPU, o `fg_gpu` do
perfil e o `LSFGVK_GPU` saíram (`run --fg-gpu` só avisa).
Upscaler próprio (docs/PRESENT-CHAIN.md): ncnn (SRVGG 323 ms, RIFE 598 ms) não serve; Anime4K como
compute Vulkan (`src/vkbench.rs` roda shaders do mpv; `present-bench --compute`) mediu 0,5–3 ms na MI50.
Camada `layer/` (VK_LAYER_NUMAPILOT_upscale, implícita, `ENABLE_NUMAPILOT_UPSCALE=1`, só age no
gamescope): o gamescope compõe sempre no tamanho da janela (`g_nOutputWidth`) e ignora a
superfície, então mentir o tamanho mostrou só um quarto da imagem no servidor. Agora o gamescope
roda com `-S integer -F nearest`, a camada lê um texel por bloco (`chain::Src`, tamanho do jogo em
`NUMAPILOT_UPSCALE_SRC`) e escreve o 2× centralizado; janela menor que 2× o jogo passa direto. Testada no lavapipe com
vkcube (Xvfb). No servidor o gamescope `--backend sdl` usa swapchain A2B10G10R10 (10 bits): a camada
trata 8 e 10 bits. Modelos de luma (ArtCNN C4F16/C4F32, MIT, no lugar do ESPCN; FSRCNNX, LGPL,
baixado em ~/.local/share/numapilot/shaders no primeiro uso, nunca no repositório): o vkbench faz
RGB → luma antes e soma a luma ampliada sobre o RGB bilinear depois; passes `//!COMPUTE` do mpv
(memória compartilhada) viram buffers, e o naga inverte arrays multidimensionais e lê `barrier()`
como de subgrupo (`flatten_shared`, `workgroup_barriers`). Falta: medir na MI50, testar junto do
`lsfg-out`. Passada temporal (M3, `shaders/temporal/temporal.glsl`, `Present.temporal` →
`NUMAPILOT_UPSCALE_TEMPORAL=1`): blocos 8×8 no quadro do jogo contra o anterior, reprojeta a saída
anterior, limita pela vizinhança 3×3 e mistura 80/20; histórico (`PREV_IN`/`PREV_OUT`) guardado
pelo runner e pela camada. Sem jitter do jogo, estabiliza mais do que ganha detalhe.
Geração de quadros própria (motor `blockmatch`, `NUMAPILOT_UPSCALE_FG=2`, `shaders/temporal/fg.glsl`):
quadro do meio pelo MOTION do temporal; a camada pede uma imagem a mais ao swapchain, adquire
ela a cada present (sem esperar), escreve o gerado e o real no mesmo submit e apresenta o gerado
antes; só com FIFO. Travamentos da BC-250 (2026-11-21): eram pools de comando compartilhados
entre swapchains e `vkDeviceWaitIdle` no destroy (corrigido no #19); cada Chain tem os seus pools.
Geração testada (2026-11-21): servidor em tela cheia no monitor da MI50 de saída (DP-8, 120 Hz):
30 → 60 e 60 → 120 fps, 1 gerado por real; ~1 em 3 capturas com defeitos em bordas rápidas. Os
"~20 fps" de antes eram os monitores em DPMS desligado (acordar com `kscreen-doctor --dpms on`
antes de medir). O gamescope envia e apresenta numa fila só de compute. Na BC-250 (40 CUs
liberadas ao vivo) a geração dá page fault em ~1 de 3 rodadas, sempre nos primeiros quadros
gerados (reset da fila de compute, o gamescope perde o device, o desktop fica). Não é a DCC
(`nodcc` também falhou), nem as escritas da camada na imagem extra (`noimage` falhou), nem
corrida entre threads (tudo na thread gamescope-xwm); sem geração e com 6 imagens ficou limpo.
Chaves de diagnóstico em `NUMAPILOT_UPSCALE_FG_TEST`. Causa ainda aberta.

## FSR 4 INT8 na placa do jogo (2026-11-21)

`src/fsr4.rs` + modo `fsr4-int8` do `Upscale` (Ajustar → Upscaling; `numapilot upscale check` e
`upscale install APPID --fsr4 [--gpu G] [--watermark]`). O método dos vídeos da RX 580 é OptiScaler
no Proton + DLL do FSR 4 INT8; **DLSS de verdade não roda em AMD** (os vídeos chamam de DLSS a
entrada que o OptiScaler converte). Rota por placa (`fsr4::route`, por gfx ou PCI id): wave32
(BC-250 gfx1013, RDNA 2/3) → DLL do BC250 FSR4 RC12; wave64 com INT8 (gfx906 MI50) → DLL `bc250`
do fsr4-gfx803 r6 (o fork sem o pedido de wave32); gfx900 → a mesma, lenta; Polaris → precisa de
RADV com patch (não instalamos); RDNA 4 → modo `fsr4` do Proton. OptiScaler 10.0.0-pre1 (nightly),
OptiPatcher e `nvngx_dlss.dll` da NVIDIA. **Nenhuma DLL no repositório** (decisão do Lucas,
2026-11-21: repo público MIT; DLL INT8 vazada da AMD, licença da NVIDIA, OptiScaler GPL-3): tudo é
baixado no PC com URL e **SHA-256 fixos** (`fsr4::Pinned`) e recusado se não bater. Servidor
conferido (2026-11-21): as duas MI50 têm `integerDotProduct4x8BitPackedSignedAccelerated` e
`shaderFloat16`, subgroup só 64; RX 570 nenhum dos dois. Com duas placas: FSR 4 no jogo + `lsfg-out`
na outra (bloco de apresentação com upscaling "none").
Testado na BC-250 (2026-11-21, 40 CUs, Proton 11.0, Spider-Man Remastered com DLSS Qualidade e
`WINEDLLOVERRIDES="dxgi=n,b" %command%`): marca d'água "FSR4-I8 UPSCALE 4.1.1R12" aparece (FSR 4
INT8 rodando), GPU sem reset. Mas a VRAM da BIOS é só 512 MB (desktop usa ~316 MB): 71 erros
`amdgpu_dm_plane_helper_prepare_fb: Failed to pin framebuffer: -ENOMEM` em 3,5 min. Falta: FPS
(MangoHud), testar sem DLSS para ver se o ENOMEM é do FSR 4, UMA maior na BIOS.
MI50 (2026-11-25, Spider-Man 1440p, gfx906-0, FSR do jogo em Qualidade → FSR 4 wave64): roda, mas
a imagem ficou pior que a nativa (17 FPS no menu de pausa); sem FSR 4 ficou melhor. Tirado das
sugestões do gfx906 na receita. `run` pela Steam agora conta como gráfico (`SteamAppId`), senão a
Wave64 agora usa por padrão a DLL **4.0.2 INT8** da AMD (pedido do Lucas, 2026-11-25; cópia fixa no
commit 6aa7c33 do fsr4-gfx803, SHA-256 fixo); `--dll vega|fork` escolhe as 4.1.1 recompiladas
(`fsr4::WAVE64_DLLS`, cache em `dll-wave64-<id>`). Falta comparar 4.0.2 × IGTI no servidor.
MI50 nomeada só recebia ROCm.
Controle no Spider-Man do servidor (2026-11-25): direcional/gatilhos falhando eram o DualSense com
5% de bateria no Bluetooth ("DualSense input CRC's check failed") mais o xalia do Proton lendo o
controle; o `run` agora põe `PROTON_USE_XALIA=0` em jogos da Steam e o `steam command` gera
`--gpu X --use graphics`. Com isso e o IGTI do jogo, o Lucas achou "muito bom".

## Capturas do README

`tools/capturas` (veja o README de lá): sysfs de mentira do servidor, notebook e BC-250, carga por
thread falsa (`NUMAPILOT_PROC_STAT`), memória (`NUMAPILOT_MEMINFO`), janela no tamanho certo
(`NUMAPILOT_GUI_SIZE`) e na aba certa (`NUMAPILOT_GUI_TAB`), cliques pelo `entrada.py` (python-xlib)
numa tela Xvfb. Programas, Inicialização, Serviços e Mapa são capturas reais do CC150.

## Testar e registros

- `numapilot selftest` — confere tudo nesta máquina sem mudar nada e grava
  `~/numapilot-autoteste-MAQUINA-DATA.txt` (sai com código 2 se algo falhou).
- Aba **Testes** na interface: roteiro à mão só com os itens desta máquina; "Gerar relatório"
  junta respostas + autoteste + registros em `~/numapilot-testes-MAQUINA-DATA.txt`.
- `numapilot log` — final dos registros: serviço em `/var/log/numapilot/daemon.log`,
  interface/bandeja/CLI em `~/.local/state/numapilot/*.log`.
- `numapilot doctor` — diagnóstico com sugestões.
- Roteiro completo por máquina: `docs/TESTES.md`.

## Instalar

- Do código: `./install.sh` (compila; precisa de `rust`/`cargo`; pede a senha; instala em
  `/usr/local/bin`, serviço e atalho; oferece pacotes opcionais e AUR via paru/yay).
- Pacote pronto: tar com o código + `target/release/{numapilot,numapilot-gui}` + arquivo
  `.prebuilt` → `./install.sh --yes` só copia.
- Atualizar: `numapilot self-update` (repositório git) ou reinstalar.

## Estado (2026-11-08)

Notebook testado à mão e pelo autoteste (51 testes: 45 OK). Corrigido depois:
NVML segurando a NVIDIA, limite de potência da NVIDIA travado no notebook, scope do systemd ao
reabrir programa, erros de EPB nos AMD, memória por programa na lista, Broken pipe na CLI.

Em aberto no notebook:
- **Acer** (causa achada em 2026-11-15): DKMS ok e `linuwu_sense` carregado, mas com todos os
  parâmetros padrão: não reconhece o AN515-46 e não cria `nitro_sense` nem perfil de energia
  (`acer_wmi` está na blacklist). Falta testar `options linuwu_sense nitro_v4=1` (pede sudo).
  O `damx-daemon` (DAMX) também está ligado e recarrega o módulo ~20× no boot. O doctor
  (`acer::findings`) mostra os dois.
- **Saúde**: discos não aparecem (só a bateria). O relatório agora grava o motivo de cada disco.
- Telas USB-C/Thunderbolt sem imagem após desligamento forçado: `ucsi_acpi: PPM init failed`
  (firmware; resolver com desligar, tirar o carregador e segurar o power 30–60 s).
- NVIDIA não dorme (2026-11-15): `runtime_suspended_time=0` em 26 h, na tomada. O
  `nvidia-smi` mostra o `kwin_wayland` com contexto C+G e 11 MiB na NVIDIA, e o Xwayland segura o
  `card1`. O KWin tem `cap_sys_nice`, então o `/proc/PID/fd` dele só é visível como root: por isso
  o serviço publica `awake_by` no `dgpu.json`. Tirar o polling da NVML (deslocamento de clock)
  por 60 s não fez a placa dormir. Falta testar `KWIN_DRM_DEVICES=/dev/dri/card2` (perde o HDMI,
  que é ligado na NVIDIA) e ver se o `nvidia-powerd` (root) também aparece em `awake_by`.
  Testado depois: `awake_by` = Xwayland, kwin_wayland, nvidia-powerd. Sem o Dynamic Boost (90 s) e
  com o serviço do numapilot parado (60 s) a placa **continuou acordada**: quem impede é o KWin, que
  abre `/dev/nvidia0`, `nvidiactl`, `nvidia-modeset`, `renderD128` e `card1` (não só a saída).
  **Tirar do PCI com o KDE aberto não funciona** (teste de 2026-11-15): `nvidia-drm` sai, mas o
  NVRM fica em `os_delay` ("remove device with non-zero usage count") esperando o KWin, o remove
  trava, o rescan fica preso atrás dele e a placa só volta reiniciando. O `sleep_only` do serviço
  está certo: nunca remover com o KWin segurando. Única saída: o KWin iniciar sem a NVIDIA
  (`KWIN_DRM_DEVICES` no login): botão Máquina → Notebook → "Desktop só na integrada"
  (`hybrid::desktop_script`, em `~/.config/plasma-workspace/env`; o Plasma 6.8 lê e passa ao
  systemd). Só `KWIN_DRM_DEVICES` não basta: o EGL do KWin ainda carrega a NVIDIA; o script põe
  também o EGL do Mesa. No primeiro teste o modo "Desligada" removeu a placa no meio da troca de
  sessão e travou de novo: agora o serviço só remove com um KWin restrito rodando
  (`hybrid::desktop_lets_go`, lê o environ dele) e a placa livre por 6 s, numa thread própria.
  Testado depois (sessão restrita, 03:15): KWin e Xwayland **largaram** a placa (`awake_by` só
  nvidia-powerd), mas ela **continua `active`** mesmo com Dynamic Boost, deslocamento NVML e o
  serviço do numapilot parados, e `awake_by` vazio. Suspeito: o fbdev do `nvidia-drm`
  ("[drm] Cannot find any crtc or sizes" no boot), cliente dentro do kernel. Testar seria
  `numapilot boot cmdline add nvidia_drm.fbdev=0` + reiniciar; o Lucas preferiu **não mexer no
  boot** por enquanto. A NVIDIA parada gasta ~10 W (P8). Driver 620.70.06 (módulos abertos).

Controle por programa (Nitro 5, 2026-11-15), medido com uma carga em C (afinidade, threads por
núcleo, cgroup, nice): núcleos/lista/sem HT, "o programa vê", prioridade e limite de memória
(o excesso vai para o swap, não mata) funcionam; perfil aplicado em < 0,3 s a programa aberto
direto. Corrigido e validado: exclusivo/turbo/modo da máquina não valiam pelo `run` nem em perfil
sem escolha de núcleos (`Tracked::profile_in_effect`); limite de CPU com sched_ext vira núcleos
físicos (`Profile::cores_for`/`smt_for`); exclusivo reserva núcleos inteiros
(`topo::whole_cores`); "limitar a N" começa sem HT na interface. O `ananicy-cpp` roda no
notebook e mexe no nice de partida (tudo do app nasce com -8: "alta" = -5 fica abaixo).

BC-250 (2026-11-08): instalada pelo `install.sh`, autoteste com 44 testes: 40 OK, 3 avisos,
0 falhas (antes das correções: falha do Mesa por causa da época `3:` no `pacman -Q`, sensores
com lixo, CUs não achadas). Avisos que sobram: sem cpufreq, sensores do firmware, e o doctor
pedindo o governor de clock da GPU (cyan-skillfish-governor) que não está rodando.
Falta: roteiro à mão da aba Testes na BC-250.

PC CC150 (2026-11-09): Intel CC150 (1 socket, 16 threads) + RX 9060 XT (gfx1200, RDNA4), repo em
`~/Projetos/numapilot`. Autoteste: 40 testes, 38 OK, 1 aviso, 0 falhas (3 discos com SMART ok).
O aviso era a ventoinha: no RDNA3/RDNA4 o `pwm1` é só leitura e o controle é a curva do firmware
(`gpu_od/fan_ctrl`), que só existe com o OverDrive ligado. O doctor agora explica isso e conta a
curva do firmware como controlável. `self-update` agora compara o commit gravado no binário
(`build.rs` → `NUMAPILOT_COMMIT`) com a pasta, além da pasta com o GitHub.
OverDrive ligado (`ppfeaturemask` 0xfff7ffff, kernel 7.3.3): doctor com 0 avisos, ventoinha pela
curva do firmware, clock/voltagem até -200 mV. Autoteste logo depois do boot agora dá "pulou" (não
aviso) na coleta de saúde se o serviço iniciou há menos de 3 min.
Falta: testar à mão ventoinha (curva), clock/voltagem e undervolt automático; aba Testes.

Servidor (2026-11-14): 2× E5-2696 v4, 2× MI50 + RX 470, kernel 7.3.4, schedutil + scx_lavd.
Doctor 0 falhas, autoteste 36 OK. Testado de verdade: `run` (cpu/mem/gpu, as duas MI50 se
distinguem pelo barramento PCI), `pin`, modo automático, modos da máquina, perfis (cgroup,
núcleos sem HT, limite de memória), ventoinha/curva/limite de potência da RX 470 pelo socket,
painel web, backup, inicialização, `gfx906-rocm lab test` (PyTorch nas duas MI50). Achados:
- **schedutil + scx_lavd rende a metade** em carga pesada com afinidade ampla (≈200 contra ≈400
  M iter/s por thread); com `performance` (modos Jogo/IA) roda cheio. Por isso o turbo máximo
  passou a pôr o socket em `performance` enquanto o programa roda (`turbo_governor`).
- `cpu.max` (limite de processador do perfil) **não é aplicado com sched_ext** (LAVD).
- O `time` do cpuidle só soma quando o núcleo acorda: `turbo::deep_time` guarda também o
  idle do /proc/stat (chaves a partir de `IDLE_KEY`) para o núcleo que dormiu o intervalo todo.
- A medição de CPU por processo usa o intervalo de cada processo (`last_at`); o `dt` do laço
  oscilava ±17% e atrasava a expansão para 2 sockets (47 s em vez de 10).
- RX 470 (Polaris): depois de usar ventoinha fixa/curva, voltar ao automático não devolve o
  modo parado (fica ~2150 rpm) até reiniciar; MI50 sem tabela de OverDrive mesmo com ele ligado.
- A regra `jogos` manda os jogos para a RX 470 (placa "de vídeo"), não para as MI50, que são
  bem mais fortes e também têm monitor: para jogar nelas, `--gpu gfx906-0 --use graphics`.
- O `sudo` bloqueia a conta por 10 min depois de 3 senhas erradas (faillock); o KDE dele tem os
  layouts `us(intl)` e `br` (Meta+Alt+K alterna), o que já causou senha "errada".
Falta no servidor: aba por aba na interface, undervolt, Steam com `numapilot steam enable`,
medir um jogo (Skyrim instalado) com o perfil Jogo e com o turbo máximo.

Aba Programas (2026-11-09) virou tabela no estilo do Mission Center (`programs.rs` na interface):
colunas ordenáveis com totais, seções Aplicativos / Sessão e sistema, linha que abre com os
processos e os controles, Encerrar/Forçar parada. Proteção e GPU por processo (fdinfo do DRM;
o amdgpu repete a VRAM em `drm-memory-vram` e `drm-resident-vram`, usar só uma) em
`src/procctl.rs`. Só encerra processos do próprio usuário fora de `session.slice`/`system.slice`
e fora da lista de protegidos (KDE, GNOME, áudio, portais, numapilot).

Próximo: aba Testes na BC-250 e no CC150, depois os outros PCs.
