//! Visual do numapilot: fonte Inter, cores próprias (escuro e claro, segue o sistema), cartões,
//! medidores que deslizam em vez de pular e mini gráficos do último minuto.

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Rect,
    Sense, Shadow, Stroke, TextStyle, Theme, pos2, vec2,
};
use std::collections::VecDeque;
use std::sync::Arc;

pub const BOLD: &str = "semibold";

/// Cores do tema atual.
#[derive(Clone, Copy)]
pub struct Pal {
    pub bg: Color32,
    pub nav: Color32,
    pub card: Color32,
    pub stroke: Color32,
    pub text: Color32,
    pub weak: Color32,
    pub accent: Color32,
    pub ok: Color32,
    pub warn: Color32,
    pub err: Color32,
    pub track: Color32,
}

const DARK: Pal = Pal {
    bg: Color32::from_rgb(0x12, 0x15, 0x1b),
    nav: Color32::from_rgb(0x0d, 0x10, 0x15),
    card: Color32::from_rgb(0x1a, 0x1e, 0x26),
    stroke: Color32::from_rgb(0x27, 0x2c, 0x37),
    text: Color32::from_rgb(0xe6, 0xe8, 0xee),
    weak: Color32::from_rgb(0x8a, 0x92, 0xa3),
    accent: Color32::from_rgb(0x5b, 0x8c, 0xff),
    ok: Color32::from_rgb(0x3e, 0xcf, 0x8e),
    warn: Color32::from_rgb(0xf0, 0xa6, 0x3a),
    err: Color32::from_rgb(0xef, 0x5b, 0x5b),
    track: Color32::from_rgb(0x25, 0x2a, 0x34),
};

const LIGHT: Pal = Pal {
    bg: Color32::from_rgb(0xf4, 0xf5, 0xf8),
    nav: Color32::from_rgb(0xea, 0xec, 0xf1),
    card: Color32::from_rgb(0xff, 0xff, 0xff),
    stroke: Color32::from_rgb(0xdf, 0xe2, 0xe9),
    text: Color32::from_rgb(0x1a, 0x1e, 0x26),
    weak: Color32::from_rgb(0x66, 0x6e, 0x80),
    accent: Color32::from_rgb(0x3b, 0x6b, 0xf0),
    ok: Color32::from_rgb(0x1e, 0x9e, 0x63),
    warn: Color32::from_rgb(0xc7, 0x7c, 0x0e),
    err: Color32::from_rgb(0xd6, 0x3c, 0x3c),
    track: Color32::from_rgb(0xe6, 0xe9, 0xef),
};

pub fn pal(ui: &egui::Ui) -> Pal {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

pub fn pal_ctx(ctx: &egui::Context) -> Pal {
    if ctx.global_style().visuals.dark_mode {
        DARK
    } else {
        LIGHT
    }
}

fn theme_file() -> std::path::PathBuf {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| crate::jobs::home().join(".config"))
        .join("numapilot/gui-theme")
}

/// Tema escolhido no menu (Sistema, Claro, Escuro); fica salvo entre aberturas.
pub fn saved_theme() -> egui::ThemePreference {
    match std::fs::read_to_string(theme_file())
        .unwrap_or_default()
        .trim()
    {
        "claro" => egui::ThemePreference::Light,
        "escuro" => egui::ThemePreference::Dark,
        _ => egui::ThemePreference::System,
    }
}

pub fn save_theme(t: egui::ThemePreference) {
    let v = match t {
        egui::ThemePreference::Light => "claro",
        egui::ThemePreference::Dark => "escuro",
        egui::ThemePreference::System => "sistema",
    };
    let f = theme_file();
    if let Some(d) = f.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(f, v);
}

