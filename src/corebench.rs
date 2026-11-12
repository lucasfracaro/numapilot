//! Latência do cache por núcleo: mede, em cada núcleo físico, o tempo de um acesso que cai no
//! cache L3 (fora do L1/L2 do próprio núcleo), e monta uma ordem do mais rápido ao mais lento.
//! Em processadores com muitos núcleos a posição no anel do L3 muda um pouco esse tempo; em
//! outros (núcleos preferidos da AMD, P/E da Intel) a diferença é maior. A ordem só é usada na
//! escolha de núcleos quando a diferença medida é maior que o ruído da própria medição.

use crate::topo::Topology;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct CoreRank {
    /// primeiro CPU do núcleo → nanossegundos por acesso (menor = melhor)
    pub ns: BTreeMap<usize, f32>,
    /// diferença entre o melhor e o pior núcleo, em % (o maior valor entre os processadores)
    pub spread_pct: f32,
    /// quanto a mesma medição varia sozinha, em %
    pub noise_pct: f32,
    /// a diferença é confiável o bastante para ordenar os núcleos?
    pub useful: bool,
    pub at: u64,
    /// modelo do processador medido (se trocar, a medição não vale mais)
    #[serde(default)]
    pub cpu: String,
}

pub fn path() -> PathBuf {
    crate::learned::state_dir().join("core-rank.json")
}

fn cache_bytes(cpu: usize, index: usize) -> Option<usize> {
    let s = crate::util::read_trim(crate::util::sys(&format!(
        "devices/system/cpu/cpu{cpu}/cache/index{index}/size"
    )))?;
    let n: usize = s.trim_end_matches(['K', 'M']).parse().ok()?;
    Some(if s.ends_with('M') { n << 20 } else { n << 10 })
}

/// Tamanho do teste: bem maior que o L2 do núcleo, cabendo com folga no L3.
pub fn test_bytes(l2: Option<usize>, l3: Option<usize>) -> usize {
    let l2 = l2.unwrap_or(512 << 10);
    let l3 = l3.unwrap_or(8 << 20);
    (l3 / 4).clamp(l2 * 4, 8 << 20).min(l3.max(l2 * 4))
}

/// Uma corrida de ponteiros aleatória no vetor (cada posição aponta para outra, num ciclo único):
/// o processador não consegue adivinhar o próximo endereço, então cada passo custa um acesso.
fn chase(buf: &[u32], steps: usize) -> f32 {
    let mut i = 0u32;
    let t = Instant::now();
    for _ in 0..steps {
        // cada elemento ocupa uma linha de cache (16 × u32 = 64 bytes)
        i = buf[(i as usize) << 4];
    }
    let ns = t.elapsed().as_nanos() as f32 / steps as f32;
    std::hint::black_box(i);
    ns
}

/// Ciclo único aleatório (Sattolo) com `lines` linhas de cache.
fn make_cycle(lines: usize) -> Vec<u32> {
    let mut perm: Vec<u32> = (0..lines as u32).collect();
    let mut x = 0x9E37_79B9_7F4A_7C15u64;
    for i in (1..lines).rev() {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let j = (x % i as u64) as usize;
        perm.swap(i, j);
    }
    let mut buf = vec![0u32; lines << 4];
    for (i, p) in perm.iter().enumerate() {
        buf[i << 4] = *p;
    }
    buf
}

/// Julga as medições: `rounds[cpu]` = tempos das rodadas daquele núcleo.
pub fn judge(rounds: &BTreeMap<usize, Vec<f32>>, groups: &[Vec<usize>]) -> CoreRank {
    let mut ns = BTreeMap::new();
    let mut noises = vec![];
    for (c, v) in rounds {
        let mut s = v.clone();
        s.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(best) = s.first() {
            ns.insert(*c, *best);
            if let Some(second) = s.get(1) {
                noises.push((second - best) / best * 100.0);
            }
        }
    }
    noises.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let noise_pct = noises.get(noises.len() / 2).copied().unwrap_or(100.0);
    // diferença dentro de cada processador (entre processadores não interessa aqui)
    let spread_pct = groups
        .iter()
        .filter_map(|g| {
            let v: Vec<f32> = g.iter().filter_map(|c| ns.get(c).copied()).collect();
            let min = v.iter().cloned().fold(f32::MAX, f32::min);
            let max = v.iter().cloned().fold(0.0, f32::max);
            (v.len() > 1 && min > 0.0).then(|| (max - min) / min * 100.0)
        })
        .fold(0.0, f32::max);
    CoreRank {
        useful: spread_pct >= 2.0 && spread_pct >= noise_pct * 3.0,
        ns,
        spread_pct,
        noise_pct,
        at: crate::history::now(),
        cpu: crate::recipes::cpu_model(),
    }
}

