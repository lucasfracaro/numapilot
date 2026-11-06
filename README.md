# numapilot

Escolhe **qual processador (socket NUMA), qual memória e qual GPU** cada programa usa,
em máquinas com 2 sockets. Feito para um servidor com 2× Xeon E5-2696 v4, 2× MI50 e 1× RX470
no CachyOS, mas funciona em qualquer Linux.

- **Automático (padrão):** um daemon observa os programas, coloca cada um no socket mais
  adequado e move a memória junto. Só usa os dois sockets quando o programa satura um deles.
- **Manual:** você decide o socket e a GPU, por comando (`numapilot run`) ou por regra no
  `/etc/numapilot.toml`.
- **Interface gráfica (`numapilot-gui`):** lista os programas abertos e deixa escolher, por
  programa, o processador e a placa de vídeo. O padrão é tudo no automático.
- **Controle fino por programa:** quantos e quais núcleos, com ou sem hyper-threading, um ou dois
  processadores, limite de memória, memória só do socket ou não, prioridade, núcleos exclusivos e
  quantos núcleos o programa "enxerga". Tem perfis prontos (Jogo, IA, Renderização, Silencioso).
- **Mapa e medições:** os núcleos de cada socket com o uso e quem está em cada um, a memória de cada
  programa (% local), alocações no outro socket e a latência/banda medidas entre os sockets.
- **Placas de vídeo:** temperaturas, curva da ventoinha e limite de potência de cada MI50.
- **Laboratório e containers:** Jupyter, VS Code e PyCharm com PyTorch nas MI50, abertos por um
  clique; lista os containers e os apps instalados em cada um.
- **Servidor de IA:** llama.cpp nas MI50 com API compatível com a da OpenAI.
- **Streaming:** liga o Sunshine e escolhe qual GPU codifica o vídeo.
- **Ícone na bandeja** com as temperaturas e atalhos, e **atualização semanal automática** que testa
  nas GPUs e volta sozinha se algo quebrar.
- **gfx906-rocm:** instala, atualiza, testa e faz rollback dos ROCm da comunidade para as MI50,
  sem mexer no ROCm do sistema.

![Interface](docs/interface.png)

## 1. Antes de tudo: BIOS

Na BIOS, **NUMA ligado / "Node Interleaving" desligado**. Se o `numapilot topo` mostrar só
1 nó com 2 sockets, ele avisa. Se alguma GPU aparecer com `nó ?`, defina o socket dela em
`[gpu_node]` no config.

## 2. Instalar

```bash
git clone <este repositório> numapilot && cd numapilot
./install.sh          # compila, instala em /usr/local/bin, cria o atalho no menu e ativa o serviço
numapilot topo        # nós, núcleos, RAM, GPUs e em qual socket cada GPU está
```

Para remover: `./uninstall.sh`.

**Pelo pacman (Arch/CachyOS):** o `packaging/arch/PKGBUILD` gera o pacote `numapilot-git` a partir
deste repositório, com o binário em `/usr/bin` e o serviço em `/usr/lib/systemd/system`:

```bash
cd packaging/arch && makepkg -si
sudo systemctl enable --now numapilot
```

Para publicar no AUR, gere o `.SRCINFO` com `makepkg --printsrcinfo > .SRCINFO`.

## 3. Modo automático (o daemon)

Ele já fica rodando depois do `install.sh`. Para cada programa de usuário:

| Situação | O que o daemon faz |
|---|---|
| O programa casa com uma regra do config | Aplica a regra na hora (antes de alocar muita memória) |
| É filho de um programa com regra (ex.: jogo aberto pela Steam) | Herda a decisão |
| Abre uma GPU (`/dev/dri/renderD*`) | Vai para o **socket ligado àquela GPU** |
| Usa CPU (≥ 0,5 núcleo por 3 s) sem GPU | Vai para o **socket menos ocupado** com RAM livre |
| Usa ≥ 85% dos CPUs do socket por 10 s | **Expande para os 2 sockets** e grava isso |
| Expandido, mas usando < 30% de um socket por 2 min | Volta para 1 socket |
| Já foi visto antes | Começa direto no lugar aprendido (2 sockets, ou o socket da GPU dele) |

