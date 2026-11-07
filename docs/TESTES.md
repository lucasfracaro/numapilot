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

## Notebook (NVIDIA)

- [ ] `./install.sh`, `numapilot doctor`: Intel/AMD integrada como vídeo e a NVIDIA como dedicada
- [ ] Placas de vídeo → Driver NVIDIA: série indicada correta; "Instalar driver oficial"; reiniciar
- [ ] `numapilot run --use graphics -- vkcube` roda na NVIDIA (PRIME); `--gpu intel-0` na integrada
- [ ] Placa dedicada dorme parada (Máquina → Notebook mostra "dormindo") e acorda ao abrir um jogo
- [ ] Modo automático: Silencioso na bateria, Jogo/Normal na tomada; limite de carga em 80%
- [ ] Limite de potência e clock máximo da NVIDIA aplicam (se o notebook permitir)

## Outros PCs

- [ ] PC i7-5775C + BC-160 e PC CC150 + RX 9060 XT: `./install.sh`, `numapilot doctor`, aba Máquina
- [ ] BC-250: `numapilot doctor` (kernel, Mesa, governor, IOMMU); instalar/ligar o governor; editar a
      configuração pela aba Placas de vídeo e conferir o clock subindo (`cat .../pp_dpm_sclk`)
- [ ] BC-250: descobrir a interface de CUs do kernel com patch (para ligar/desligar pela interface)
      e o caminho de configuração do bc250_smu_oc (para aplicar o overclock da CPU pelo app)
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
