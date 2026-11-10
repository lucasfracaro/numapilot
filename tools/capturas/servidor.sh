#!/usr/bin/env bash
# Servidor de mentira para capturas: 2× Xeon E5-2696 v4 (2×22 núcleos, 88 threads), 2× MI50 + RX 470
F=${1:-/tmp/srv}; rm -rf $F; mkdir -p $F/devices/system/cpu $F/devices/system/node
echo 0-87 > $F/devices/system/cpu/online; echo 0-87 > $F/devices/system/cpu/possible; echo 0-1 > $F/devices/system/node/online
for c in $(seq 0 87); do
  s=$(( (c/22)%2 )); core=$(( c%22 )); d=$F/devices/system/cpu/cpu$c
  mkdir -p $d/topology $d/cpufreq $d/cpuidle $d/power
  echo $core > $d/topology/core_id; echo $s > $d/topology/physical_package_id; echo 1 > $d/online
  sib=$(( c<44 ? c+44 : c-44 )); a=$(( c<sib ? c : sib )); b=$(( c<sib ? sib : c ))
  echo "$a,$b" > $d/topology/thread_siblings_list
  echo powersave > $d/cpufreq/scaling_governor; echo "performance powersave" > $d/cpufreq/scaling_available_governors
  echo intel_pstate > $d/cpufreq/scaling_driver; echo 1200000 > $d/cpufreq/cpuinfo_min_freq; echo 3700000 > $d/cpufreq/cpuinfo_max_freq
  echo 1200000 > $d/cpufreq/scaling_min_freq; echo 3700000 > $d/cpufreq/scaling_max_freq; echo $(( 2200000 + (c*37%15)*100000 )) > $d/cpufreq/scaling_cur_freq
  echo balance_performance > $d/cpufreq/energy_performance_preference; echo "default performance balance_performance balance_power power" > $d/cpufreq/energy_performance_available_preferences
  echo 6 > $d/power/energy_perf_bias
done
for n in 0 1; do
  d=$F/devices/system/node/node$n; mkdir -p $d
  if [ $n = 0 ]; then echo 0-21,44-65 > $d/cpulist; echo "10 21" > $d/distance; else echo 22-43,66-87 > $d/cpulist; echo "21 10" > $d/distance; fi
  printf "Node $n MemTotal:       65800000 kB\nNode $n MemFree:        21000000 kB\nNode $n FilePages:      18000000 kB\nNode $n Shmem:            600000 kB\nNode $n SReclaimable:    1500000 kB\n" > $d/meminfo
  printf "numa_hit 912345678\nnuma_miss %s\nnuma_foreign 1234\ninterleave_hit 4567\nlocal_node 912000000\nother_node %s\n" $((3000+n*900)) $((5000+n*1200)) > $d/numastat
done
mkgpu() { # pci dev card numa busy vramu vramt edge junc mem power cap capmax fanrpm pwm
  g=$F/bus/pci/devices/$1; mkdir -p $g/drm/$3 $g/drm/renderD$((128+${3#card})) $g/hwmon/hwmon${3#card}
  echo 0x038000 > $g/class; [ "$2" = 0x67df ] && echo 0x030000 > $g/class
  echo 0x1002 > $g/vendor; echo $2 > $g/device; echo $4 > $g/numa_node; echo $5 > $g/gpu_busy_percent
  echo $6 > $g/mem_info_vram_used; echo $7 > $g/mem_info_vram_total; echo 268435456 > $g/mem_info_vis_vram_total
  echo "8.0 GT/s PCIe" > $g/current_link_speed; echo 16 > $g/current_link_width; echo "8.0 GT/s PCIe" > $g/max_link_speed; echo 16 > $g/max_link_width
  echo auto > $g/power_dpm_force_performance_level
  printf "0: 808Mhz \n1: 1304Mhz *\n2: 1801Mhz \n" > $g/pp_dpm_sclk
  h=$g/hwmon/hwmon${3#card}; echo amdgpu > $h/name
  echo edge > $h/temp1_label; echo $8 > $h/temp1_input; echo junction > $h/temp2_label; echo $9 > $h/temp2_input; echo mem > $h/temp3_label; echo ${10} > $h/temp3_input
  echo 105000 > $h/temp1_crit; echo 110000 > $h/temp2_crit
  echo ${11} > $h/power1_average; echo ${12} > $h/power1_cap; echo ${13} > $h/power1_cap_max; echo 100000000 > $h/power1_cap_min; echo ${13} > $h/power1_cap_default
  echo ${14} > $h/fan1_input; echo 3850 > $h/fan1_max; echo 0 > $h/fan1_min; echo ${15} > $h/pwm1; echo 2 > $h/pwm1_enable; echo 255 > $h/pwm1_max; echo 0 > $h/pwm1_min
  echo 1801000000 > $h/freq1_input; echo sclk > $h/freq1_label
  cat > $g/pp_od_clk_voltage <<OD
OD_SCLK:
0: 808Mhz
1: 1801Mhz
OD_MCLK:
1: 1000Mhz
OD_VDDC_CURVE:
0: 808Mhz 721mV
1: 1304Mhz 797mV
2: 1801Mhz 1078mV
OD_RANGE:
SCLK:     808Mhz       2200Mhz
MCLK:     800Mhz       1200Mhz
VDDC_CURVE_SCLK[0]: 808Mhz       2200Mhz
VDDC_CURVE_VOLT[0]: 738mV        1218mV
VDDC_CURVE_SCLK[1]: 808Mhz       2200Mhz
VDDC_CURVE_VOLT[1]: 738mV        1218mV
VDDC_CURVE_SCLK[2]: 808Mhz       2200Mhz
VDDC_CURVE_VOLT[2]: 738mV        1218mV
OD
  mkdir -p $F/class/drm/$3; ln -sfn $g $F/class/drm/$3/device 2>/dev/null
}
mkgpu 0000:03:00.0 0x66a1 card1 0 95 12884901888 34342961152 70000 78000 74000 182000000 225000000 300000000 2100 110
mkgpu 0000:83:00.0 0x66a1 card2 1 37 6442450944 34342961152 63000 71000 67000 121000000 225000000 300000000 1800 100
mkgpu 0000:04:00.0 0x67df card0 0 8 1288490188 8589934592 48000 52000 50000 32000000 120000000 150000000 900 70
mkdir -p $F/class/drm/card0-HDMI-A-1; echo connected > $F/class/drm/card0-HDMI-A-1/status
# sensores do processador
for p in 0 1; do h=$F/class/hwmon/hwmon$((10+p)); mkdir -p $h; echo coretemp > $h/name; echo "Package id $p" > $h/temp1_label; echo $((54000+p*6000)) > $h/temp1_input; done
# RAPL
for p in 0 1; do r=$F/class/powercap/intel-rapl:$p; mkdir -p $r; echo package-$p > $r/name; echo $((123456789000+p*1000)) > $r/energy_uj; echo 262143328850 > $r/max_energy_range_uj; done
