//! numapilot: escolhe socket NUMA, memória e GPU para cada programa.
pub mod config;
pub mod containers;
pub mod bench;
pub mod cgroup;
pub mod ctl;
pub mod daemon;
pub mod decide;
pub mod desktop;
pub mod gpuctl;
pub mod labcfg;
pub mod learned;
pub mod procevents;
pub mod procfs;
pub mod profiles;
pub mod topo;
pub mod util;
