//! numapilot: escolhe socket NUMA, memória e GPU para cada programa.
pub mod config;
pub mod ctl;
pub mod daemon;
pub mod decide;
pub mod desktop;
pub mod learned;
pub mod procfs;
pub mod profiles;
pub mod topo;
pub mod util;
