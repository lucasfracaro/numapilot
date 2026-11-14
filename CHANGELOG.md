# Mudanças

## Ainda não lançado

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
