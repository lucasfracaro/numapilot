//! Aba "Testes": roteiro para testar o programa à mão (só os itens que valem para esta
//! máquina), com OK / Falhou / Pulei e uma nota em cada um, mais o autoteste da linha de
//! comando e um relatório único com tudo (respostas, autoteste e o final dos registros).

use crate::App;
use crate::theme;
use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq)]
enum When {
    Always,
    Laptop,
    Nvidia,
    NvidiaLaptop,
    Board,
    MultiSocket,
    RyzenLaptop,
    Acer,
    /// alguma placa com ventoinha controlável
    FanControl,
    /// placa AMD com o OverDrive desligado / ligado
    AmdOdOff,
    AmdOdOn,
}

struct Item {
    id: &'static str,
    area: &'static str,
    title: &'static str,
    how: &'static str,
    expect: &'static str,
    when: When,
}

const fn it(
    id: &'static str,
    area: &'static str,
    title: &'static str,
    how: &'static str,
    expect: &'static str,
    when: When,
) -> Item {
    Item {
        id,
        area,
        title,
        how,
        expect,
        when,
    }
}

use When::*;

const ITEMS: &[Item] = &[
    // Geral
    it(
        "abrir",
        "Geral",
        "Abrir o programa",
        "Abra o numapilot pelo menu do sistema.",
        "A janela abre em poucos segundos, no tema do sistema, sem aviso de erro.",
        Always,
    ),
    it(
        "janela-unica",
        "Geral",
        "Uma janela só",
        "Com a janela aberta, abra o numapilot de novo pelo menu.",
        "A janela que já estava aberta vem para frente; não abre outra.",
        Always,
    ),
    it(
        "bandeja",
        "Geral",
        "Ícone na bandeja",
        "Ligue o ícone na bandeja (Configurações), feche a janela e clique no ícone.",
        "A janela abre. Só existe um ícone do numapilot na bandeja.",
        Always,
    ),
    it(
        "tema",
        "Geral",
        "Tema",
        "Troque para Claro e Escuro no menu lateral, feche e abra de novo.",
        "O tema muda na hora e continua o escolhido depois de reabrir.",
        Always,
    ),
    it(
        "medidores",
        "Geral",
        "Medidores do topo",
        "Abra um jogo ou benchmark e olhe os cartões de CPU, memória e placas.",
        "Os valores sobem, as barras deslizam sem pular e batem com outro monitor (Mission Center, nvtop).",
        Always,
    ),
    it(
        "servico",
        "Geral",
        "Serviço parado e religar",
        "No terminal: sudo systemctl stop numapilot. Depois use o botão \"Ativar o serviço\".",
        "Aparece \"Serviço parado\" com o botão; o botão pede a senha e volta para \"Serviço ativo\".",
        Always,
    ),
    it(
        "log",
        "Geral",
        "Registro",
        "No terminal: numapilot log",
        "Mostra o que foi feito (serviço, programa e comandos) com data e hora.",
        Always,
    ),
    // Programas
    it(
        "lista",
        "Programas",
        "Lista de programas",
        "Abra um programa qualquer (navegador, jogo).",
        "Ele aparece em Programas em até 5 segundos, com o socket e a placa que está usando.",
        Always,
    ),
    it(
        "nucleos",
        "Programas",
        "Escolher núcleos",
        "Em Ajustar…, escolha núcleos específicos e salve.",
        "No Mapa (gráfico por thread) aparece o ponto amarelo nesses núcleos e o uso fica neles.",
        Always,
    ),
    it(
        "limite-mem",
        "Programas",
        "Limite de memória",
        "Em Ajustar…, marque \"Limitar a memória do programa a\" 2 GB e use o programa.",
        "O programa não passa do limite (o excedente vai para o swap); o PC não trava.",
        Always,
    ),
    it(
        "gpu-escolha",
        "Programas",
        "Escolher a placa",
        "Em Ajustar…, escolha outra placa e use Reabrir.",
        "O programa reabre e passa a usar a placa escolhida (o uso aparece nela).",
        Always,
    ),
    it(
        "steam",
        "Programas",
        "Jogos da Steam",
        "Use \"Aplicar em todos os jogos\" com a Steam fechada; depois \"Remover de todos\".",
        "As opções de inicialização dos jogos mudam e voltam ao que eram.",
        Always,
    ),
    // Mapa
    it(
        "threads",
        "Mapa",
        "Gráfico por thread",
        "Abra Mapa e medições → Gráfico por thread e rode algo pesado.",
        "Os gráficos dos threads em uso sobem; ao passar o mouse mostra o núcleo e a frequência.",
        Always,
    ),
    it(
        "temp-cpu",
        "Mapa",
        "Temperatura do processador",
        "Olhe o gráfico de temperatura com carga e depois parado.",
        "Acompanha a temperatura real (sobe com carga, desce parado).",
        Always,
    ),
    it(
        "memoria-local",
        "Mapa",
        "Memória só do socket",
        "Em Ajustar… de um programa, escolha usar só a memória do socket dele.",
        "Em \"Alocações no outro socket\" esse programa não aumenta as alocações remotas.",
        MultiSocket,
    ),
    it(
        "medir",
        "Mapa",
        "Medir latência e banda",
        "Clique em \"Medir agora\".",
        "A tabela é preenchida sem erro.",
        MultiSocket,
    ),
    // Placas
    it(
        "ventoinha",
        "Placas",
        "Curva da ventoinha",
        "Em Placas de vídeo, escolha \"curva\", aplique e depois Desfazer.",
        "A ventoinha segue a curva; Desfazer volta para o automático da placa.",
        FanControl,
    ),
    it(
        "potencia",
        "Placas",
        "Limite de potência",
        "Baixe o limite de potência de uma placa e rode um jogo. Se a placa não mostra esse controle, o fabricante trava: marque Pulei.",
        "Os watts no topo não passam do limite; Desfazer volta ao padrão.",
        Always,
    ),
    it(
        "undervolt",
        "Placas",
        "Undervolt automático",
        "Na tomada, use \"Achar o undervolt sozinho\" e deixe o PC parado.",
        "Escolhe um programa de carga com 80% de uso ou mais, mostra clock e uso, termina e grava; o terminal fecha com Enter.",
        Always,
    ),
    it(
        "nv-offset",
        "Placas",
        "Deslocamento de clock (NVIDIA)",
        "Aplique um deslocamento pequeno (ex.: +50 MHz) e reinicie o serviço.",
        "O valor aparece aplicado e volta sozinho depois de reiniciar o serviço.",
        Nvidia,
    ),
    it(
        "overdrive",
        "Placas",
        "OverDrive (AMD)",
        "Use \"Ativar OverDrive\" e reinicie o PC.",
        "Depois de reiniciar, os controles de clock e voltagem da placa ficam disponíveis.",
        AmdOdOff,
    ),
    it(
        "clock-amd",
        "Placas",
        "Clock e voltagem (AMD)",
        "Na placa AMD, em \"Clock e voltagem\", marque personalizar, baixe \"GPU máx.\" e aplique; depois Desfazer.",
        "Com carga, o clock da placa não passa do valor escolhido; Desfazer volta ao de fábrica.",
        AmdOdOn,
    ),
    // Máquina
    it(
        "modo",
        "Máquina",
        "Modos da máquina",
        "Troque entre Jogo, IA, Silencioso e Normal.",
        "O governador e a preferência de energia mudam (numapilot autoteste mostra); Normal volta ao que era.",
        Always,
    ),
    it(
        "horario",
        "Máquina",
        "Modo por horário",
        "Crie um horário que começa daqui a 2 minutos.",
        "Na hora, o modo troca sozinho; depois do fim, volta.",
        Always,
    ),
    it(
        "energia",
        "Máquina",
        "Energia e custo",
        "Informe o preço do kWh e espere alguns minutos.",
        "Hoje/mês começam a contar e crescem com o uso.",
        Always,
    ),
    it(
        "saude",
        "Máquina",
        "Saúde do hardware",
        "Espere 1 minuto depois de iniciar o serviço e abra a seção Saúde.",
        "Discos, memória e placas aparecem com estado (sem \"nunca coletado\").",
        Always,
    ),
    // Notebook
    it(
        "bateria",
        "Notebook",
        "Tomada e bateria",
        "Tire o carregador; depois ligue de novo.",
        "Em até 10 s o numapilot mostra \"na bateria\" e depois \"na tomada\".",
        Laptop,
    ),
    it(
        "nv-auto",
        "Notebook",
        "NVIDIA no automático",
        "Modo auto. Na bateria, feche o que usa a NVIDIA; depois ligue na tomada.",
        "Na bateria a NVIDIA desliga (numapilot hybrid: \"desligada\"); na tomada volta.",
        NvidiaLaptop,
    ),
    it(
        "nv-segurando",
        "Notebook",
        "Quem segura a NVIDIA",
        "Rode: numapilot hybrid",
        "Lista só programas que realmente usam a NVIDIA (sem systemd, kwin, etc.).",
        NvidiaLaptop,
    ),
    it(
        "autostart",
        "Notebook",
        "Programas do login na integrada",
        "Depois de entrar na sessão, rode: numapilot hybrid",
        "Nada que abre com o sistema aparece segurando a NVIDIA.",
        NvidiaLaptop,
    ),
    it(
        "telas",
        "Notebook",
        "Telas externas",
        "Ligue monitores no HDMI e no USB-C/Thunderbolt; troque um de porta.",
        "Todas dão imagem; trocar de porta não deixa as telas piscando.",
        Laptop,
    ),
    it(
        "ryzenadj",
        "Notebook",
        "Limites do Ryzen",
        "Mude o limite STAPM em Máquina e rode: sudo ryzenadj --info",
        "O valor novo aparece no ryzenadj e continua depois de alguns minutos.",
        RyzenLaptop,
    ),
    it(
        "acer",
        "Notebook",
        "Controles da Acer",
        "Mude o perfil/ventoinha da Acer em Máquina. Se não aparece nada, rode numapilot autoteste: o item Acer mostra o motivo.",
        "O notebook responde (ventoinha/perfil mudam) e o valor fica salvo.",
        Acer,
    ),
    // Placa especial
    it(
        "bc-apps",
        "BC-250",
        "Apps da comunidade",
        "Em Placas de vídeo, veja a seção da BC-250.",
        "Os apps da comunidade aparecem com instalar/abrir.",
        Board,
    ),
    it(
        "bc-mem",
        "BC-250",
        "Memória da GPU",
        "Mude a memória da GPU e reinicie.",
        "Depois de reiniciar, o autoteste mostra o valor novo em \"Memória da GPU (TTM)\".",
        Board,
    ),
    it(
        "bc-cu",
        "BC-250",
        "Núcleos e CUs",
        "Rode o autoteste.",
        "Mostra os núcleos e CUs ativos esperados para o desbloqueio.",
        Board,
    ),
    it(
        "bc-stream",
        "BC-250",
        "Streaming sem codificador de vídeo",
        "Ligue o Sunshine pelo numapilot e conecte pelo Moonlight.",
        "Conecta usando a codificação por software, com imagem estável.",
        Board,
    ),
    // Outros
    it(
        "ia",
        "Outros",
        "Servidor de IA",
        "Inicie um modelo pequeno e mande uma pergunta.",
        "Responde; \"Ajustar melhor\" termina e sugere configurações.",
        Always,
    ),
    it(
        "lab",
        "Outros",
        "Laboratório",
        "Crie um container e abra o Jupyter.",
        "O container sobe e o Jupyter abre no navegador.",
        Always,
    ),
    it(
        "streaming",
        "Outros",
        "Streaming",
        "Ligue o Sunshine e conecte pelo Moonlight; abra um jogo pelo celular.",
        "Conecta e o jogo abre.",
        Always,
    ),
    it(
        "peers",
        "Outros",
        "Outros PCs",
        "Adicione o link de outro PC e use \"ligar pela rede\" com ele desligado.",
        "Ele aparece online; o comando de ligar acorda o PC.",
        Always,
    ),
    it(
        "avisos",
        "Outros",
        "Avisos no celular",
        "Configure o ntfy e envie o teste.",
        "O aviso chega no celular.",
        Always,
    ),
    it(
        "backup",
        "Outros",
        "Cópia das configurações",
        "Use \"Guardar de todos\" em Outros PCs.",
        "Os arquivos aparecem na pasta de cópias.",
        Always,
    ),
];

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Res {
    pub st: String,
    #[serde(default)]
    pub nota: String,
    #[serde(default)]
    pub quando: String,
}