/// Mede todos os núcleos físicos (um thread de cada), em rodadas intercaladas para o
/// aquecimento e o turbo afetarem todos por igual. Leva alguns segundos; o PC deve estar parado.
pub fn measure(topo: &Topology, out: &mut dyn FnMut(&str)) -> Result<CoreRank, String> {
    let groups: Vec<Vec<usize>> = topo
        .nodes
        .iter()
        .map(|n| n.cores.iter().filter_map(|c| c.first().copied()).collect())
        .collect();
    let cpus: Vec<usize> = groups.iter().flatten().copied().collect();
    if cpus.len() < 2 {
        return Err(crate::i18n::tr("there is only one core: nothing to compare").into());
    }
    let bytes = test_bytes(cache_bytes(cpus[0], 2), cache_bytes(cpus[0], 3));
    let lines = bytes / 64;
    let buf = make_cycle(lines);
    let steps = (lines * 6).max(400_000);
    out(&crate::tf!(
        "Measuring {} cores, {} rounds, {} MB block (fits in the L3 cache)…",
        "Medindo {} núcleos, {} rodadas, bloco de {} MB (cabe no cache L3)…",
        cpus.len(),
        ROUNDS,
        bytes >> 20
    ));
    let before = crate::procfs::get_affinity(0);
    let mut rounds: BTreeMap<usize, Vec<f32>> = BTreeMap::new();
    for r in 0..ROUNDS {
        for &c in &cpus {
            if crate::procfs::set_affinity(0, &[c]).is_err() {
                continue;
            }
            // aquece: traz o bloco para o L3 a partir deste núcleo e acorda o clock
            chase(&buf, lines);
            rounds.entry(c).or_default().push(chase(&buf, steps));
        }
        out(&crate::tf!(
            "  round {} of {ROUNDS}",
            "  rodada {} de {ROUNDS}",
            r + 1
        ));
    }
    if let Some(b) = before {
        let _ = crate::procfs::set_affinity(0, &b);
    }
    if rounds.len() < 2 {
        return Err(
            crate::i18n::tr("could not pin the test to the cores (sched_setaffinity)").into(),
        );
    }
    Ok(judge(&rounds, &groups))
}

const ROUNDS: usize = 4;

/// Ordem salva (se existir, for deste processador e a diferença for confiável). Relê quando o
/// arquivo muda.
pub fn saved() -> Option<CoreRank> {
    static CACHE: Mutex<Option<(std::time::SystemTime, Option<CoreRank>)>> = Mutex::new(None);
    if std::env::var_os("NUMAPILOT_SYSFS").is_some() {
        return None;
    }
    let p = path();
    let mtime = std::fs::metadata(&p).ok()?.modified().ok()?;
    let mut g = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((m, r)) = g.as_ref()
        && *m == mtime
    {
        return r.clone();
    }
    let r: Option<CoreRank> = std::fs::read_to_string(&p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .filter(|r: &CoreRank| r.cpu == crate::recipes::cpu_model());
    *g = Some((mtime, r.clone()));
    r
}

/// Tempo de um núcleo para ordenar (só quando a medição é confiável).
pub fn ns_of(first_cpu: usize) -> Option<f32> {
    let r = saved()?;
    if !r.useful {
        return None;
    }
    r.ns.get(&first_cpu).copied()
}

/// Ordena núcleos (cada um com seus threads) do mais rápido ao mais lento; sem medição
/// confiável, a ordem fica como está. Diferenças menores que 1% não mudam a ordem.
pub fn sort_units(units: &mut [Vec<usize>], ns: &dyn Fn(usize) -> Option<f32>) {
    let key = |u: &Vec<usize>| u.first().and_then(|c| ns(*c));
    if units.iter().any(|u| key(u).is_none()) {
        return;
    }
    let best = units.iter().filter_map(key).fold(f32::MAX, f32::min);
    // degraus de 1% sobre o melhor: núcleos praticamente iguais mantêm a ordem original
    units.sort_by_key(|u| ((key(u).unwrap_or(best) / best - 1.0) * 100.0) as i32);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn judge_and_sort() {
        let mut r = BTreeMap::new();
        r.insert(0, vec![10.0, 10.1, 10.3]);
        r.insert(1, vec![10.9, 10.8, 11.0]);
        r.insert(2, vec![10.05, 10.2, 10.1]);
        let j = judge(&r, &[vec![0, 1, 2]]);
        assert!(j.useful, "{j:?}");
        assert!((j.spread_pct - 8.0).abs() < 0.2);
        // ruído grande: não confia
        let mut n = BTreeMap::new();
        n.insert(0, vec![10.0, 11.0]);
        n.insert(1, vec![10.3, 11.5]);
        assert!(!judge(&n, &[vec![0, 1]]).useful);

        let ns = |c: usize| j.ns.get(&c).copied();
        let mut u = vec![vec![1, 5], vec![0, 4], vec![2, 6]];
        sort_units(&mut u, &ns);
        // 0 (10.0) e 2 (10.05) empatam no degrau de 1% e mantêm a ordem; 1 (10.8) vai para o fim
        assert_eq!(u, vec![vec![0, 4], vec![2, 6], vec![1, 5]]);
        let mut v = vec![vec![1], vec![9]];
        sort_units(&mut v, &ns);
        assert_eq!(v, vec![vec![1], vec![9]]);
        assert_eq!(test_bytes(Some(256 << 10), Some(55 << 20)), 8 << 20);
        assert_eq!(test_bytes(Some(512 << 10), Some(4 << 20)), 2 << 20);
    }

    #[test]
    fn chase_runs() {
        let b = make_cycle(1024);
        // ciclo único: depois de 1024 passos volta ao início
        let mut i = 0u32;
        for _ in 0..1024 {
            i = b[(i as usize) << 4];
        }
        assert_eq!(i, 0);
        assert!(chase(&b, 10_000) > 0.0);
    }
}
