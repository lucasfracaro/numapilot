# numapilot — contexto para o Claude

Leia antes de mexer. Resume o que já foi feito e combinado com o Lucas em sessões anteriores.

## O dono e como ele trabalha

- Lucas, Brasil, fala **português**: respostas, textos da interface, mensagens de erro,
  comentários de código e commits em português do Brasil.
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
    e a tag `vX.Y.Z` gera a release no GitHub (binários prontos). Última: **v0.5.1** (2026-11-09).

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
  modelo da CPU for o mesmo. Ainda não medido em hardware real (no servidor a diferença esperada
  é de poucos %).
- Inglês (2026-11-12): interface, serviço e CLI. Texto fixo = `crate::i18n::tr("pt")` com o par
  na tabela (`src/i18n_en.rs` para biblioteca/CLI, `src/bin/numapilot-gui/i18n_en.rs` para a
  interface, que cai na da biblioteca); texto com valores = `crate::tf!("pt {}", "en {}", v)`
  (`numapilot::` no `main.rs`); tabelas `const` guardam o português e o `tr()` fica onde o texto
  é mostrado. Os testes `every_tr_has_a_pair` falham se um `tr()` ficar sem par. Texto novo:
  sempre nos dois idiomas. Idioma (`src/i18n.rs`): `NUMAPILOT_LANG`, `~/.config/numapilot/lang`,
  para o root `/var/lib/numapilot/lang`, senão o do sistema; cada pedido ao serviço leva `"lang"`
  e o serviço passa a responder nele. Nos testes é sempre português. Não traduzir: palavras
  usadas como chave/comparação (modos `jogo`/`ia`, `nenhum`, `padrao`, subcomandos), conteúdo de
  arquivos gravados, `nlog!`, e os estados `AVISO`/`FALHOU`/`PULOU` do autoteste.
- Falta: `--help` (clap), painel web e documentação em inglês; testar tudo isso em hardware real.

## Capturas do README

`tools/capturas` (veja o README de lá): sysfs de mentira do servidor, notebook e BC-250, carga por
thread falsa (`NUMAPILOT_PROC_STAT`), memória (`NUMAPILOT_MEMINFO`), janela no tamanho certo
(`NUMAPILOT_GUI_SIZE`) e na aba certa (`NUMAPILOT_GUI_TAB`), cliques pelo `entrada.py` (python-xlib)
numa tela Xvfb. Programas, Inicialização, Serviços e Mapa são capturas reais do CC150.

## Testar e registros

- `numapilot autoteste` — confere tudo nesta máquina sem mudar nada e grava
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
- **Acer**: sem perfil de energia nem controles depois de instalar o Linuwu-Sense (provável
  DKMS não compilado para o kernel 7.3.1 ou acer_wmi bloqueado). O autoteste novo mostra o motivo.
- **Saúde**: discos não aparecem (só a bateria). O relatório agora grava o motivo de cada disco.
- Telas USB-C/Thunderbolt sem imagem após desligamento forçado: `ucsi_acpi: PPM init failed`
  (firmware; resolver com desligar, tirar o carregador e segurar o power 30–60 s).
- Conferir que a NVIDIA desliga na bateria agora que a NVML não segura mais.

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

Aba Programas (2026-11-09) virou tabela no estilo do Mission Center (`programs.rs` na interface):
colunas ordenáveis com totais, seções Aplicativos / Sessão e sistema, linha que abre com os
processos e os controles, Encerrar/Forçar parada. Proteção e GPU por processo (fdinfo do DRM;
o amdgpu repete a VRAM em `drm-memory-vram` e `drm-resident-vram`, usar só uma) em
`src/procctl.rs`. Só encerra processos do próprio usuário fora de `session.slice`/`system.slice`
e fora da lista de protegidos (KDE, GNOME, áudio, portais, numapilot).

Próximo: aba Testes na BC-250 e no CC150, depois os outros PCs.