O daemon não mexe em processos do sistema, em programas abertos com `numapilot run` no modo
manual, em programas fixados com `numapilot pin`, nem em quem já definiu a própria afinidade
(ex.: `taskset`). Ao parar o serviço, ele devolve todos os CPUs para os processos que moveu.

```bash
numapilot status                  # o que está sendo gerenciado agora
numapilot learned                 # o que ele aprendeu
sudo numapilot learned --forget blender
journalctl -u numapilot -f        # decisões em tempo real
```

## 4. Interface gráfica

Abra **numapilot** no menu (ou `numapilot-gui`). O daemon em segundo plano mapeia cada programa
aberto, e ele aparece na lista agrupado por programa. Os jogos da Steam são agrupados pelo AppId.

- **Processador:** Automático, Socket da GPU escolhida, Só o socket 0/1 ou Os dois sockets.
  A troca vale **na hora**: o processo e a memória dele mudam de socket.
- **Placa de vídeo:** Automático, a placa de vídeo, cada MI50 como **jogo/gráfico** ou
  **IA/ROCm**, ou Nenhuma. A GPU só pode ser escolhida quando o programa abre, então a escolha
  fica salva e vale na próxima abertura. Para valer sem digitar comandos:
  - programas comuns: **Reabrir** (fecha e abre já na GPU nova) ou **Usar no menu**. Esse botão
    cria uma cópia do atalho em `~/.local/share/applications` que abre o programa pelo numapilot.
  - jogos da Steam: **Copiar opção Steam** e cole `numapilot run -- %command%` em
    Propriedades → Opções de inicialização. É uma vez por jogo; depois, a GPU escolhida na interface vale sempre.
- **Lossless Scaling:** em qual GPU o lsfg-vk gera os quadros (veja a seção 6).
- A aba **Escolhas salvas** mostra o que não está no automático e permite voltar ao automático.

As escolhas ficam em `/var/lib/numapilot/profiles.json`. Na linha de comando, use
`numapilot profile list|set|rm`.

**Outras abas:**

| Aba | O que faz |
|---|---|
| Mapa e medições | núcleos de cada socket (uso, programas presos a cada um), memória por socket, alocações no outro socket, governador de frequência por socket, latência e banda medidas |
| Placas de vídeo | temperaturas (borda/hotspot/memória), ventoinha automática, por curva ou fixa, e limite de potência |
| Laboratório e containers | liga/atualiza o laboratório; abre JupyterLab, VS Code (navegador ou app do PC), PyCharm e terminal; lista os containers e os apps de cada um |
| Servidor de IA | escolhe o modelo `.gguf`, as MI50 e o contexto; inicia/para; abre o chat e mostra o endereço da API |
| Streaming | liga o Sunshine e escolhe a GPU que codifica o vídeo (mostra quem faz H.264/HEVC/AV1) |
| Configurações | ícone na bandeja ao iniciar a sessão, atualização semanal, hardware detectado |

Comandos demorados (ligar o laboratório, atualizar...) rodam em segundo plano, numa janela com a
saída ao vivo. A interface continua usável enquanto isso.

![Laboratório e containers](docs/laboratorio.png)

## 5. Escolher GPU e socket ao abrir um programa (linha de comando)

A GPU só pode ser escolhida **quando o programa abre** (por variáveis de ambiente), por isso
existe o `numapilot run`. Sem opções, ele decide sozinho:

```bash
numapilot run -- blender                    # automático
numapilot run -n -- llama-server -m x.gguf  # -n: só mostra o que faria, e por quê
```

Manual:

```bash
numapilot run --gpu display --cpu gpu -- ./jogo        # RX470 + o socket dela
numapilot run --gpu gfx906-1 --use graphics -- %command% # jogo renderizado numa MI50
numapilot run --gpu gfx906-1 -- llama-server ...        # MI50 específica (socket dela)
numapilot run --gpu compute:all --cpu all -- python treino.py   # as 2 MI50 + 2 sockets
numapilot run --cpu 0 --gpu none -- ffmpeg ...           # só o socket 0
numapilot run --cpu all --mem interleave -- ./banco-de-dados
```