/// Fonte, cores e espaçamentos; segue o tema claro/escuro do sistema.
pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "inter".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-Regular.otf"
        ))),
    );
    fonts.font_data.insert(
        "inter-semibold".into(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../assets/fonts/Inter-SemiBold.otf"
        ))),
    );
    // Inter primeiro; as fontes padrão continuam atrás para símbolos e emoji.
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "inter".into());
    let fallback = prop.clone();
    let mut bold = vec!["inter-semibold".to_string()];
    bold.extend(fallback.into_iter().filter(|f| f != "inter"));
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);

    ctx.set_theme(saved_theme());
    for (theme, p) in [(Theme::Dark, DARK), (Theme::Light, LIGHT)] {
        ctx.style_mut_of(theme, |s| {
            s.text_styles = [
                (
                    TextStyle::Small,
                    FontId::new(12.0, FontFamily::Proportional),
                ),
                (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
                (
                    TextStyle::Button,
                    FontId::new(14.0, FontFamily::Proportional),
                ),
                (
                    TextStyle::Heading,
                    FontId::new(19.0, FontFamily::Name(BOLD.into())),
                ),
                (
                    TextStyle::Monospace,
                    FontId::new(13.0, FontFamily::Monospace),
                ),
            ]
            .into();
            let sp = &mut s.spacing;
            sp.item_spacing = vec2(10.0, 8.0);
            sp.button_padding = vec2(12.0, 5.0);
            sp.interact_size.y = 28.0;
            sp.window_margin = Margin::same(14);
            sp.menu_margin = Margin::same(8);
            sp.combo_width = 140.0;

            let v = &mut s.visuals;
            v.panel_fill = p.bg;
            v.window_fill = p.card;
            v.window_stroke = Stroke::new(1.0, p.stroke);
            v.window_corner_radius = CornerRadius::same(12);
            v.menu_corner_radius = CornerRadius::same(10);
            v.window_shadow = Shadow {
                offset: [0, 8],
                blur: 24,
                spread: 0,
                color: Color32::from_black_alpha(if theme == Theme::Dark { 110 } else { 40 }),
            };
            v.popup_shadow = Shadow {
                offset: [0, 4],
                blur: 14,
                spread: 0,
                color: Color32::from_black_alpha(if theme == Theme::Dark { 90 } else { 30 }),
            };
            v.extreme_bg_color = if theme == Theme::Dark {
                Color32::from_rgb(0x0f, 0x12, 0x17)
            } else {
                Color32::from_rgb(0xf7, 0xf8, 0xfa)
            };
            v.faint_bg_color = if theme == Theme::Dark {
                Color32::from_rgb(0x1e, 0x23, 0x2c)
            } else {
                Color32::from_rgb(0xf3, 0xf5, 0xf8)
            };
            v.hyperlink_color = p.accent;
            v.warn_fg_color = p.warn;
            v.error_fg_color = p.err;
            v.selection.bg_fill = p.accent.gamma_multiply(0.85);
            v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
            v.override_text_color = None;
            v.slider_trailing_fill = true;
            v.striped = false;

            let (idle, hover, active) = if theme == Theme::Dark {
                (
                    Color32::from_rgb(0x24, 0x29, 0x33),
                    Color32::from_rgb(0x2d, 0x33, 0x40),
                    Color32::from_rgb(0x35, 0x3c, 0x4b),
                )
            } else {
                (
                    Color32::from_rgb(0xee, 0xf0, 0xf4),
                    Color32::from_rgb(0xe4, 0xe7, 0xee),
                    Color32::from_rgb(0xd9, 0xdd, 0xe6),
                )
            };
            let r = CornerRadius::same(8);
            let w = &mut v.widgets;
            w.noninteractive.bg_fill = p.card;
            w.noninteractive.weak_bg_fill = p.card;
            w.noninteractive.bg_stroke = Stroke::new(1.0, p.stroke);
            w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
            w.noninteractive.corner_radius = r;
            for (wv, fill) in [
                (&mut w.inactive, idle),
                (&mut w.hovered, hover),
                (&mut w.active, active),
                (&mut w.open, active),
            ] {
                wv.bg_fill = fill;
                wv.weak_bg_fill = fill;
                wv.corner_radius = r;
                wv.expansion = 0.0;
                wv.fg_stroke = Stroke::new(1.0, p.text);
                wv.bg_stroke = Stroke::NONE;
            }
            w.inactive.fg_stroke = Stroke::new(1.0, p.text.gamma_multiply(0.92));
            w.hovered.bg_stroke = Stroke::new(1.0, p.stroke);
            w.active.bg_stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.6));
        });
    }
}

