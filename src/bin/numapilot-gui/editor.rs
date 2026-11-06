//! Janela "Ajustar programa": onde no hardware ele roda (núcleos, memória, prioridade, GPU),
//! com perfis prontos e avisos de compatibilidade para jogos.

use crate::map::core_map;
use crate::App;
use eframe::egui::{self, Color32, RichText};
use numapilot::ctl::Request;
use numapilot::decide::{decide_launch, game_warnings, LaunchInput};
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
    pub fn new(key: &str, label: &str, main_pid: Option<i32>, cpus: &str, mem: &[u64], p: Profile) -> Editor {
        let pick: BTreeSet<usize> = p.cpu_list.as_deref().map(parse_cpulist).unwrap_or_default().into_iter().collect();
        Editor {
            key: key.into(),
            label: label.into(),
            main_pid,
            current_cpus: cpus.into(),
            mem: mem.to_vec(),
            picking: !pick.is_empty(),
            pick,
            p: Profile { label: label.into(), ..p },
        }
    }
}

const MEM_OPTS: &[(&str, &str, &str)] = &[
    ("auto", "Automático", "o numapilot decide (normalmente: memória do socket dos núcleos)"),
    ("strict", "Só do socket (estrito)", "menor latência; se a memória do socket encher, o programa sofre"),
    ("prefer", "Preferir a local", "usa a do outro socket só quando faltar"),
    ("interleave", "Intercalar entre sockets", "mais banda total; bom para render/IA na CPU (vale ao reabrir)"),
    ("free", "Livre", "não move a memória"),
];

pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(mut ed) = app.editor.take() else { return };
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

                // Perfis prontos
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new("Perfis prontos:").strong());
                    for (id, name, tip) in [
                        ("jogo", "Jogo", "socket da GPU, 16 núcleos físicos, memória só local, prioridade alta, núcleos reservados, o jogo enxerga só esses núcleos"),
                        ("ia", "IA", "socket da GPU de computação, memória preferindo a local"),
                        ("render", "Renderização", "os 2 sockets, memória intercalada (banda máxima)"),
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
                    ui.vertical(|ui| {
                        for (v, t, d) in MEM_OPTS {
                            ui.horizontal(|ui| {
                                ui.radio_value(&mut ed.p.memory, v.to_string(), *t);
                                ui.label(RichText::new(*d).small().weak());
                            });
                        }
                        ui.horizontal(|ui| {
                            let mut lim = ed.p.mem_max_gb.is_some();
                            if ui.checkbox(&mut lim, "limite de").changed() {
                                ed.p.mem_max_gb = if lim { Some(16.0) } else { None };
                            }
                            let max_gb = (app.topo.nodes.iter().map(|n| n.mem_total_kb).sum::<u64>() as f64 / 1048576.0).max(1.0);
                            if let Some(g) = ed.p.mem_max_gb.as_mut() {
                                ui.add(egui::Slider::new(g, 0.5..=max_gb).suffix(" GB").step_by(0.5));
                            }
                        });
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
        app.send(Request::SetProfile { key: ed.key.clone(), profile: p, pid: ed.main_pid });
    }
    if fix {
        app.send(Request::FixMemory { key: ed.key.clone() });
    }
    if open {
        app.editor = Some(ed);
    }
}