| Opção | Valores |
|---|---|
| `--cpu` | `auto`, `all`, `gpu` (socket da GPU), `0`, `1`, `0,1`, `cpus:0-21` |
| `--gpu` | `auto`, `display`, `compute`, `compute:2`, `compute:all`, `none`, apelido ou PCI |
| `--mem` | `local` (padrão), `interleave`, `bind` |
| `--use` | `auto`, `graphics` (jogo, inclusive numa MI50), `compute` (ROCm) |
| `--fg-gpu` | GPU do frame generation do Lossless Scaling (padrão: a mesma do jogo) |

No automático, a GPU de vídeo (com monitor) vai para programas gráficos. As MI50 vão para
programas de IA/computação conhecidos (`llama-*`, `ollama`, `comfyui`, `vllm`, `whisper*`...;
lista em `compute_hints`), sempre a **menos ocupada** (uso + VRAM). O socket é o da GPU escolhida.

O que ele configura: `DRI_PRIME=pci-...` para OpenGL/Vulkan (o `MESA_VK_DEVICE_SELECT` só é usado
quando o ID da placa é único; as duas MI50 têm o mesmo ID), `LSFGVK_GPU` para o Lossless Scaling, e
`ROCR_VISIBLE_DEVICES` (UUID da MI50, que esconde a RX470 do ROCm) para ROCm/HIP. Também exporta
`NUMAPILOT_CPUS`, `NUMAPILOT_MEMS` e `NUMAPILOT_RENDER` para scripts e containers.

**Steam:** nas opções de inicialização do jogo, use `numapilot run --gpu display -- %command%`.
A regra `jogos` do config já cobre a Steam inteira pelo daemon.

Para um processo que já está rodando: `sudo numapilot pin <pid> --cpu 1` (move os CPUs e a RAM).

## 6. Jogos nas MI50 e Lossless Scaling

**Jogar numa MI50:** escolha "gfx906-N — jogo/gráfico" na interface, ou `--use graphics`.
- Com o monitor ligado na própria MI50, ela renderiza e exibe direto. O numapilot reconhece a
  placa como MI50 mesmo com monitor ligado.
- Com o monitor na RX470, a MI50 renderiza e cada quadro é copiado para a RX470 (PRIME
  offload). Nesse caso, prefira a MI50 do mesmo socket da RX470.
- Para o automático mandar todos os jogos para uma MI50, defina `graphics_gpu = "gfx906-1"` no config.

**Lossless Scaling no Linux (lsfg-vk):**
- O lsfg-vk oficial gera os quadros **na mesma GPU do jogo**. Dual GPU ainda não é suportado,
  e o numapilot já aponta o `LSFGVK_GPU` para a GPU do jogo.
- Para dual GPU (jogo numa MI50, frame generation na outra, com o monitor ligado nela), é
  preciso um fork do lsfg-vk com essa função. Em "Lossless Scaling" na interface (ou
  `--fg-gpu`), escolha a outra GPU: o numapilot exporta `LSFGVK_GPU` com ela. Isso é
  experimental e depende do fork respeitar essa variável.

## 7. Regras manuais permanentes

Edite `/etc/numapilot.toml` (exemplo comentado incluído) e rode `sudo systemctl restart numapilot`:

```toml
[aliases]
"0000:03:00.0" = "rx470"
"0000:04:00.0" = "mi50a"

[[rule]]
name = "jogos"
match = ["steam", "*.exe", "gamescope"]
cpu = "gpu"        # socket da RX470
gpu = "display"
reserve = true     # o automático evita pôr outras cargas pesadas nesse socket
```

## 8. MI50 com o ROCm da comunidade (`gfx906-rocm`)

A AMD tirou a gfx906 dos builds oficiais, mas o compilador ainda suporta ela. Existem dois caminhos
mantidos pela comunidade, e o script cuida dos dois:

### a) Container: o mais fácil de manter

Usa a imagem do projeto **mixa3607/ML-gfx906** (ROCm 10.1 para gfx906, compilado via TheRock; 7.14 e 10.0 também existem: troque `IMAGE` no config).
Isso não depende das bibliotecas do CachyOS. O host só precisa do driver `amdgpu`, que já vem no kernel.

