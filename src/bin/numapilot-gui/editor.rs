//! Janela "Ajustar programa": onde no hardware ele roda (núcleos, memória, prioridade, GPU),
//! com perfis prontos e avisos de compatibilidade para jogos.

use crate::App;
use crate::map::core_map;
use eframe::egui::{self, Color32, RichText};
use numapilot::ctl::Request;
use numapilot::decide::{LaunchInput, decide_launch, game_warnings};
use numapilot::learned::Learned;
use numapilot::profiles::{Profile, Profiles};
use numapilot::util::{format_cpulist, parse_cpulist};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone)]
pub struct Editor {
    pub key: String,
    pub label: String,
    pub main_pid: Option<i32>,
    pub current_cpus: String,
    pub mem: Vec<u64>,
    pub p: Profile,
    pub picking: bool,
    pub pick: BTreeSet<usize>,
}

impl Editor {
    pub fn new(
        key: &str,
        label: &str,
        main_pid: Option<i32>,
        cpus: &str,
        mem: &[u64],
        p: Profile,
    ) -> Editor {
        let pick: BTreeSet<usize> = p
            .cpu_list
            .as_deref()
            .map(parse_cpulist)
            .unwrap_or_default()
            .into_iter()
            .collect();
        Editor {
            key: key.into(),
            label: label.into(),
            main_pid,
            current_cpus: cpus.into(),
            mem: mem.to_vec(),
            picking: !pick.is_empty(),
            pick,
            p: Profile {
                label: label.into(),
                ..p
            },
        }
    }
}

const MEM_OPTS: &[(&str, &str, &str)] = &[
    (
        "auto",
        "Automático",
        "o numapilot decide (normalmente: memória do socket dos núcleos)",
    ),
    (
        "strict",
        "Só do socket (estrito)",
        "menor latência; se a memória do socket encher, o programa sofre",
    ),
    (
        "prefer",
        "Preferir a local",
        "usa a do outro socket só quando faltar",
    ),
    (
        "interleave",
        "Intercalar entre sockets",
        "mais banda total; bom para render/IA na CPU (vale ao reabrir)",
    ),
    ("free", "Livre", "não move a memória"),
];

