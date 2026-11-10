#!/usr/bin/env bash
# Hardware de mentira para as capturas (ver README.md desta pasta).
F=${1:-/tmp/note}; rm -rf $F; mkdir -p $F/devices/system/cpu $F/devices/system/node/node0
echo 0-15 > $F/devices/system/cpu/online; echo 0 > $F/devices/system/node/online
for c in $(seq 0 15); do d=$F/devices/system/cpu/cpu$c; mkdir -p $d/topology $d/cpufreq
  echo $((c%8)) > $d/topology/core_id; echo 0 > $d/topology/physical_package_id; echo "$((c%8)),$((c%8+8))" > $d/topology/thread_siblings_list
  echo powersave > $d/cpufreq/scaling_governor; echo "performance powersave" > $d/cpufreq/scaling_available_governors; echo amd-pstate-epp > $d/cpufreq/scaling_driver
  echo 400000 > $d/cpufreq/cpuinfo_min_freq; echo 4785000 > $d/cpufreq/cpuinfo_max_freq; echo 400000 > $d/cpufreq/scaling_min_freq; echo 4785000 > $d/cpufreq/scaling_max_freq
  echo $((1800000 + c*90000)) > $d/cpufreq/scaling_cur_freq; echo balance_performance > $d/cpufreq/energy_performance_preference
  echo "default performance balance_performance balance_power power" > $d/cpufreq/energy_performance_available_preferences
done
n=$F/devices/system/node/node0; echo 0-15 > $n/cpulist; echo 10 > $n/distance
printf "Node 0 MemTotal:       32000000 kB\nNode 0 MemFree:        12000000 kB\nNode 0 FilePages:       7000000 kB\nNode 0 Shmem:            400000 kB\nNode 0 SReclaimable:     600000 kB\n" > $n/meminfo
# Radeon 680M
g=$F/bus/pci/devices/0000:74:00.0; mkdir -p $g/drm/card1 $g/drm/renderD129 $g/hwmon/hwmon5
echo 0x030000 > $g/class; echo 0x1002 > $g/vendor; echo 0x1681 > $g/device; echo -1 > $g/numa_node; echo 9 > $g/gpu_busy_percent
echo 412000000 > $g/mem_info_vram_used; echo 536870912 > $g/mem_info_vram_total; echo 4500000000 > $g/mem_info_gtt_used; echo 16000000000 > $g/mem_info_gtt_total
h=$g/hwmon/hwmon5; echo amdgpu > $h/name; echo edge > $h/temp1_label; echo 54000 > $h/temp1_input; echo 21000000 > $h/power1_input; echo 2200000000 > $h/freq1_input
printf "0: 200Mhz\n1: 1500Mhz *\n2: 2200Mhz\n" > $g/pp_dpm_sclk
for o in eDP-2 DP-1 DP-2 HDMI-A-9; do mkdir -p $F/class/drm/card1-$o; done; echo connected > $F/class/drm/card1-eDP-2/status; for o in DP-1 DP-2; do echo disconnected > $F/class/drm/card1-$o/status; done
# RTX 3070 Ti Laptop
g=$F/bus/pci/devices/0000:01:00.0; mkdir -p $g/drm/card0 $g/drm/renderD128 $g/power
echo 0x030000 > $g/class; echo 0x10de > $g/vendor; echo 0x24a0 > $g/device; echo -1 > $g/numa_node; echo active > $g/power/runtime_status
mkdir -p $F/class/drm/card0-HDMI-A-1; echo connected > $F/class/drm/card0-HDMI-A-1/status
mkdir -p $F/module/nvidia; echo 615.78.08 > $F/module/nvidia/version
# bateria e tomada
b=$F/class/power_supply/BAT1; mkdir -p $b; echo Battery > $b/type; echo Discharging > $b/status; echo 86 > $b/capacity; echo 1 > $b/present
echo 57480000 > $b/energy_full_design; echo 49400000 > $b/energy_full; echo 42480000 > $b/energy_now; echo 18900000 > $b/power_now; echo 214 > $b/cycle_count
a=$F/class/power_supply/ADP1; mkdir -p $a; echo Mains > $a/type; echo 0 > $a/online
mkdir -p $F/class/dmi/id; echo 10 > $F/class/dmi/id/chassis_type; echo Acer > $F/class/dmi/id/sys_vendor; echo "Nitro AN515-46" > $F/class/dmi/id/product_name
mkdir -p $F/firmware/acpi; echo balanced > $F/firmware/acpi/platform_profile; echo "low-power balanced performance" > $F/firmware/acpi/platform_profile_choices
h=$F/class/hwmon/hwmon3; mkdir -p $h; echo k10temp > $h/name; echo Tctl > $h/temp1_label; echo 63000 > $h/temp1_input