#[derive(Default)]
pub struct Checklist {
    loaded: bool,
    pub res: BTreeMap<String, Res>,
    flags: Option<Flags>,
    pub last_report: Option<PathBuf>,
}

#[derive(Clone, Copy)]
struct Flags {
    laptop: bool,
    nvidia: bool,
    amd: bool,
    board: bool,
    multi: bool,
    ryzen: bool,
    acer: bool,
    fan: bool,
    od_on: bool,
}

fn state_dir() -> PathBuf {
    numapilot::log::user_dir()
}

fn path() -> PathBuf {
    state_dir().join("testes.json")
}

pub fn autotest_path() -> PathBuf {
    state_dir().join("autoteste.txt")
}

impl Checklist {
    fn load(&mut self) {
        if !self.loaded {
            self.loaded = true;
            self.res = std::fs::read_to_string(path())
                .ok()
                .and_then(|t| serde_json::from_str(&t).ok())
                .unwrap_or_default();
        }
    }
    fn save(&self) {
        let _ = std::fs::create_dir_all(state_dir());
        if let Ok(t) = serde_json::to_string_pretty(&self.res) {
            let _ = std::fs::write(path(), t);
        }
    }
}

fn flags(app: &App) -> Flags {
    let model = numapilot::recipes::cpu_model();
    let laptop = numapilot::notebook::is_laptop();
    Flags {
        laptop,
        nvidia: app.topo.gpus.iter().any(|g| g.is_nvidia()),
        amd: app.topo.gpus.iter().any(|g| g.vendor == 0x1002),
        board: !numapilot::recipes::load().boards(&app.topo).is_empty(),
        multi: app.topo.nodes.len() > 1,
        ryzen: laptop && model.contains("Ryzen"),
        acer: numapilot::acer::is_acer(),
        fan: app.topo.gpus.iter().any(|g| {
            let s = numapilot::gpuctl::read_sensors(&g.pci);
            s.fan_controllable || s.fw_fan_curve
        }),
        od_on: app
            .topo
            .gpus
            .iter()
            .any(|g| g.vendor == 0x1002 && numapilot::gpuctl::read_od(&g.pci).is_some()),
    }
}

