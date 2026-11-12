//! Real measurement of memory latency and bandwidth between sockets (CPU-node × memory-node
//! matrix).

use crate::procfs;
use crate::topo::Topology;
use serde::{Deserialize, Serialize};
use std::hint::black_box;
use std::path::PathBuf;
use std::sync::{Arc, Barrier};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BenchResult {
    pub nodes: Vec<usize>,
    /// latency_ns[cpu_node][mem_node]
    pub latency_ns: Vec<Vec<f64>>,
    /// bandwidth_gbs[cpu_node][mem_node]
    pub bandwidth_gbs: Vec<Vec<f64>>,
    pub threads: usize,
    pub size_mb: usize,
    pub time: u64,
}

pub fn paths() -> Vec<PathBuf> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    vec![
        crate::learned::state_dir().join("bench.json"),
        home.join(".cache/numapilot/bench.json"),
    ]
}

/// The most recent result (the service's or the user's).
pub fn load() -> Option<BenchResult> {
    paths()
        .iter()
        .filter_map(|p| std::fs::read_to_string(p).ok())
        .filter_map(|s| serde_json::from_str::<BenchResult>(&s).ok())
        .max_by_key(|r| r.time)
}

pub fn save(r: &BenchResult) -> Option<PathBuf> {
    for p in paths() {
        if let Some(d) = p.parent()
            && std::fs::create_dir_all(d).is_ok()
            && std::fs::write(&p, serde_json::to_string_pretty(r).unwrap_or_default()).is_ok()
        {
            return Some(p);
        }
    }
    None
}

fn bind_here(cpu: usize, mem_node: usize) -> bool {
    procfs::set_affinity(0, &[cpu]).is_ok()
        && procfs::set_mempolicy(procfs::MPOL_BIND, &[mem_node]).is_ok()
}

/// Average latency (ns) of dependent accesses ("pointer chasing") in a random buffer.
fn latency(cpu: usize, mem_node: usize, size_mb: usize) -> f64 {
    std::thread::spawn(move || {
        if !bind_here(cpu, mem_node) {
            return f64::NAN;
        }
        let n = size_mb * 1024 * 1024 / 4;
        // Single random cycle (Sattolo) so the processor cannot predict it.
        let mut perm: Vec<u32> = (0..n as u32).collect();
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        for i in (1..n).rev() {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let j = (x % i as u64) as usize;
            perm.swap(i, j);
        }
        let mut next = vec![0u32; n];
        for i in 0..n {
            next[perm[i] as usize] = perm[(i + 1) % n];
        }
        drop(perm);
        let steps = 4_000_000usize;
        let mut p = 0u32;
        for _ in 0..200_000 {
            p = next[p as usize];
        }
        let t = Instant::now();
        for _ in 0..steps {
            p = next[p as usize];
        }
        black_box(p);
        t.elapsed().as_nanos() as f64 / steps as f64
    })
    .join()
    .unwrap_or(f64::NAN)
}

/// Read bandwidth (GB/s) with several threads on one node's CPUs reading another node's memory.
fn bandwidth(cpus: &[usize], mem_node: usize, size_mb: usize) -> f64 {
    let barrier = Arc::new(Barrier::new(cpus.len()));
    let handles: Vec<_> = cpus
        .iter()
        .map(|&cpu| {
            let b = barrier.clone();
            std::thread::spawn(move || -> Option<(f64, usize)> {
                let ok = bind_here(cpu, mem_node);
                let n = size_mb * 1024 * 1024 / 8;
                let v: Vec<u64> = (0..n as u64).collect();
                b.wait();
                if !ok {
                    return None;
                }
                let reps = 3;
                let t = Instant::now();
                let mut s = 0u64;
                for _ in 0..reps {
                    for chunk in v.as_chunks::<8>().0 {
                        s = s.wrapping_add(
                            chunk[0]
                                ^ chunk[1]
                                ^ chunk[2]
                                ^ chunk[3]
                                ^ chunk[4]
                                ^ chunk[5]
                                ^ chunk[6]
                                ^ chunk[7],
                        );
                    }
                }
                black_box(s);
                Some((t.elapsed().as_secs_f64(), reps * n * 8))
            })
        })
        .collect();
    let mut bytes = 0usize;
    let mut secs = 0f64;
    for h in handles {
        match h.join().ok().flatten() {
            Some((t, b)) => {
                bytes += b;
                secs = secs.max(t);
            }
            None => return f64::NAN,
        }
    }
    if secs > 0.0 {
        bytes as f64 / secs / 1e9
    } else {
        f64::NAN
    }
}