fn upscale_ui(app: &mut App, ui: &mut egui::Ui, ed: &mut Editor) {
    use numapilot::upscale;
    ui.label(RichText::new("Upscaling").strong());
    // placa em que o jogo vai rodar → o que funciona nela
    let gpu = app
        .topo
        .gpus
        .iter()
        .find(|g| g.alias == ed.p.gpu || g.pci == ed.p.gpu)
        .or_else(|| app.topo.gaming_gpu())
        .cloned();
    let rec = numapilot::recipes::load();
    let sug: Vec<&str> = gpu
        .as_ref()
        .and_then(|g| rec.arch_for(g))
        .map(|a| upscale::suggested(&a.upscale))
        .unwrap_or_default();
    if let (Some(g), Some(a)) = (gpu.as_ref(), gpu.as_ref().and_then(|g| rec.arch_for(g))) {
        ui.label(
            RichText::new(format!(
                "Na {} ({}), funcionam: {}",
                g.alias,
                a.family,
                sug.join(", ")
            ))
            .small()
            .weak(),
        );
    }
    let mut u =
        ed.p.upscale
            .clone()
            .unwrap_or_else(|| numapilot::profiles::Upscale {
                mode: "nenhum".into(),
                ..Default::default()
            });
    let cur = upscale::MODES
        .iter()
        .find(|m| m.0 == u.mode)
        .map(|m| m.1)
        .unwrap_or("Nenhum");
    egui::ComboBox::from_id_salt("ed-up")
        .selected_text(cur)
        .width(240.0)
        .show_ui(ui, |ui| {
            for (k, l, d) in upscale::MODES {
                let ok = *k == "nenhum" || sug.contains(k);
                let label = if ok {
                    l.to_string()
                } else {
                    format!("{l} (não indicado para esta placa)")
                };
                ui.selectable_value(&mut u.mode, k.to_string(), label)
                    .on_hover_text(*d);
            }
        });
    if u.mode != "nenhum" {
        ui.label(
            RichText::new(
                upscale::MODES
                    .iter()
                    .find(|m| m.0 == u.mode)
                    .map(|m| m.2)
                    .unwrap_or(""),
            )
            .small()
            .weak(),
        );
        if u.mode != "optiscaler" {
            ui.label(RichText::new("Precisa do proton-cachyos ou do GE-Proton como ferramenta de compatibilidade do jogo.").small().weak());
        }
        ui.horizontal(|ui| {
            ui.checkbox(&mut u.indicator, "mostrar a marca do FSR 4/DLSS (para conferir)");
            if u.mode == "optiscaler" {
                ui.checkbox(&mut u.int8, "forçar FSR 4 INT8 (experimental)")
                    .on_hover_text("Para placas sem FSR 4 oficial (RDNA 2/3.5, Intel, NVIDIA). Placas sem INT8 não ganham FSR 4.");
            }
        });
    }
    if u.mode == "optiscaler" {
        ui.label(
            RichText::new("Não use o OptiScaler em jogos online: o anti-cheat pode banir.")
                .small()
                .color(Color32::from_rgb(230, 120, 60)),
        );
        if let Some(id) = ed.key.strip_prefix("steam:") {
            let dir = upscale::steam_game_root(id).and_then(|r| upscale::exe_dir(&r));
            ui.horizontal(|ui| {
                match dir.as_ref().and_then(|d| upscale::installed(d)) {
                    Some(i) => {
                        ui.label(RichText::new(format!("OptiScaler {} instalado", i.version)).color(Color32::from_rgb(110, 190, 120)));
                        if ui.button("Remover do jogo").clicked() {
                            app.start_job("OptiScaler", vec![crate::jobs::sibling("numapilot"), "upscale".into(), "remove".into(), id.into()]);
                        }
                        if ui.button("Reinstalar (versão nova)").clicked() {
                            let mut v = vec![crate::jobs::sibling("numapilot"), "upscale".into(), "install".into(), id.into()];
                            if u.int8 {
                                v.push("--int8".into());
                            }
                            app.start_job("OptiScaler", v);
                        }
                    }
                    None if dir.is_some() => {
                        if ui.button(RichText::new("Instalar no jogo").strong()).on_hover_text("Baixa a versão estável mais nova e copia para a pasta do executável, guardando o que for do jogo").clicked() {
                            let mut v = vec![crate::jobs::sibling("numapilot"), "upscale".into(), "install".into(), id.into()];
                            if u.int8 {
                                v.push("--int8".into());
                            }
                            app.start_job("OptiScaler", v);
                        }
                    }
                    None => {
                        ui.label(RichText::new("Pasta do jogo não encontrada nas bibliotecas da Steam.").weak());
                    }
                }
            });
            if let Some(d) = dir {
                ui.label(
                    RichText::new(format!("Pasta: {}", d.display()))
                        .small()
                        .weak(),
                );
            }
        } else {
            ui.label(RichText::new("Para instalar pela interface, abra o jogo pela Steam com o numapilot (o perfil fica ligado ao appid).").small().weak());
        }
    }
    let new = if u.mode == "nenhum" && !u.indicator {
        None
    } else {
        Some(u)
    };
    if new != ed.p.upscale {
        ed.p.upscale = new;
    }
}

