# Roteiro de testes

## Antes de tudo: autoteste, aba Testes e registros (qualquer PC)

1. `numapilot autoteste`: passa por todas as partes nesta máquina **só lendo** (sistema,
   serviço, configuração, cada placa, notebook, placa especial, doctor e cada comando de
   leitura) e grava `~/numapilot-autoteste-MAQUINA-DATA.txt`. Sai com código 2 se algo falhou.
   `--rapido` pula os comandos; `--json` para scripts.
2. Aba **Testes** no programa: roteiro à mão com só os itens que valem para esta máquina
   (notebook, NVIDIA, BC-250, 2 processadores...). Marque OK / Falhou / Pulei e escreva o que
   aconteceu. "Rodar o autoteste" e "Gerar relatório" juntam tudo (respostas, autoteste e o
   final dos registros) em `~/numapilot-testes-MAQUINA-DATA.txt`, para enviar.
3. Registros: `numapilot log` (ou `numapilot log servico|gui|cli|tray|web -n 100`).
   Ficam em `/var/log/numapilot/daemon.log` (serviço) e `~/.local/state/numapilot/*.log`
   (programa, bandeja, linha de comando); cada um vai até 2 MB e guarda o anterior em `.log.1`.

---

# Roteiro de testes no servidor

Tudo no numapilot foi testado com um hardware simulado. Este roteiro confere, no servidor de
verdade (2× Xeon E5-2696 v4, 2× MI50, RX470, CachyOS), o que só dá para ver lá. Para fazer junto
com o Claude: abra o app do Claude para desktop **no servidor**, ligue a conversa a ele e peça
"vamos fazer o roteiro de testes do numapilot". Mudanças no boot, undervolt e mitigações só com a
sua confirmação.

Ordem sugerida: do mais seguro para o que mexe no boot.

## 0. Instalação e diagnóstico

- [ ] `./install.sh` (ou `./install.sh --restore ~/numapilot-backup-....tar.gz` depois de formatar)
- [ ] `numapilot doctor --report` e guardar o `~/numapilot-relatorio.txt`
- [ ] `numapilot topo`: 2 nós, 44 threads cada, as 3 GPUs com o socket certo
- [ ] Se alguma GPU aparecer com `nó ?`: preencher `[gpu_node]` em `/etc/numapilot.toml`

## 1. Placas de vídeo e seleção de GPU

- [ ] No doctor, "Vulkan (DXVK/VKD3D): só esta GPU visível" para cada placa
- [ ] `numapilot run --gpu gfx906-0 --use graphics -- vkcube` abre na MI50 escolhida
- [ ] `numapilot run --gpu gfx906-1 --use graphics -- glxinfo -B` mostra a outra MI50
- [ ] Monitor ligado numa MI50: o aviso "monitor em outra placa" some
- [ ] Link PCIe de cada MI50 (aba Placas de vídeo): x16 com a placa em uso
- [ ] Ventoinha: curva e fixa funcionam; parar o serviço devolve ao automático
- [ ] Limite de potência aplica e volta depois de reiniciar

## 2. Jogos

- [ ] Inicialização: a lista bate com o que abre no login (Steam, bandeja, serviços); desligar um
      item, sair e entrar de novo: ele não abre; ligar de novo: abre. Itens essenciais (Plasma,
      rede) não deixam desligar
- [ ] Inicialização → + Adicionar: programa instalado (ex.: ZapZap), arquivo AppImage/.sh e comando
      com espera de 10 s; todos abrem no próximo login; Remover apaga o atalho
- [ ] Programas: "Abrir ao iniciar" num programa aberto; aparece na aba Inicialização
- [ ] Programas: ordenar por Nome, CPU, Memória e GPU (clicar de novo inverte); a GPU de um jogo
      bate com o Mission Center/MangoHud; buscar por nome e por PID
- [ ] Programas: abrir um programa seu (ex.: Dolphin) → selecionar → Encerrar fecha; Forçar parada
      pede confirmação; kwin/plasmashell/Xwayland ficam em "Sessão e sistema" e os botões não deixam
- [ ] Steam fechada → Programas → "Aplicar em todos os jogos"; abrir a Steam e conferir as opções
- [ ] Elden Ring com o perfil Jogo: FPS antes/depois (era ~20 com os 2 sockets)
- [ ] Jogo com Easy Anti-Cheat abre com o perfil Jogo (enxerga só os núcleos liberados)
- [ ] Crimson Desert renderizando e exibindo na mesma MI50 (monitor nela)
- [ ] "Baixa latência" ativa durante o jogo (Máquina → Latência em jogos mostra os núcleos)
- [ ] Perfil de energia da GPU muda para 3D_FULL_SCREEN com o jogo aberto e volta ao fechar
- [ ] Otimizador de FPS com MangoHud: rodar os testes num jogo e "Usar a melhor"
- [ ] gamescope por jogo com FSR

## 3. Máquina

