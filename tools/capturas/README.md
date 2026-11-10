# Capturas de tela do README

Gera as imagens de `docs/` sem precisar do hardware: um sysfs de mentira (`NUMAPILOT_SYSFS`), um
`/proc/stat` com carga inventada (`NUMAPILOT_PROC_STAT`), memória (`NUMAPILOT_MEMINFO`) e a interface
numa tela virtual. Precisa de `xorg-server-xvfb`, `imagemagick` e `python-xlib`, e do numapilot
compilado (`cargo build --release`).

```bash
cd tools/capturas
./servidor.sh /tmp/srv            # 2× Xeon E5-2696 v4 + 2× MI50 + RX 470 (ou notebook.sh, bc250.sh)
mkdir -p /tmp/fakeproc && printf 'MemTotal: 131600000 kB\nMemAvailable: 79800000 kB\n' > /tmp/fakeproc/meminfo
touch /tmp/vary.on && python3 cpu.py &   # carga por thread em /tmp/fakeproc/stat
# o serviço, como root, contra o mesmo sysfs (histórico, potência, programas)
sudo env NUMAPILOT_SYSFS=/tmp/srv NUMAPILOT_PROC_STAT=/tmp/fakeproc/stat ../../target/release/numapilot daemon &
./capturar.sh 5 ../../docs/gpus.png 25 click 382 268 sleep 1   # aba Placas, histórico de 6 h
```

- `capturar.sh ABA SAIDA [ESPERA] [AÇÕES]`: as ações (`click X Y`, `scroll X Y N`, `sleep S`) vão
  para o `entrada.py`, em coordenadas da janela.
- `programa.c`: programa que gasta CPU e memória, para aparecer na aba Programas
  (`gcc -O1 -pthread -o /usr/bin/obs programa.c`; rode com `NUMAPILOT_MODE=auto` e um usuário comum).
- O histórico de 24 h das placas pode ser escrito direto em `/run/numapilot/history.json` antes de
  iniciar o serviço.
- Sem `nvidia-smi` de verdade, a NVIDIA do `notebook.sh` precisa de um `nvidia-smi` falso no PATH
  (`FBIN`) que responda ao `--query-gpu`.