fn tuner_ui(app: &mut App, ui: &mut egui::Ui, ed: &mut Editor) {
    use numapilot::tuner::{self, Tuner};
    let mut t = Tuner::load();
    ui.label(RichText::new("Otimizar FPS").strong());
    let steam_id = ed.key.strip_prefix("steam:").map(String::from);
    let mut save = false;
    let mut apply_best = None;
    match t.sessions.get(&ed.key).cloned() {
        None => {
            ui.label(
                RichText::new(
                    "Testa algumas configurações (núcleos, HT, socket, placa), uma a cada vez que você abre o jogo, \
                     mede com o MangoHud (FPS médio e 1% low) e mostra a melhor.",
                )
                .weak(),
            );
            if !app.has_mangohud {
                ui.label(
                    RichText::new("Precisa do MangoHud: sudo pacman -S mangohud")
                        .color(Color32::from_rgb(230, 160, 40)),
                );
            }
            if ui.button("Começar a otimização").clicked() {
                let v = tuner::variants(&app.topo, &ed.p);
                t.start(&ed.key, &ed.label, v);
                save = true;
            }
        }
        Some(s) => {
            let best = s.best();
            egui::Grid::new("ed-tune")
                .striped(true)
                .num_columns(4)
                .spacing([12.0, 3.0])
                .show(ui, |ui| {
                    for h in ["", "Configuração", "FPS médio", "1% low"] {
                        ui.label(RichText::new(h).strong());
                    }
                    ui.end_row();
                    for (i, v) in s.variants.iter().enumerate() {
                        ui.label(if best == Some(i) {
                            "melhor"
                        } else if i == s.next {
                            "próximo"
                        } else {
                            ""
                        });
                        ui.label(&v.label);
                        match &v.result {
                            Some(r) => {
                                ui.label(format!("{:.1}", r.avg));
                                ui.label(format!("{:.1}", r.low1));
                            }
                            None => {
                                ui.label("—");
                                ui.label("—");
                            }
                        }
                        ui.end_row();
                    }
                });
            if let Some(v) = s.current() {
                ui.label(format!(
                    "Próximo teste ({}/{}): {}. Abra o jogo, vá para uma cena pesada e jogue uns {} s; depois feche.",
                    s.next + 1,
                    s.variants.len(),
                    v.label,
                    s.seconds + 10
                ));
            } else {
                ui.label(RichText::new("Todos os testes terminaram.").strong());
            }
            ui.horizontal(|ui| {
                if let Some(id) = &steam_id
                    && !s.done()
                    && ui.button("Abrir o jogo").clicked()
                {
                    let _ = crate::jobs::spawn_detached(&[
                        "xdg-open".into(),
                        format!("steam://rungameid/{id}"),
                    ]);
                }
                if let Some(b) = best
                    && ui.button(RichText::new("Usar a melhor").strong()).clicked()
                {
                    apply_best = Some(s.variants[b].profile.clone());
                }
                if ui.button("Recomeçar").clicked() {
                    let v = tuner::variants(&app.topo, &ed.p);
                    t.start(&ed.key, &ed.label, v);
                    save = true;
                }
                if ui.button("Cancelar").clicked() {
                    t.sessions.remove(&ed.key);
                    save = true;
                }
            });
            if steam_id.is_none() {
                ui.label(
                    RichText::new("Abra pelo atalho do menu (numapilot run) para o teste valer.")
                        .small()
                        .weak(),
                );
            }
        }
    }
    if save && let Err(e) = t.save() {
        app.toast(format!("otimizador: {e}"), false);
    }
    if let Some(mut p) = apply_best {
        p.label = ed.label.clone();
        p.gamescope = ed.p.gamescope.clone();
        ed.p = p.clone();
        app.send(Request::SetProfile {
            key: ed.key.clone(),
            profile: p,
            pid: ed.main_pid,
        });
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut ed) = app.editor.take() else {
        return;
    };
    let mut open = true;
    let mut apply = false;
    let mut fix = false;
    egui::Window::new(format!("Ajustar: {}", ed.label))
        .id(egui::Id::unique("numapilot-editor"))
        .open(&mut open)
        .default_size([820.0, 640.0])
        .resizable(true)
        .collapsible(false)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new(&ed.key).small().weak());
                // Situação atual
                let tot: u64 = ed.mem.iter().sum();
                let cur = parse_cpulist(&ed.current_cpus);
                let local_nodes = app.topo.nodes_of(&cur);
                let local: u64 = ed.mem.iter().enumerate().filter(|(n, _)| local_nodes.contains(n)).map(|(_, v)| *v).sum();
                ui.label(format!(
                    "Agora: {} CPUs ({}) · memória {:.1} GB{}",
                    cur.len(),
                    if ed.current_cpus.is_empty() { "todos".to_string() } else { ed.current_cpus.clone() },
                    tot as f64 / 1024.0,
                    if tot > 0 { format!(", {:.0}% no socket dos núcleos", local as f64 * 100.0 / tot as f64) } else { String::new() }
                ));
                ui.add_space(4.0);

                // Histórico: quanto o programa usou nas últimas horas (o serviço guarda)
                {
                    let id = ui.make_persistent_id(("prog-hist", &ed.key));
                    let pts: Vec<numapilot::proghist::Point> = ui.data_mut(|d| {
                        d.get_temp_mut_or_insert_with(id, || numapilot::proghist::load(&ed.key))
                            .clone()
                    });
                    if let Some(s) = numapilot::proghist::summary(&pts) {
                        let p = crate::theme::pal(ui);
                        ui.horizontal(|ui| {
                            let mut sp = crate::theme::Spark::default();
                            let peak = s.peak.max(0.1);
                            let step = (pts.len() as f32 / crate::theme::Spark::LEN as f32).max(1.0);
                            let mut i = 0.0f32;
                            while (i as usize) < pts.len() {
                                sp.push(pts[i as usize].peak / peak);
                                i += step;
                            }
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(140.0, 30.0), egui::Sense::hover());
                            crate::theme::spark_in(ui.painter(), rect, &sp, p.accent);
                            let h = s.minutes / 60;
                            let m = s.minutes % 60;
                            ui.label(
                                RichText::new(format!(
                                    "Uso registrado ({}{m} min): em média {:.1} núcleos, pico {:.1}; memória no pico {}",
                                    if h > 0 { format!("{h} h ") } else { String::new() },
                                    s.avg,
                                    s.peak,
                                    crate::fmt_bytes(s.mem_peak_mb as u64 * 1048576)
                                ))
                                .small(),
                            )
                            .on_hover_text("Das últimas 6 horas em que o programa esteve aberto, um ponto por minuto. Use para escolher o limite de memória e de processador com folga sobre o pico.");
                        });
                        ui.add_space(4.0);
                    }
                }
                // Perfis prontos
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Perfis prontos:").strong());
                    for (id, name, tip) in [
                        ("jogo", "Jogo", "socket da GPU, 16 núcleos físicos, memória só local, prioridade alta, núcleos reservados, o jogo enxerga só esses núcleos"),
                        ("ia", "IA", "socket da GPU de computação, memória preferindo a local"),
                        ("render", "Renderização", "os 2 sockets, memória intercalada (banda máxima)"),
                        ("turbo", "Turbo máximo", "4 núcleos físicos sozinhos num processador: o resto do sistema vai para o outro socket e o clock sobe ao degrau mais alto do turbo (precisa de 2 processadores; confira em Mapa → Turbo)"),
                        ("video", "Edição de vídeo", "o socket da GPU inteiro, com os dois threads de cada núcleo, memória preferindo a local"),
                        ("fundo", "Segundo plano", "no máximo um núcleo de processador e prioridade baixa: para launchers, sincronizadores e afins"),
                        ("silencioso", "Silencioso", "4 núcleos e prioridade baixa: não atrapalha o resto"),
                    ] {
                        if ui.button(name).on_hover_text(tip).clicked() {
                            ed.p = Profile::preset(id, &ed.p);
                            ed.picking = false;
                            ed.pick.clear();
                        }
                    }
                    if ui.button("Tudo automático").clicked() {
                        ed.p = Profile { label: ed.label.clone(), ..Default::default() };
                        ed.picking = false;
                        ed.pick.clear();
                    }
                });
                ui.separator();

                egui::Grid::new("ed").num_columns(2).spacing([14.0, 10.0]).show(ui, |ui| {
                    ui.label(RichText::new("Processadores").strong());
                    let txt = app.cpu_opts.iter().find(|(v, _)| *v == ed.p.cpu).map(|(_, t)| t.clone()).unwrap_or(ed.p.cpu.clone());
                    ui.add_enabled_ui(!ed.picking, |ui| {
                        egui::ComboBox::from_id_salt("ed-cpu").selected_text(txt).width(240.0).show_ui(ui, |ui| {
                            for (v, t) in &app.cpu_opts {
                                ui.selectable_value(&mut ed.p.cpu, v.clone(), t);
                            }
                        });
                    });
                    ui.end_row();

                    ui.label(RichText::new("Núcleos").strong());
                    ui.vertical(|ui| {
                        ui.add_enabled_ui(!ed.picking, |ui| {
                            ui.horizontal(|ui| {
                                let mut lim = ed.p.cores.is_some();
                                if ui.checkbox(&mut lim, "limitar a").changed() {
                                    ed.p.cores = if lim { Some(16) } else { None };
                                }
                                if let Some(c) = ed.p.cores.as_mut() {
                                    ui.add(egui::Slider::new(c, 1..=app.topo.all_cpus().len()).suffix(" CPUs"));
                                }
                            });
                            ui.checkbox(&mut ed.p.smt, "usar hyper-threading (2 threads por núcleo)")
                                .on_hover_text("Desligado: 1 thread por núcleo físico. Em jogos costuma dar mais FPS e menos travadas.");
                        });
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut ed.picking, "escolher os núcleos no mapa").changed() && ed.picking && ed.pick.is_empty() {
                                ed.pick.extend(cur.iter().copied());
                            }
                            if ed.picking {
                                ui.label(RichText::new(format!("{} CPUs selecionados", ed.pick.len())).weak());
                                if ui.small_button("limpar").clicked() {
                                    ed.pick.clear();
                                }
                            }
                        });
                    });
                    ui.end_row();

                    ui.label(RichText::new("O programa enxerga").strong());
                    ui.checkbox(&mut ed.p.limit_visible, "só os núcleos liberados")
                        .on_hover_text(
                            "Jogos do Proton/Wine (WINE_CPU_TOPOLOGY), OpenMP, MKL, OpenBLAS, .NET, Java, Go...\n\
                             Evita jogos e o Easy Anti-Cheat recusarem abrir por ver núcleos demais. Vale ao abrir.",
                        );
                    ui.end_row();

                    ui.label(RichText::new("Memória").strong());
                    let multi = app.topo.nodes.len() > 1;
                    ui.vertical(|ui| {
                        // Com um processador só, não há memória "do outro socket".
                        for (v, t, d) in MEM_OPTS.iter().filter(|_| multi) {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut ed.p.memory, v.to_string(), *t);
                                ui.label(RichText::new(*d).small().weak());
                            });
                        }
                        if !multi {
                            ui.label(
                                RichText::new("Este PC tem um processador só: toda a memória já é a mais próxima dele (as opções de memória local aparecem nos PCs com 2 processadores).")
                                    .small()
                                    .weak(),
                            );
                        }
                        ui.horizontal(|ui| {
                            let mut lim = ed.p.mem_max_gb.is_some();
                            if ui
                                .checkbox(&mut lim, "Limitar a memória do programa a")
                                .on_hover_text("Passou do limite, o sistema tira memória dele (vai para o swap) em vez de travar o PC todo.")
                                .changed()
                            {
                                ed.p.mem_max_gb = if lim { Some(16.0) } else { None };
                            }
                            let nodes_kb = app.topo.nodes.iter().map(|n| n.mem_total_kb).sum::<u64>();
                            let total_kb = if nodes_kb > 0 { nodes_kb } else { crate::mem_system().1 };
                            let max_gb = (total_kb as f64 / 1048576.0).max(1.0);
                            if let Some(g) = ed.p.mem_max_gb.as_mut() {
                                *g = g.min(max_gb);
                            }
                            let now = app.prog_mem.get(&ed.key).copied().unwrap_or(0);
                            if let Some(g) = ed.p.mem_max_gb.as_mut() {
                                ui.add(egui::Slider::new(g, 0.5..=max_gb).suffix(" GB").step_by(0.5));
                            }
                            if now > 0 {
                                ui.label(RichText::new(format!("agora usa {}", crate::fmt_bytes(now))).small().weak());
                            }
                        });
                    });
                    ui.end_row();

                    ui.label(RichText::new("Limite de processador").strong());
                    ui.horizontal(|ui| {
                        let mut lim = ed.p.cpu_max_pct.is_some();
                        if ui
                            .checkbox(&mut lim, "Usar no máximo")
                            .on_hover_text("Teto de uso somando todos os núcleos: 50% = meio núcleo, 200% = dois núcleos. Bom para programas de fundo.")
                            .changed()
                        {
                            ed.p.cpu_max_pct = lim.then_some(100.0);
                        }
                        if let Some(c) = ed.p.cpu_max_pct.as_mut() {
                            ui.add(
                                egui::Slider::new(c, 5.0..=1600.0)
                                    .logarithmic(true)
                                    .suffix("%")
                                    .fixed_decimals(0),
                            );
                            ui.label(RichText::new(format!("= {:.1} núcleo(s)", *c / 100.0)).small().weak());
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Prioridade").strong());
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut ed.p.priority, "low".to_string(), "baixa");
                        ui.radio_value(&mut ed.p.priority, "normal".to_string(), "normal");
                        ui.radio_value(&mut ed.p.priority, "high".to_string(), "alta");
                        ui.separator();
                        ui.checkbox(&mut ed.p.exclusive, "núcleos exclusivos")
                            .on_hover_text("Enquanto ele roda, o resto do sistema não usa esses núcleos.");
                        if multi {
                            ui.checkbox(&mut ed.p.turbo_max, "turbo máximo").on_hover_text(
                                "Enquanto ele roda, o resto do sistema sai do processador inteiro onde ele está. \
                                 Com os outros núcleos dormindo, o clock dos núcleos dele sobe mais (veja a tabela em Mapa → Turbo). \
                                 Funciona melhor com poucos núcleos (2 a 8) e os estados de economia profundos ligados.",
                            );
                        }
                        ui.checkbox(&mut ed.p.low_latency, "baixa latência").on_hover_text(
                            "Enquanto ele roda: os núcleos dele não entram nos estados de economia profundos e o uncore do socket \
                             fica no máximo (menos travadinhas). Ajuste em Máquina → Latência em jogos.",
                        );
                    });
                    ui.end_row();

                    if multi && ed.p.turbo_max {
                        ui.label(RichText::new("Núcleos no turbo").strong());
                        ui.vertical(|ui| {
                            let per_socket = app
                                .topo
                                .nodes
                                .iter()
                                .map(|n| n.physical_cores.max(n.cores.len()))
                                .min()
                                .unwrap_or(0);
                            let table = app.turbo_tables.values().next();
                            let ghz = |n: usize| {
                                table
                                    .and_then(|t| numapilot::turbo::limit_for(t, n as f32))
                                    .map(|m| format!(" · {:.1} GHz", m as f64 / 1000.0))
                                    .unwrap_or_default()
                            };
                            ui.horizontal_wrapped(|ui| {
                                for n in [2usize, 4, 6, 8, 10, 12, 16] {
                                    if n > per_socket {
                                        continue;
                                    }
                                    let sel = ed.p.cores == Some(n) && !ed.p.smt;
                                    if ui
                                        .selectable_label(sel, format!("{n}{}", ghz(n)))
                                        .on_hover_text(format!(
                                            "{n} núcleos físicos (sem dividir núcleo) sozinhos no processador"
                                        ))
                                        .clicked()
                                    {
                                        ed.p.cores = Some(n);
                                        ed.p.smt = false;
                                        ed.p.cpu_list = None;
                                        ed.picking = false;
                                        ed.pick.clear();
                                    }
                                }
                            });
                            let all = ghz(per_socket);
                            ui.label(
                                RichText::new(if table.is_some() {
                                    format!(
                                        "Clock máximo com esse número de núcleos acordados (todos os {per_socket} ligados{all}). \
                                         Jogos que usam muitos núcleos rendem mais com 6 ou 8 do que com 4."
                                    )
                                } else {
                                    "Quanto menos núcleos, mais alto o clock. A tabela deste processador aparece com o serviço rodando (só Intel).".to_string()
                                })
                                .small()
                                .weak(),
                            );
                        });
                        ui.end_row();
                    }

                    ui.label(RichText::new("Modo da máquina").strong())
                        .on_hover_text("Enquanto o programa está aberto a máquina fica neste modo (aba Máquina) e volta ao anterior quando ele fecha.");
                    let modes = [(None, "não trocar"), (Some("jogo"), "Jogo"), (Some("ia"), "IA"), (Some("silencioso"), "Silencioso")];
                    let cur = modes.iter().find(|m| m.0 == ed.p.machine_mode.as_deref()).map(|m| m.1).unwrap_or("?");
                    egui::ComboBox::from_id_salt("ed-mmode").selected_text(cur).show_ui(ui, |ui| {
                        for (k, l) in modes {
                            ui.selectable_value(&mut ed.p.machine_mode, k.map(String::from), l);
                        }
                    });
                    ui.end_row();

                    ui.label(RichText::new("Placa de vídeo").strong());
                    ui.vertical(|ui| {
                        let cur = (ed.p.gpu.clone(), ed.p.usage.clone());
                        let mut sel = cur.clone();
                        let gtxt = app
                            .gpu_opts
                            .iter()
                            .find(|(g, u, _)| *g == cur.0 && (*u == cur.1 || cur.0 == "auto" || cur.0 == "none"))
                            .map(|(_, _, t)| t.clone())
                            .unwrap_or_else(|| format!("{} ({})", cur.0, cur.1));
                        egui::ComboBox::from_id_salt("ed-gpu").selected_text(gtxt).width(300.0).show_ui(ui, |ui| {
                            for (g, u, t) in &app.gpu_opts {
                                ui.selectable_value(&mut sel, (g.clone(), u.clone()), t);
                            }
                        });
                        if sel != cur {
                            ed.p.gpu = sel.0;
                            ed.p.usage = sel.1;
                        }
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Perfil de energia da placa:").small());
                            let names = [
                                ("auto", "não mexer"),
                                ("3d", "3D tela cheia (jogo)"),
                                ("compute", "computação (IA/ROCm)"),
                                ("vr", "VR"),
                                ("video", "vídeo"),
                                ("economia", "economia"),
                            ];
                            let cur = names.iter().find(|n| n.0 == ed.p.gpu_power).map(|n| n.1).unwrap_or("?");
                            egui::ComboBox::from_id_salt("ed-gpow").selected_text(cur).show_ui(ui, |ui| {
                                for (k, l) in names {
                                    ui.selectable_value(&mut ed.p.gpu_power, k.to_string(), l);
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Lossless Scaling:").small());
                            egui::ComboBox::from_id_salt("ed-fg").selected_text(if ed.p.fg_gpu == "same" { "mesma GPU".to_string() } else { ed.p.fg_gpu.clone() }).show_ui(ui, |ui| {
                                ui.selectable_value(&mut ed.p.fg_gpu, "same".to_string(), "mesma GPU do jogo");
                                for g in &app.topo.gpus {
                                    ui.selectable_value(&mut ed.p.fg_gpu, g.alias.clone(), format!("{} (dual GPU, fork)", g.alias));
                                }
                            });
                        });
                    });
                    ui.end_row();
                });

                if ed.picking {
                    ui.separator();
                    ui.label(RichText::new("Clique num núcleo para usar os 2 threads; botão direito = só 1 thread.").weak());
                    let who = BTreeMap::new();
                    core_map(ui, &app.topo, &app.cpu_loads, Some(&mut ed.pick), &who);
                    ed.p.cpu_list = if ed.pick.is_empty() { None } else { Some(format_cpulist(&ed.pick.iter().copied().collect::<Vec<_>>())) };
                } else {
                    ed.p.cpu_list = None;
                }

                // gamescope
                ui.separator();
                let mut gs_on = ed.p.gamescope.is_some();
                ui.horizontal(|ui| {
                    ui.checkbox(&mut gs_on, RichText::new("Abrir dentro do gamescope").strong());
                    if !app.has_gamescope {
                        ui.label(RichText::new("(não instalado: sudo pacman -S gamescope)").small().color(Color32::from_rgb(230, 160, 40)));
                    }
                });
                if gs_on != ed.p.gamescope.is_some() {
                    ed.p.gamescope = gs_on.then(numapilot::profiles::Gamescope::default);
                }
                if let Some(g) = ed.p.gamescope.as_mut() {
                    let opt_num = |ui: &mut egui::Ui, v: &mut Option<u32>, def: u32, max: u32| {
                        let mut on = v.is_some();
                        let mut x = v.unwrap_or(def);
                        ui.checkbox(&mut on, "");
                        ui.add_enabled(on, egui::DragValue::new(&mut x).range(1..=max));
                        *v = on.then_some(x);
                    };
                    egui::Grid::new("ed-gs").num_columns(2).spacing([10.0, 4.0]).show(ui, |ui| {
                        ui.label("Resolução do jogo");
                        ui.horizontal(|ui| {
                            opt_num(ui, &mut g.render_w, 1920, 7680);
                            ui.label("x");
                            opt_num(ui, &mut g.render_h, 1080, 4320);
                            ui.label(RichText::new("desmarcado = a da tela").small().weak());
                        });
                        ui.end_row();
                        ui.label("Resolução de saída");
                        ui.horizontal(|ui| {
                            opt_num(ui, &mut g.out_w, 2560, 7680);
                            ui.label("x");
                            opt_num(ui, &mut g.out_h, 1440, 4320);
                        });
                        ui.end_row();
                        ui.label("Limite de FPS");
                        ui.horizontal(|ui| opt_num(ui, &mut g.fps_limit, 60, 1000));
                        ui.end_row();
                        ui.label("");
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut g.fullscreen, "Tela cheia");
                            ui.checkbox(&mut g.fsr, "Ampliar com FSR");
                        });
                        ui.end_row();
                    });
                    ui.label(
                        RichText::new("O gamescope usa a mesma placa escolhida acima. Bom para rodar o jogo em resolução menor e ampliar com FSR, ou travar o FPS.")
                            .small()
                            .weak(),
                    );
                }

                // Upscaling
                ui.separator();
                upscale_ui(app, ui, &mut ed);

                // Otimizador de FPS
                ui.separator();
                tuner_ui(app, ui, &mut ed);

                // Prévia: o que vai acontecer ao abrir (mesma lógica do numapilot run).
                ui.separator();
                let mut profs = Profiles::default();
                profs.map.insert(ed.key.clone(), ed.p.clone());
                let idents = vec![ed.label.clone()];
                let inp = LaunchInput { idents: &idents, learn_key: "", key: &ed.key, cpu: None, gpu: None, mem: None, usage: None, fg_gpu: None };
                match decide_launch(&app.cfg, &app.topo, &Learned::default(), &profs, &app.loads, &inp) {
                    Ok(d) => {
                        ui.label(RichText::new(format!(
                            "Vai usar {} CPUs ({}) no(s) socket(s) {:?} · GPU {} · memória {}",
                            d.cpus.len(),
                            format_cpulist(&d.cpus),
                            d.nodes,
                            d.gpus.first().map(|g| g.alias.as_str()).unwrap_or("nenhuma"),
                            match d.mem {
                                numapilot::config::MemSpec::Bind => "só do socket",
                                numapilot::config::MemSpec::Interleave => "intercalada",
                                numapilot::config::MemSpec::Local => "local",
                            }
                        )).strong());
                        for w in game_warnings(&app.topo, &d) {
                            ui.label(RichText::new(format!("Atenção: {w}")).color(Color32::from_rgb(230, 160, 40)));
                        }
                    }
                    Err(e) => {
                        ui.label(RichText::new(format!("Combinação inválida: {e}")).color(Color32::from_rgb(220, 80, 60)));
                    }
                }
                ui.label(
                    RichText::new(
                        "Valem na hora: processadores, núcleos, memória (exceto intercalar), limite, prioridade e exclusivo. \
                         Valem ao reabrir: placa de vídeo, intercalar e o que o programa enxerga.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Aplicar").strong()).clicked() {
                        apply = true;
                    }
                    if ui.button("Trazer a memória para o socket certo").clicked() {
                        fix = true;
                    }
                });
            });
        });
    if apply {
        let p = ed.p.clone();
        app.send(Request::SetProfile {
            key: ed.key.clone(),
            profile: p,
            pid: ed.main_pid,
        });
    }
    if fix {
        app.send(Request::FixMemory {
            key: ed.key.clone(),
        });
    }
    if open {
        app.editor = Some(ed);
    }
}