```bash
sudo pacman -S podman
gfx906-rocm container pull                       # atualizar = rodar de novo / trocar a tag
numapilot run --gpu gfx906-1 -- gfx906-rocm container run -- rocminfo
```

O container recebe só a(s) MI50 escolhida(s) pelo numapilot (`NUMAPILOT_RENDER`) e os CPUs do
socket dela. O mesmo projeto também tem imagens prontas de PyTorch, ComfyUI e llama.cpp.

### b) Nativo (tarball lado a lado)

Cada versão fica em `/opt/rocm-gfx906/<versão>`. `current` aponta para a ativa e `previous` para
a anterior. Nada vai para o PATH global, então não briga com o ROCm do pacman.

```bash
sudo gfx906-rocm update --check   # há versão nova?
sudo gfx906-rocm update           # baixa, testa nas MI50 e só ativa se passar
gfx906-rocm list
sudo gfx906-rocm rollback         # voltou algo quebrado? volta para a anterior
sudo gfx906-rocm prune 2          # limpa as antigas
gfx906-rocm doctor                # driver, /dev/kfd, grupos, GPUs, conflitos

# usar
numapilot run --gpu compute -- gfx906-rocm exec -- llama-server -m modelo.gguf -ngl 99
eval "$(gfx906-rocm env)"         # ou carregar no shell atual
```

O `update` procura, por padrão, os tarballs noturnos do TheRock com `gfx906` no nome
(`INDEX_URL`/`FILTER` em `/etc/gfx906-rocm.conf`). Também dá para instalar qualquer tarball
(ex.: um build seu ou de outro mantenedor): `sudo gfx906-rocm install <url-ou-arquivo> [nome]`.
O teste roda `rocminfo` e compila e executa um kernel HIP em cada MI50. Se falhar, a versão nova
fica instalada, mas **não é ativada**.

Para manutenção automática, crie um timer do systemd que rode `gfx906-rocm update` uma vez por
semana. Como ele só ativa versões que passam no teste, é seguro.

Dicas da comunidade para gfx906:

- Compile seus programas com `--offload-arch=gfx906` (ou `AMDGPU_TARGETS=gfx906`).
- Builds antigos do ROCm 6.x precisavam de `HSA_OVERRIDE_GFX_VERSION=9.0.6`. Os builds próprios
  para gfx906 não precisam.
- Usuário nos grupos `render` e `video` (o `doctor` verifica).

## 9. Laboratório de ciência de dados e ML nas MI50 (`gfx906-rocm lab`)

Um container com tudo pronto para usar as MI50. É construído sobre o PyTorch compilado para gfx906
pela comunidade (mixa3607/ML-gfx906) e ganha:

- **Dados/ciência:** numpy, scipy, pandas, polars, pyarrow, duckdb, numba, scikit-learn,
  scikit-image, statsmodels, sympy, networkx
- **Visualização:** matplotlib, seaborn, plotly, altair, ipywidgets
- **ML clássico:** xgboost, lightgbm, catboost, optuna, shap, mlflow, tensorboard. Esses rodam na
  CPU, que aqui são 88 threads; o PyPI não tem builds deles para ROCm.
- **Deep learning (na GPU, via PyTorch):** transformers, datasets, accelerate, peft, lightning,
  torchmetrics, timm, kornia
- **Ferramentas:** JupyterLab, VS Code no navegador (code-server, com as extensões Python e Jupyter),
  SSH, ruff, pytest, nvtop, git, opcionalmente o PyCharm

O PyTorch gfx906 da imagem fica **travado** no build: nenhum pacote pode trocá-lo por um build CUDA do PyPI.

```bash
sudo pacman -S podman          # recomendado (sem root); docker também funciona
gfx906-rocm lab up             # constrói na 1ª vez (a base tem vários GB) e sobe
gfx906-rocm lab                # mostra os endereços, o token e a senha
gfx906-rocm lab test           # PyTorch enxerga as MI50? faz conta em cada uma?
gfx906-rocm lab shell          # terminal dentro do container
gfx906-rocm lab down
```