fn applies(w: When, f: &Flags) -> bool {
    match w {
        Always => true,
        Laptop => f.laptop,
        Nvidia => f.nvidia,
        NvidiaLaptop => f.nvidia && f.laptop,
        Board => f.board,
        MultiSocket => f.multi,
        RyzenLaptop => f.ryzen,
        Acer => f.acer,
        FanControl => f.fan,
        AmdOdOff => f.amd && !f.od_on,
        AmdOdOn => f.od_on,
    }
}

fn st_label(st: &str) -> &'static str {
    match st {
        "ok" => "OK",
        "falhou" => "FALHOU",
        "pulei" => "PULEI",
        _ => "—",
    }
}

/// Relatório único: respostas, autoteste e o final dos registros.
fn report(app: &App) -> Result<PathBuf, String> {
    let f = app.checklist.flags.unwrap_or_else(|| flags(app));
    let items: Vec<&Item> = ITEMS.iter().filter(|i| applies(i.when, &f)).collect();
    let r = &app.checklist.res;
    let count = |s: &str| {
        items
            .iter()
            .filter(|i| r.get(i.id).is_some_and(|x| x.st == s))
            .count()
    };
    let mut s = format!(
        "# numapilot — relatório de testes\n\nversão {} · máquina {} · {}\n\
         Testes à mão: {} itens · {} OK · {} falharam · {} pulados · {} sem resposta\n",
        env!("CARGO_PKG_VERSION"),
        numapilot::selftest::hostname(),
        numapilot::log::now(),
        items.len(),
        count("ok"),
        count("falhou"),
        count("pulei"),
        items.len() - count("ok") - count("falhou") - count("pulei"),
    );
    let mut area = "";
    for i in &items {
        if i.area != area {
            area = i.area;
            s.push_str(&format!("\n## {area}\n"));
        }
        let x = r.get(i.id).cloned().unwrap_or_default();
        s.push_str(&format!("{:<7} {}", st_label(&x.st), i.title));
        if !x.quando.is_empty() {
            s.push_str(&format!(" ({})", x.quando));
        }
        s.push('\n');
        if !x.nota.trim().is_empty() {
            for l in x.nota.lines() {
                s.push_str(&format!("        nota: {l}\n"));
            }
        }
    }
    s.push_str("\n\n");
    match std::fs::read_to_string(autotest_path()) {
        Ok(t) => s.push_str(&t),
        Err(_) => s.push_str(
            "# autoteste\n\n(não rodado: use \"Rodar o autoteste\" antes de gerar o relatório)\n",
        ),
    }
    s.push_str("\n\n# registros (final)\n");
    for (name, p) in numapilot::log::files() {
        let t = numapilot::log::tail(&p, 150);
        if !t.is_empty() {
            s.push_str(&format!("\n## {name} ({})\n", p.display()));
            for l in t {
                s.push_str(&l);
                s.push('\n');
            }
        }
    }
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| "/tmp".into()));
    let d = numapilot::log::now().replace([' ', ':'], "-");
    let out = home.join(format!(
        "numapilot-testes-{}-{}.txt",
        numapilot::selftest::hostname(),
        &d[..16.min(d.len())]
    ));
    std::fs::write(&out, s).map_err(|e| format!("{}: {e}", out.display()))?;
    numapilot::log::info(&format!("relatório de testes gerado: {}", out.display()));
    Ok(out)
}

