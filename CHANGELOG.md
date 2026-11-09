# Mudanças

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
