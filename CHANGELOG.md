# Mudanças

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