| Serviço | Endereço |
|---|---|
| JupyterLab | `http://127.0.0.1:8888/lab?token=...` |
| VS Code (navegador) | `http://127.0.0.1:8443` (a senha é o mesmo token) |
| SSH | `ssh -p 2222 root@127.0.0.1` (usa as suas chaves de `~/.ssh`) |

Os projetos ficam em `~/lab` (no container: `/workspace`). Os modelos baixados ficam no cache do
Hugging Face do host (`~/.cache/huggingface`), então não são baixados de novo.

**Editores no host:**
- **VS Code (desktop):** use a extensão *Remote - SSH* com `Host gfx906-lab`, `HostName 127.0.0.1`,
  `Port 2222`, `User root`. Outra opção é *Dev Containers → Attach to Running Container*; com podman,
  configure `"dev.containers.dockerPath": "podman"`.
- **PyCharm:** o interpretador remoto por SSH (`/usr/bin/python3` em `127.0.0.1:2222`) é recurso da
  versão paga. Sem ela, defina `LAB_PYCHARM=1` em `/etc/gfx906-rocm.conf`, rode `gfx906-rocm lab build`
  e depois `gfx906-rocm lab pycharm`: o PyCharm roda **dentro** do container e a janela abre na sua tela.

**Atualizar com segurança:**

```bash
gfx906-rocm lab update --check   # há base nova (PyTorch/ROCm)?
gfx906-rocm lab update           # reconstrói, testa nas GPUs e volta sozinho se falhar
gfx906-rocm lab update --force   # mesma base, mas atualiza todos os pacotes Python
gfx906-rocm lab rollback         # volta para a imagem anterior
```

