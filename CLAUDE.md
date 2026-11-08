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
    ```
    Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>
    Claude-Session: https://claude.ai/code/session_01JaLAC3YoxBRZYaaPzqYegM
    ```
    (numa sessão nova, use o link da sessão atual no lugar do Claude-Session).
  - A tag **v0.2.0** ainda precisa ser enviada por ele: `git tag v0.2.0 && git push origin v0.2.0`.

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
| **BC-250** | APU AMD (Cyan Skillfish), 6 núcleos | GPU integrada da própria placa |
| PC CC150 | CC150 | RX 9060 XT |
| PC | i7-5775C | BC-160 |

### BC-250 (próximo a testar)
- Kernel recente do CachyOS já funciona: **não precisa mais de kernel com patch**.
- Apps da comunidade aparecem na aba Placas (BC250 Control Center, umr,
  bc250-core-cu-unlock, bc250_smu_oc) — receitas em `recipes/default.toml` (id `bc250`).
- Sem codificador de vídeo: Sunshine com codificação por software (`no_video_encoder`).
- Memória da GPU (TTM `pages_limit` em modprobe.d) em Placas → memória da GPU integrada;
  llama.cpp com `GGML_VK_PREFER_HOST_MEMORY`.
- `boards::active_cores`/`active_cu` (journal `active_cu_number`) para conferir o desbloqueio.

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

Próximo: instalar e testar na **BC-250** (autoteste + aba Testes) e depois nos outros PCs.
