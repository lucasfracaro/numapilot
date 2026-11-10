#!/usr/bin/env bash
# Hardware de mentira para as capturas (ver README.md desta pasta).
F=${1:-/tmp/bc}; rm -rf $F; mkdir -p $F/devices/system/cpu $F/devices/system/node/node0
echo 0-11 > $F/devices/system/cpu/online; echo 0 > $F/devices/system/node/online
for c in $(seq 0 11); do d=$F/devices/system/cpu/cpu$c; mkdir -p $d/topology; echo $((c%6)) > $d/topology/core_id; echo 0 > $d/topology/physical_package_id; echo "$((c%6)),$((c%6+6))" > $d/topology/thread_siblings_list; done
n=$F/devices/system/node/node0; echo 0-11 > $n/cpulist; echo 10 > $n/distance
printf "Node 0 MemTotal:       16100000 kB\nNode 0 MemFree:         6000000 kB\nNode 0 FilePages:       3000000 kB\nNode 0 Shmem:            200000 kB\nNode 0 SReclaimable:     300000 kB\n" > $n/meminfo
g=$F/bus/pci/devices/0000:01:00.0; mkdir -p $g/drm/card1 $g/drm/renderD128 $g/hwmon/hwmon2
echo 0x030000 > $g/class; echo 0x1002 > $g/vendor; echo 0x13fe > $g/device; echo -1 > $g/numa_node; echo 41 > $g/gpu_busy_percent
echo 2100000000 > $g/mem_info_vram_used; echo 8589934592 > $g/mem_info_vram_total
h=$g/hwmon/hwmon2; echo amdgpu > $h/name; echo edge > $h/temp1_label; echo 58000 > $h/temp1_input; echo 95000000 > $h/power1_input; echo 1500000000 > $h/freq1_input; echo sclk > $h/freq1_label
printf "0: 1000Mhz\n1: 1500Mhz *\n2: 2000Mhz\n" > $g/pp_dpm_sclk
mkdir -p $F/class/drm/card1-HDMI-A-1; echo connected > $F/class/drm/card1-HDMI-A-1/status
h=$F/class/hwmon/hwmon3; mkdir -p $h; echo k10temp > $h/name; echo Tctl > $h/temp1_label; echo 57000 > $h/temp1_input