/// Cartão: fundo próprio, borda fina e cantos arredondados.
#[allow(dead_code)]
pub fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.card)
        .stroke(Stroke::new(1.0, p.stroke))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::same(14))
        .show(ui, add)
        .inner
}

/// Cartão com título (e uma linha de explicação opcional).
#[allow(dead_code)]
pub fn section<R>(
    ui: &mut egui::Ui,
    title: &str,
    hint: Option<&str>,
    add: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    card(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.label(
            egui::RichText::new(title)
                .size(15.5)
                .family(FontFamily::Name(BOLD.into())),
        );
        if let Some(h) = hint {
            ui.label(egui::RichText::new(h).weak().size(12.5));
        }
        ui.add_space(4.0);
        add(ui)
    })
}

/// Etiqueta colorida (status).
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (r, _) = ui.allocate_exact_size(vec2(7.0, 7.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, color);
                ui.label(egui::RichText::new(text).color(color).size(12.5));
            });
        })
        .response
}

/// Valor que desliza até o alvo (sem pulos quando a leitura muda).
pub fn smooth(ui: &egui::Ui, id: &str, target: f32) -> f32 {
    ui.ctx()
        .animate_value_with_time(ui.make_persistent_id(("smooth", id)), target, 0.45)
}

/// Cor do medidor pelo nível: normal, alto, crítico.
pub fn level_color(p: &Pal, frac: f32, color: Color32) -> Color32 {
    if frac >= 0.95 {
        p.err
    } else if frac >= 0.85 {
        p.warn
    } else {
        color
    }
}

/// Barra fina arredondada.
pub fn meter(ui: &mut egui::Ui, frac: f32, width: f32, color: Color32) -> egui::Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 6.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(3), p.track);
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        let mut r = rect;
        r.set_width((rect.width() * f).max(6.0));
        painter.rect_filled(r, CornerRadius::same(3), color);
    }
    resp
}

/// Histórico curto (último minuto) para os mini gráficos.
#[derive(Default, Clone)]
pub struct Spark(VecDeque<f32>);

impl Spark {
    pub const LEN: usize = 60;
    pub fn push(&mut self, v: f32) {
        if self.0.len() >= Self::LEN {
            self.0.pop_front();
        }
        self.0.push_back(v.clamp(0.0, 1.0));
    }
}