pub fn ui(app: &mut App, ui: &mut egui::Ui) {
    app.checklist.load();
    if app.checklist.flags.is_none() {
        app.checklist.flags = Some(flags(app));
    }
    let f = app.checklist.flags.unwrap();
    let p = theme::pal(ui);
    let items: Vec<&Item> = ITEMS.iter().filter(|i| applies(i.when, &f)).collect();
    let answered = items
        .iter()
        .filter(|i| app.checklist.res.contains_key(i.id))
        .count();
    let failed = items
        .iter()
        .filter(|i| {
            app.checklist
                .res
                .get(i.id)
                .is_some_and(|r| r.st == "falhou")
        })
        .count();

    ui.horizontal_wrapped(|ui| {
        if ui
            .button("Rodar o autoteste")
            .on_hover_text("Testa todas as partes pela linha de comando (só lê, não muda nada). Leva um ou dois minutos.")
            .clicked()
        {
            let exe = crate::jobs::sibling_abs("numapilot");
            app.start_job(
                "Autoteste",
                vec![
                    exe,
                    "autoteste".into(),
                    "--saida".into(),
                    autotest_path().display().to_string(),
                ],
            );
        }
        if ui
            .button("Gerar relatório")
            .on_hover_text("Um arquivo com suas respostas, o último autoteste e o final dos registros, para enviar.")
            .clicked()
        {
            match report(app) {
                Ok(pth) => {
                    app.toast(format!("Relatório: {}", pth.display()), true);
                    app.checklist.last_report = Some(pth);
                }
                Err(e) => app.toast(format!("Não consegui gerar o relatório: {e}"), false),
            }
        }
        if ui.button("Limpar respostas").clicked() {
            app.checklist.res.clear();
            app.checklist.save();
        }
        ui.label(
            RichText::new(format!(
                "{answered} de {} respondidos · {failed} falharam",
                items.len()
            ))
            .color(if failed > 0 { p.err } else { p.weak }),
        );
    });
    if let Some(pth) = &app.checklist.last_report {
        ui.label(
            RichText::new(format!("Último relatório: {}", pth.display()))
                .small()
                .weak(),
        );
    }
    ui.label(
        RichText::new(
            "Só aparecem os testes que valem para esta máquina. As respostas ficam salvas; marque e escreva uma nota quando algo der errado.",
        )
        .small()
        .weak(),
    );
    ui.add_space(6.0);

    let mut changed = false;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let mut area = "";
            for i in &items {
                if i.area != area {
                    area = i.area;
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(area)
                            .size(15.0)
                            .family(egui::FontFamily::Name(theme::BOLD.into())),
                    );
                }
                let entry = app.checklist.res.get(i.id).cloned();
                let st = entry.as_ref().map(|r| r.st.clone()).unwrap_or_default();
                let stroke = match st.as_str() {
                    "ok" => p.ok.gamma_multiply(0.6),
                    "falhou" => p.err.gamma_multiply(0.7),
                    "pulei" => p.weak.gamma_multiply(0.5),
                    _ => p.stroke,
                };
                egui::Frame::new()
                    .fill(p.card)
                    .stroke(egui::Stroke::new(1.0, stroke))
                    .corner_radius(10)
                    .inner_margin(egui::Margin::symmetric(12, 9))
                    .show(ui, |ui| {
                        ui.set_min_width(ui.available_width() - 12.0);
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(i.title)
                                    .family(egui::FontFamily::Name(theme::BOLD.into())),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    for (v, l, c) in [
                                        ("pulei", "Pulei", p.weak),
                                        ("falhou", "Falhou", p.err),
                                        ("ok", "OK", p.ok),
                                    ] {
                                        let sel = st == v;
                                        let txt = if sel {
                                            RichText::new(l).color(Color32::WHITE)
                                        } else {
                                            RichText::new(l).color(c)
                                        };
                                        let b = egui::Button::new(txt)
                                            .fill(if sel { c } else { Color32::TRANSPARENT })
                                            .stroke(egui::Stroke::new(1.0, c.gamma_multiply(0.6)));
                                        if ui.add(b).clicked() {
                                            if sel {
                                                app.checklist.res.remove(i.id);
                                            } else {
                                                let e = app
                                                    .checklist
                                                    .res
                                                    .entry(i.id.to_string())
                                                    .or_default();
                                                e.st = v.to_string();
                                                e.quando = numapilot::log::now();
                                                numapilot::log::info(&format!(
                                                    "teste \"{}\": {}",
                                                    i.title,
                                                    st_label(v)
                                                ));
                                            }
                                            changed = true;
                                        }
                                    }
                                },
                            );
                        });
                        ui.label(
                            RichText::new(format!("Como: {}", i.how))
                                .size(12.5)
                                .color(p.weak),
                        );
                        ui.label(RichText::new(format!("Esperado: {}", i.expect)).size(12.5));
                        if let Some(e) = app.checklist.res.get_mut(i.id) {
                            let r = ui.add(
                                egui::TextEdit::singleline(&mut e.nota)
                                    .hint_text(if e.st == "falhou" {
                                        "O que aconteceu? (aparece no relatório)"
                                    } else {
                                        "nota (opcional)"
                                    })
                                    .desired_width(f32::INFINITY),
                            );
                            if r.lost_focus() || (r.changed() && e.nota.ends_with(' ')) {
                                changed = true;
                            }
                        }
                    });
                ui.add_space(4.0);
            }
        });
    if changed {
        app.checklist.save();
    }
}