pub fn run(
    topo: &Topology,
    size_mb: usize,
    threads: usize,
    progress: impl Fn(&str),
) -> BenchResult {
    let nodes = topo.node_ids();
    let mut r = BenchResult {
        nodes: nodes.clone(),
        latency_ns: vec![vec![f64::NAN; nodes.len()]; nodes.len()],
        bandwidth_gbs: vec![vec![f64::NAN; nodes.len()]; nodes.len()],
        threads,
        size_mb,
        time: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
    };
    for (i, &cn) in nodes.iter().enumerate() {
        let node = topo.node(cn).unwrap();
        // One CPU per physical core, so two threads on the same core are not measured.
        let firsts: Vec<usize> = node.cores.iter().map(|c| c[0]).collect();
        let lat_cpu = firsts.first().copied().unwrap_or(node.cpus[0]);
        let bw_cpus: Vec<usize> = firsts.iter().copied().take(threads.max(1)).collect();
        for (j, &mn) in nodes.iter().enumerate() {
            progress(&crate::tf!(
                "CPUs of socket {cn} reading the memory of socket {mn}...",
                "CPUs do socket {cn} lendo a memória do socket {mn}..."
            ));
            r.latency_ns[i][j] = latency(lat_cpu, mn, size_mb);
            r.bandwidth_gbs[i][j] = bandwidth(&bw_cpus, mn, (size_mb / 2).max(16));
        }
    }
    r
}

pub fn print(r: &BenchResult) {
    let head: Vec<String> = r.nodes.iter().map(|n| format!("mem {n:>2}")).collect();
    println!(
        "{}",
        crate::tf!(
            "\nLatency (ns per access)      rows = socket's CPUs, columns = socket's memory",
            "\nLatência (ns por acesso)      linhas = CPUs do socket, colunas = memória do socket"
        )
    );
    println!("          {}", head.join("  "));
    for (i, n) in r.nodes.iter().enumerate() {
        let row: Vec<String> = r.latency_ns[i]
            .iter()
            .map(|v| format!("{v:>6.0}"))
            .collect();
        println!("CPU {n:>2}    {}", row.join("  "));
    }
    println!(
        "{}",
        crate::tf!(
            "\nRead bandwidth (GB/s, {} threads)",
            "\nBanda de leitura (GB/s, {} threads)",
            r.threads
        )
    );
    println!("          {}", head.join("  "));
    for (i, n) in r.nodes.iter().enumerate() {
        let row: Vec<String> = r.bandwidth_gbs[i]
            .iter()
            .map(|v| format!("{v:>6.1}"))
            .collect();
        println!("CPU {n:>2}    {}", row.join("  "));
    }
    if r.nodes.len() >= 2 {
        let local = r.latency_ns[0][0];
        let remote = r.latency_ns[0][1];
        if local.is_finite() && remote.is_finite() && local > 0.0 {
            println!(
                "{}",
                crate::tf!(
                    "\nAccessing the other socket's memory costs {:.0}% more latency.",
                    "\nAcessar a memória do outro socket custa {:.0}% a mais de latência.",
                    (remote / local - 1.0) * 100.0
                )
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiny_bench_on_this_machine() {
        // Minimal topology (CPU 0, node 0): only checks that it measures something plausible.
        let t = Topology {
            nodes: vec![crate::topo::Node {
                id: 0,
                cpus: vec![0],
                cores: vec![vec![0]],
                physical_cores: 1,
                sockets: Default::default(),
                mem_total_kb: 0,
                distances: vec![10],
            }],
            gpus: vec![],
            default_graphics: None,
        };
        let r = run(&t, 16, 1, |_| {});
        assert!(
            r.latency_ns[0][0].is_finite() && r.latency_ns[0][0] > 0.1,
            "{:?}",
            r.latency_ns
        );
        assert!(
            r.bandwidth_gbs[0][0].is_finite() && r.bandwidth_gbs[0][0] > 0.01,
            "{:?}",
            r.bandwidth_gbs
        );
    }
}