- [ ] Modos Jogo, IA, Silencioso e Normal: `numapilot machine status` muda e volta
- [ ] Uncore disponível (`sudo modprobe intel_uncore_frequency`) e fixo no máximo no modo Jogo
- [ ] RAPL: consumo por socket aparece; limite do modo Silencioso aplica
- [ ] scx_loader: trocar o escalonador pelo modo Jogo e voltar no Normal
- [ ] Ventoinhas da placa-mãe aparecem (talvez precise de `sudo sensors-detect` / `nct6775`) ou IPMI
- [ ] Balanceamento NUMA desligado no modo Jogo (`cat /proc/sys/kernel/numa_balancing`)

## 4. Medições

- [ ] `numapilot bench`: latência e banda local × remota
- [ ] Coluna de memória (% local) e "corrigir" num programa com memória no socket errado

## 5. ROCm, laboratório e servidor de IA

- [ ] `gfx906-rocm lab up`, Jupyter e VS Code abrem; PyTorch vê as 2 MI50
- [ ] `gfx906-rocm validate lab`: correção e desempenho de cada MI50, cópia entre as duas
- [ ] `gfx906-rocm llm up <modelo>` com as 2 MI50; `gfx906-rocm validate llm` mede tokens/s
- [ ] `gfx906-rocm check` mostra o que há de novo sem mudar nada
- [ ] `gfx906-rocm update-all`: atualiza só o que passar na validação e grava o histórico

## 6. Boot (com confirmação)

- [ ] OverDrive: Placas de vídeo → "Ativar OverDrive", reiniciar, conferir clock/voltagem
- [ ] Undervolt de 25 mV numa MI50, teste de carga, conferir estabilidade
- [ ] Mitigações: "Desligar as leves", reiniciar, medir o ganho (ex.: o mesmo jogo ou `numapilot bench`)
- [ ] `pci=realloc` para tentar o Resizable BAR (se a placa-mãe não bootar, tirar o parâmetro no menu do boot)

## 7. Celular e extras

- [ ] Painel no celular pelo QR code, trocar o perfil de um programa
- [ ] Alerta de GPU quente na bandeja
- [ ] `numapilot backup` e guardar o arquivo fora do disco do sistema
- [ ] Meus PCs: colar o link do painel da BC-250 e do notebook no servidor; no celular, ver os três e abrir cada um
- [ ] Wake-on-LAN: ligar na BC-250 (Máquina → Ligar pela rede e na BIOS), desligar e ligar pelo botão do painel
- [ ] Undervolt automático numa MI50 e na RX 9060 XT; saúde mostra o ECC das MI50
- [ ] Servidor de IA: Otimizar (tokens/s) com um modelo de 7-14B; o `llm up` usa a combinação gravada
- [ ] Avisos: ntfy no celular, "Mandar teste"; GPU quente (baixe o limite para 50 °C para testar); derrubar o container da IA
- [ ] Modo automático: jogo com "Modo da máquina: Jogo" muda e volta ao fechar; horário Silencioso
- [ ] Energia: preço do kWh e "resto do PC"; comparar com um medidor de tomada; MI50 com "Economia quando parada"
- [ ] Painel: abrir um jogo da Steam e ligar o Sunshine pelo celular; gráfico de 24 h dos outros PCs
- [ ] Copiar perfis do servidor para o notebook; `numapilot peers backup`
- [ ] Atualizar o numapilot pela interface
- [ ] Servidor: `numapilot doctor` mostra NVMe e rede no socket certo
- [ ] VM Windows com a RX470: Conferir, Preparar, reiniciar, Criar, instalar o Windows e o driver

## Notebook (NVIDIA)

- [ ] `./install.sh`, `numapilot doctor`: Intel/AMD integrada como vídeo e a NVIDIA como dedicada
- [ ] Placas de vídeo → Driver NVIDIA: série indicada correta; "Instalar driver oficial"; reiniciar
- [ ] `numapilot run --use graphics -- vkcube` roda na NVIDIA (PRIME); `--gpu intel-0` na integrada
- [ ] Placa dedicada dorme parada (Máquina → Notebook mostra "dormindo") e acorda ao abrir um jogo
- [ ] Modo automático: Silencioso na bateria, Jogo/Normal na tomada; limite de carga em 80%
- [ ] Limite de potência e clock máximo da NVIDIA aplicam (se o notebook permitir)
- [x] Rodar `bash ~/Projetos/numapilot-teste/testar.sh` (Acer Nitro AN515-46, Ryzen 7 6800H, RTX 3070 Ti, 7 nov):
      corrigidos limite da NVIDIA de notebook, OverDrive padrão do CachyOS, temperatura do Ryzen,
      avisos de ventoinha/potência que não se aplicam a notebook, uncore só na Intel, EPP
- [ ] Instalar de verdade (`git clone` + `./install.sh`) para testar o serviço
- [x] Telas: com os adaptadores USB-C/Thunderbolt, DP-9 e DP-10 aparecem na 680M; o HDMI é da NVIDIA
- [ ] Modo da NVIDIA (experimental; salve o trabalho antes): `sudo numapilot hybrid xwayland on` e entrar de novo;
      "Desligada" com as telas USB-C ligadas: a NVIDIA some do `lspci` e o KDE continua; "Sempre disponível":
      ela volta e um monitor no HDMI acende sem sair da sessão; "Automático": tirar/colocar o carregador troca