Por padrão, o update usa sempre a tag mais nova (`vX.Y.Z-rocm-A.B`). Para ficar numa versão,
defina `LAB_BASE` no config. Há relatos de falha em multi-GPU com ROCm novo em algumas máquinas
(issue #28 do ML-gfx906). Se acontecer, fixe uma base 7.2.x.

**Com o numapilot:** para dedicar uma MI50 ao laboratório e deixar a outra livre para jogos, use
`numapilot run --gpu gfx906-1 --use compute -- gfx906-rocm lab up`. O container recebe só aquela
GPU e os CPUs do socket dela.

Para acessar de outro computador da rede, use `LAB_BIND="0.0.0.0"`. O acesso continua protegido por
token, senha e chave SSH.

## 10. Ventoinha e potência das MI50

Na aba **Placas de vídeo**, cada placa tem três modos de ventoinha: o automático da própria placa,
uma **curva** (temperatura do hotspot → %) ou uma velocidade **fixa**. Também dá para limitar a
**potência** em W, dentro do mínimo e do máximo da placa.

- Quem aplica é o serviço numapilot. As escolhas ficam em `/var/lib/numapilot/gpu.json` e voltam
  sozinhas depois de reiniciar.
- Segurança: a 95 °C a ventoinha vai a 100%, qualquer que seja o modo. Se o serviço parar, as
  ventoinhas voltam para o controle da placa.
- Algumas VBIOS não expõem o controle da ventoinha. Nesse caso, a interface avisa.

![Placas de vídeo](docs/gpus.png)

## 11. Servidor de IA (`gfx906-rocm llm`)

Roda o llama.cpp compilado para gfx906 (builds diárias do mixa3607/ML-gfx906) num container, com
API compatível com a da OpenAI. Funciona com Open WebUI, Continue, LibreChat e scripts.

```bash
mkdir -p ~/models   # coloque os .gguf aqui
gfx906-rocm llm models
gfx906-rocm llm up Qwen3-32B-Q4_K_M.gguf   # ou escolha pela interface
gfx906-rocm llm                            # endereço do chat e da API
gfx906-rocm llm update                     # build nova do llama.cpp (volta se falhar)
```

- Padrão: as duas MI50, com o modelo dividido por camadas. Para usar só uma, escolha `gfx906-0` ou
  `gfx906-1` na interface ou em `LLM_GPU`.
- Ele passa pelo numapilot: o container recebe só as GPUs escolhidas e os processadores e a memória
  do socket delas.
- Os ajustes da interface ficam em `~/.config/gfx906-lab/settings.env`.

## 12. Atualização automática e ícone na bandeja

- `gfx906-rocm autoupdate enable` (ou o checkbox em Configurações) cria um timer semanal do systemd
  do usuário. Ele atualiza o laboratório, o llama.cpp e a imagem ROCm, sempre testando nas GPUs e
  voltando para a versão anterior se falhar. O resultado aparece numa notificação e em Configurações.
- O **ícone na bandeja** (`numapilot-gui --tray`, ou ative em Configurações) mostra a temperatura,
  o uso e a potência de cada GPU. Fica de outra cor acima de 90 °C. O menu dá acesso ao JupyterLab,
  ao VS Code, ao PyCharm, ao terminal do laboratório, ao chat do servidor de IA e ao painel do Sunshine.
- Para os containers voltarem depois de reiniciar o PC, o podman sem root usa o
  `podman-restart.service` do usuário, que é ativado automaticamente. Para eles subirem mesmo sem
  você fazer login: `sudo loginctl enable-linger $USER`.

## 13. Detecção instantânea

Como root, o daemon recebe do kernel um aviso a cada programa aberto (netlink "proc connector").
Assim, a regra, o perfil ou o aprendizado é aplicado logo após o programa iniciar, antes de ele
alocar memória, e a memória já nasce no socket certo. Sem essa permissão, ele volta para a
verificação a cada segundo.

## 14. Controle fino por programa (botão "Ajustar…")

Em **Programas** (ou em **Escolhas salvas**), o botão **Ajustar…** abre o editor do programa:

| Opção | O que faz | Quando vale |
|---|---|---|
| Processadores | socket 0, socket 1, os dois ou o socket da GPU | na hora |
| Núcleos | limitar a N CPUs, com ou sem hyper-threading, ou escolher no mapa (clique = núcleo inteiro; botão direito = 1 thread) | na hora |
| O programa enxerga | só os núcleos liberados: `WINE_CPU_TOPOLOGY` (Proton), `OMP/MKL/OPENBLAS/RAYON_NUM_THREADS`, `GOMAXPROCS`, .NET e Java | ao abrir |
| Memória | automático, **só do socket** (estrito, menor latência), preferir a local, intercalar (mais banda) ou livre | na hora (intercalar: ao abrir) |
| Limite de memória | teto em GB (cgroup `MemoryMax`) | na hora |
| Prioridade | baixa, normal ou alta (CPU e disco) | na hora |
| Núcleos exclusivos | o resto do sistema deixa de usar esses núcleos enquanto o programa roda | na hora |
| Placa de vídeo | GPU e uso (jogo/IA) e GPU do Lossless Scaling | ao abrir |

Embaixo, uma prévia mostra exatamente o que vai acontecer ("Vai usar 16 CPUs (0-15) no socket 0 ·
GPU gfx906-0 · memória só do socket"), com avisos de compatibilidade.

**Como funciona:** os limites de memória, a memória estrita e os núcleos exclusivos usam um grupo de
controle (cgroup v2) por programa, criado pelo próprio systemd em `numapilot.slice`. Assim o
numapilot não briga com o systemd e os processos filhos herdam tudo. Para núcleos exclusivos, o
`user.slice` e o `system.slice` deixam de usar esses núcleos enquanto o programa roda. Se o serviço
parar, tudo volta ao normal.

![Editor de programa](docs/editor.png)

## 15. Jogos em servidor com 2 processadores (DirectX e Vulkan)

Os problemas típicos desse tipo de máquina, e o que o perfil **Jogo** faz:

| Problema | Causa | O que o numapilot faz |
|---|---|---|
| Jogo não abre ou o Easy Anti-Cheat bloqueia | o jogo enxerga 88 threads; vários jogos e anti-cheats quebram acima de ~32 | `WINE_CPU_TOPOLOGY` faz o jogo do Proton enxergar só os núcleos liberados (o `taskset` sozinho não muda isso) |
| FPS muito baixo (ex.: 20 em vez de 60) | threads e memória espalhados pelos 2 sockets: cada acesso remoto custa ~50% a mais | 1 socket só (o da GPU), núcleos físicos sem HT, memória só local, prioridade alta e núcleos reservados |
| Jogo recusa abrir renderizando numa placa e exibindo em outra | a placa do jogo não tem o monitor (PRIME) ou está no outro socket | o jogo só enxerga a GPU escolhida e um aviso pede para ligar o monitor nela |

**DirectX e Vulkan:** no Linux, jogos DirectX rodam pelo Proton, que traduz para Vulkan: o DXVK
cuida do DirectX 9/10/11 e o VKD3D-Proton do DirectX 12. Então tudo que vale para Vulkan vale para
os dois.
- O numapilot define `DRI_PRIME=pci-...` e `MESA_VK_DEVICE_SELECT_FORCE_DEFAULT_DEVICE=1`. Com isso,
  o jogo enxerga **só** a GPU escolhida e não escolhe sozinho outra placa (ex.: a outra MI50, com
  mais VRAM).
- Jogos OpenGL (e DirectX antigo pelo WineD3D) seguem o `DRI_PRIME`.
- A parte de processador e memória vale para qualquer programa, independente da API gráfica.

Receita para um jogo exigente (ex.: Elden Ring):
1. Ligue o monitor na MI50 que vai rodar o jogo, ou use a RX470 se for o caso.
2. Em **Ajustar…**, clique em **Jogo** e escolha essa placa como "jogo/gráfico".
3. Na Steam, cole `numapilot run -- %command%` nas opções de inicialização.
4. Em **Mapa e medições**, coloque o socket da placa em `performance`.

## 16. Medições

- `numapilot bench`, ou o botão **Medir agora** em Mapa e medições, mede a latência (pointer
  chasing) e a banda de leitura de cada socket lendo a memória dele mesmo e a do outro.
- A coluna de memória em **Programas** mostra quanto cada programa usa e quanto está no socket dos
  núcleos dele (lido do `numa_maps`). Abaixo de 80% aparece em laranja, com o botão **corrigir**.
- Em Mapa e medições, "alocações no outro socket" vem do `numastat` e mostra páginas criadas longe
  de quem pediu.

## 17. Limitações

- O daemon troca CPU e memória de programas em execução, mas não a GPU (isso exige `numapilot run`).
- Programas Vulkan de computação (ex.: llama.cpp com backend Vulkan) escolhem a GPU pelo índice
  próprio (`GGML_VK_VISIBLE_DEVICES`). O numapilot ainda não configura isso.
- No CachyOS, os schedulers sched-ext (scx_*) e o ananicy-cpp convivem com o numapilot, porque
  respeitam a afinidade de CPU. Vale medir com e sem.
- Ganho esperado em jogos: frametime mais estável e menos stutter. O teto continua sendo o clock
  do Broadwell.

## Arquitetura do código

```
src/bin/numapilot-gui/  interface gráfica (egui) e ícone na bandeja (ksni)
src/ctl.rs      socket de controle do daemon (a interface fala por ele)
src/profiles.rs escolhas por programa
src/desktop.rs  atalhos do menu que abrem pelo numapilot
src/gpuctl.rs   sensores, curva da ventoinha e limite de potência (hwmon)
src/procevents.rs  aviso instantâneo de programas novos (proc connector)
src/containers.rs  containers podman/docker e os apps de cada um
src/labcfg.rs   ajustes do laboratório e do servidor de IA
src/cgroup.rs   grupos de controle por programa via systemd (memória, núcleos exclusivos)
src/bench.rs    medição de latência e banda de memória entre sockets
container/      imagem do laboratório (Containerfile, pacotes, lab-start)
src/topo.rs     nós NUMA, núcleos, RAM, GPUs (PCI, socket, KFD/gfx, UUID ROCm, monitor)
src/decide.rs   decisão de GPU + socket (usada pelo `run`)
src/daemon.rs   modo automático (observa /proc, aplica afinidade, migra memória)
src/procfs.rs   leitura do /proc + sched_setaffinity, set_mempolicy, migrate_pages
src/config.rs   /etc/numapilot.toml
src/learned.rs  /var/lib/numapilot/learned.json
```

`cargo test` roda os testes, incluindo uma simulação desta máquina (2×22c/44t, RX470 + 2×MI50)
com um sysfs falso.