/// Item do menu lateral.
pub fn nav_item(ui: &mut egui::Ui, icon: &str, label: &str, selected: bool) -> egui::Response {
    let p = pal(ui);
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 34.0), Sense::click());
    let hovered = resp.hovered();
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(8), p.accent.gamma_multiply(0.16));
        let bar = Rect::from_min_size(rect.min + vec2(0.0, 8.0), vec2(3.0, rect.height() - 16.0));
        painter.rect_filled(bar, CornerRadius::same(2), p.accent);
    } else if hovered {
        painter.rect_filled(rect, CornerRadius::same(8), p.stroke.gamma_multiply(0.6));
    }
    let fg = if selected { p.text } else { p.weak };
    let icon_c = if selected { p.accent } else { p.weak };
    painter.text(
        rect.left_center() + vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        icon,
        FontId::proportional(15.0),
        icon_c,
    );
    painter.text(
        rect.left_center() + vec2(40.0, 0.0),
        egui::Align2::LEFT_CENTER,
        label,
        if selected {
            FontId::new(14.0, FontFamily::Name(BOLD.into()))
        } else {
            FontId::proportional(14.0)
        },
        if hovered && !selected { p.text } else { fg },
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Cartão de medida: nome, valor grande, detalhe, mini gráfico do último minuto, barra que
/// desliza até o valor novo e, opcionalmente, uma segunda barra (ex.: VRAM).
pub struct Metric<'a> {
    pub key: &'a str,
    pub name: &'a str,
    pub big: &'a str,
    pub small: &'a str,
    pub frac: f32,
    pub color: Color32,
    pub spark: Option<&'a Spark>,
    pub extra: Option<(&'a str, f32)>,
}

pub fn metric(ui: &mut egui::Ui, m: &Metric) -> egui::Response {
    let p = pal(ui);
    let h = if m.extra.is_some() { 128.0 } else { 104.0 };
    let (rect, resp) = ui.allocate_exact_size(vec2(248.0, h), Sense::hover());
    let painter = ui.painter_at(rect.expand(2.0));
    painter.rect(
        rect,
        CornerRadius::same(12),
        p.card,
        Stroke::new(1.0, p.stroke),
        egui::StrokeKind::Inside,
    );
    let inner = rect.shrink(14.0);
    let x = inner.left();
    // nome em toda a largura (cortado se não couber)
    let name: String = if m.name.chars().count() > 36 {
        m.name.chars().take(35).collect::<String>() + "…"
    } else {
        m.name.to_string()
    };
    painter.text(
        pos2(x, inner.top()),
        egui::Align2::LEFT_TOP,
        name,
        FontId::proportional(12.0),
        p.weak,
    );
    painter.text(
        pos2(x, inner.top() + 16.0),
        egui::Align2::LEFT_TOP,
        m.big,
        FontId::new(22.0, FontFamily::Name(BOLD.into())),
        p.text,
    );
    painter.text(
        pos2(x, inner.top() + 44.0),
        egui::Align2::LEFT_TOP,
        m.small,
        FontId::proportional(11.5),
        p.weak,
    );
    if let Some(sp) = m.spark {
        let r = Rect::from_min_size(
            pos2(inner.right() - 84.0, inner.top() + 18.0),
            vec2(84.0, 40.0),
        );
        spark_in(&painter, r, sp, m.color);
    }
    let bar = |y: f32, f: f32, c: Color32| {
        let track = Rect::from_min_size(pos2(x, y), vec2(inner.width(), 6.0));
        painter.rect_filled(track, CornerRadius::same(3), p.track);
        let f = f.clamp(0.0, 1.0);
        if f > 0.0 {
            let mut r = track;
            r.set_width((track.width() * f).max(6.0));
            painter.rect_filled(r, CornerRadius::same(3), c);
        }
    };
    let f = smooth(ui, m.key, m.frac);
    bar(inner.top() + 66.0, f, level_color(&p, m.frac, m.color));
    if let Some((label, ef)) = m.extra {
        painter.text(
            pos2(x, inner.top() + 80.0),
            egui::Align2::LEFT_TOP,
            label,
            FontId::proportional(11.5),
            p.weak,
        );
        let sf = smooth(ui, &format!("{}-extra", m.key), ef);
        bar(
            inner.top() + 96.0,
            sf,
            level_color(&p, ef, m.color.gamma_multiply(0.65)),
        );
    }
    resp
}

pub fn spark_in(painter: &egui::Painter, rect: Rect, s: &Spark, color: Color32) {
    if s.0.len() < 2 {
        return;
    }
    let n = (Spark::LEN - 1) as f32;
    let len = s.0.len();
    let pts: Vec<_> =
        s.0.iter()
            .enumerate()
            .map(|(i, v)| {
                pos2(
                    rect.right() - (len - 1 - i) as f32 * rect.width() / n,
                    rect.bottom() - v * (rect.height() - 2.0) - 1.0,
                )
            })
            .collect();
    let mut mesh = egui::Mesh::default();
    let fill = color.gamma_multiply(0.16);
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        for q in [
            w[0],
            w[1],
            pos2(w[1].x, rect.bottom()),
            pos2(w[0].x, rect.bottom()),
        ] {
            mesh.colored_vertex(q, fill);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(mesh);
    painter.add(egui::Shape::line(pts, Stroke::new(1.5, color)));
}