- [ ] Com um jogo aberto na NVIDIA, pedir "Desligada": espera e mostra o jogo; ao fechar, desliga
- [x] Teste 7 nov: o modo esperou o Elden Ring; a placa nunca foi tirada (KWin, LACT, Sunshine, ZapZap
      seguravam). Telas piscando após trocar o monitor entre HDMI e USB-C; sumiu ao reiniciar
      (numapilot não mexeu nas placas). Se voltar: parâmetro amdgpu.dcdebugmask=0x10
- [ ] Instalador: aceitar "tudo que inicia com o sistema na integrada" e desligar o lactd; entrar de novo;
      `numapilot autostart` lista o que foi para a integrada; `numapilot hybrid` mostra a NVIDIA "dormindo";
      um jogo da Steam com perfil na NVIDIA ainda roda nela
- [ ] Máquina → Notebook: "Acordada por" lista quem segura a NVIDIA; fechar e ver "dormindo"
- [ ] Limites do Ryzen (ryzenadj): STAPM 35 W e 85 °C aplicam; voltam sozinhos depois de tirar/pôr na tomada
- [ ] EPP (power/balance_power) aplica no modo Silencioso
- [ ] Acer: módulo Linuwu-Sense carregado; ventoinha máximo/automático, limite de 80%, RGB
- [ ] NVIDIA: clock travado 1800 MHz + curva +150 MHz; consumo menor no mesmo FPS; volta a 0 ao desmarcar
- [ ] Undervolt automático na NVIDIA (`sudo pacman -S vkmark`; botão "Achar o undervolt sozinho"); Ctrl+C devolve
- [ ] Saúde do hardware: SSD e bateria aparecem (sudo pacman -S smartmontools)

## Outros PCs

- [ ] PC i7-5775C + BC-160 e PC CC150 + RX 9060 XT: `./install.sh`, `numapilot doctor`, aba Máquina
- [ ] BC-250: `numapilot doctor` (kernel, Mesa, governor, IOMMU); instalar/ligar o governor; editar a
      configuração pela aba Placas de vídeo e conferir o clock subindo no cartão da GPU (vem do
      hwmon `freq1_input`; com 8 núcleos o `pp_dpm_sclk` não é confiável)
- [ ] BC-250: instalar o BC250 Control Center pelo botão; desbloquear 8 núcleos e 40 CUs por ele;
      o numapilot mostra 8/8 núcleos e (depois de reiniciar) 40/40 CUs
- [ ] BC-250: Placas de vídeo → ajustes da comunidade: "★ BC250 Control Center" → Instalar (terminal
      com o paru); depois o botão vira Abrir/Atualizar. Em "Outros programas", instalar o
      bc250-core-unlock (git) e conferir `~/.local/bin/bc250-unlock-cores.py`; Atualizar faz `git pull`
- [ ] BC-250: o resumo "agora" mostra núcleos + GHz e CUs + MHz; depois de abrir a página de CUs no
      Control Center (40 CUs ao vivo), o numapilot passa a mostrar 40 "última leitura do BC250 Control Center"
- [ ] BC-250 com 8 núcleos: o doctor diz se o desbloqueio fica ao desligar da tomada (biosmod/EFI)
      ou não (bc250-core-unlock); se temperatura/potência da GPU vierem 0 °C ou ~1470 W, a dica do
      doctor bate com o kernel em uso (patch da SMU no BIOS, tirar `cs_legacy_8core_metrics` ou
      kernel linux-cachyos-bc250)
- [ ] BC-250 com o kernel linux-cachyos-bc250: se `amdgpu.bc250_cc_write_mode=3`, o doctor mostra
      "CUs pelo kernel" e o autoteste "liberadas pelo kernel"
- [ ] BC-250: Memória para a GPU 14 GiB; `numapilot boot status`; carregar um modelo de 14B no servidor de IA
- [ ] BC-250: Streaming com codificador por software (superfast) no Moonlight
- [ ] RX 9060 XT: deslocamento de voltagem (-50 mV), curva da ventoinha no firmware, link PCIe
- [ ] BC-160: curva de 3 pontos e ventoinha; renderizar nela com o monitor na outra placa

## Próximas melhorias combinadas

1. Otimizador do servidor de IA (tokens/s: 1 ou 2 MI50, divisão por camadas ou linhas, socket).
2. Undervolt automático com teste de estabilidade.
3. Saúde do hardware: erros ECC da RAM e da HBM das MI50, SMART dos discos.
4. Modo da máquina automático por programa e por horário.
5. Consumo e custo de energia (R$/kWh), limite automático em GPU parada.
6. Discos e rede no socket certo.
7. Celular como controle remoto (Wake-on-LAN, abrir jogo pelo Sunshine).
8. Atualização do próprio numapilot pela interface.
9. GPU para máquina virtual Windows (jogos com anti-cheat que não rodam no Linux).
