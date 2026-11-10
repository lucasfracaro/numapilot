#!/usr/bin/env bash
# Captura uma aba da interface numa tela virtual (Xvfb) com hardware de mentira.
#   capturar.sh ABA SAIDA.png [ESPERA_s] [ACOES...]
# ABA: 0 Programas, 1 Inicialização, 2 Serviços, 3 Mapa, 4 Escolhas, 5 Placas, 6 Máquina,
#      7 Laboratório, 8 IA, 9 Streaming, 10 Configurações, 11 Testes
# ACOES: passadas ao entrada.py (click X Y, scroll X Y N, sleep S...), em coordenadas da janela.
# Variáveis: SYSFS (padrão /tmp/srv), CPUM (modelo da CPU), MEMF (meminfo falso),
#            GSIZE (tamanho da janela, padrão 1840x1040), FBIN (pasta com programas de mentira).
set -e
D=$(cd "$(dirname "$0")" && pwd)
ABA=$1; OUT=$2; ESPERA=${3:-8}; shift 3 || true
GSIZE=${GSIZE:-1840x1040}
BIN=${BIN:-$D/../../target/release/numapilot-gui}
mkdir -p /tmp/np-capt-run /tmp/np-capt-home
xvfb-run -a -s "-screen 0 2400x1500x24" bash -c "
  PATH=${FBIN:-/tmp/fakebin}:\$PATH NUMAPILOT_GUI_TAB=$ABA NUMAPILOT_GUI_SIZE=$GSIZE \
  NUMAPILOT_PROC_STAT=/tmp/fakeproc/stat NUMAPILOT_MEMINFO=${MEMF:-/tmp/fakeproc/meminfo} \
  NUMAPILOT_SYSFS=${SYSFS:-/tmp/srv} NUMAPILOT_CPU_MODEL='${CPUM:-Intel(R) Xeon(R) CPU E5-2696 v4 @ 2.20GHz}' \
  LIBGL_ALWAYS_SOFTWARE=1 WAYLAND_DISPLAY= XDG_RUNTIME_DIR=/tmp/np-capt-run HOME=/tmp/np-capt-home \
  '$BIN' & P=\$!
  sleep $ESPERA
  python3 '$D/entrada.py' $* move 2300 1400 sleep 1
  import -window root -crop ${GSIZE}+0+0 +repage /tmp/np-capt.png
  kill \$P"
"$D/moldura.sh" /tmp/np-capt.png "$OUT"
