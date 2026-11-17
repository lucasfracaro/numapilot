//! numapilot's look: Inter font, its own colors (dark and light, follows the system), cards,
//! meters that slide instead of jumping and sparklines of the last minute.

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

/// Theme in use, for colors asked where there is no `ui` at hand (set at every frame).
static DARK_NOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

pub fn note_theme(ctx: &egui::Context) {
    DARK_NOW.store(
        ctx.global_style().visuals.dark_mode,
        std::sync::atomic::Ordering::Relaxed,
    );
}

fn now() -> Pal {
    if DARK_NOW.load(std::sync::atomic::Ordering::Relaxed) {
        DARK
    } else {
        LIGHT
    }
}

/// Status colors of the theme in use: fine, attention, problem.
pub fn ok() -> Color32 {
    now().ok
}
pub fn warn() -> Color32 {
    now().warn
}
pub fn err() -> Color32 {
    now().err
}

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

/// Theme chosen in the menu (System, Light, Dark); saved between launches.
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

/// Font, colors and spacing; follows the system's light/dark theme.
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
    // Inter first; the default fonts stay behind it for symbols and emoji.
    let prop = fonts.families.entry(FontFamily::Proportional).or_default();
    prop.insert(0, "inter".into());
    let fallback = prop.clone();
    let mut bold = vec!["inter-semibold".to_string()];
    bold.extend(fallback.into_iter().filter(|f| f != "inter"));
    fonts.families.insert(FontFamily::Name(BOLD.into()), bold);
    ctx.set_fonts(fonts);

    // NUMAPILOT_GUI_THEME=light|dark: for screenshots, without touching the saved choice
    ctx.set_theme(match std::env::var("NUMAPILOT_GUI_THEME").as_deref() {
        Ok("light") => egui::ThemePreference::Light,
        Ok("dark") => egui::ThemePreference::Dark,
        _ => saved_theme(),
    });
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

/// Card: its own background, thin border and rounded corners.
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

/// Card with a title (and an optional line of explanation).
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

/// Colored tag (status).
pub fn chip(ui: &mut egui::Ui, text: &str, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.16))
        .corner_radius(CornerRadius::same(255))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.spacing_mut().interact_size.y = 0.0;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                // as tall as the text, so the dot sits on its middle
                let (r, _) = ui.allocate_exact_size(vec2(7.0, 16.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 3.5, color);
                ui.label(egui::RichText::new(text).color(color).size(12.5));
            });
        })
        .response
}

/// Value that slides to the target (no jumps when the reading changes).
pub fn smooth(ui: &egui::Ui, id: &str, target: f32) -> f32 {
    ui.ctx()
        .animate_value_with_time(ui.make_persistent_id(("smooth", id)), target, 0.45)
}

/// Meter color by level: normal, high, critical.
pub fn level_color(p: &Pal, frac: f32, color: Color32) -> Color32 {
    if frac >= 0.95 {
        p.err
    } else if frac >= 0.85 {
        p.warn
    } else {
        color
    }
}

/// Thin rounded bar.
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

/// Short history (last minute) for the sparklines.
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
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 32.0), Sense::click());
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
    glyph(
        painter,
        Rect::from_center_size(rect.left_center() + vec2(21.0, 0.0), vec2(16.0, 16.0)),
        icon,
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

/// Metric card: name, large value, detail, sparkline of the last minute, a bar that slides
/// to the new value and, optionally, a second bar (e.g. VRAM).
pub struct Metric<'a> {
    pub key: &'a str,
    pub name: &'a str,
    pub big: &'a str,
    pub small: &'a str,
    pub frac: f32,
    pub color: Color32,
    pub spark: Option<&'a Spark>,
    pub extra: Option<(&'a str, f32)>,
    /// Card width (the strip shares the row between its cards).
    pub width: f32,
    /// Leave room for the second bar even without one (same height along the row).
    pub tall: bool,
}

/// Width of each card of a strip of `n` so they share one row (wraps only when they would
/// get too narrow to read).
pub fn metric_width(avail: f32, n: usize, gap: f32) -> f32 {
    let n = n.max(1) as f32;
    ((avail - gap * (n - 1.0)) / n).floor().clamp(176.0, 300.0)
}

pub fn metric(ui: &mut egui::Ui, m: &Metric) -> egui::Response {
    let p = pal(ui);
    let h = if m.extra.is_some() || m.tall {
        126.0
    } else {
        100.0
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(m.width, h), Sense::hover());
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
    // name across the full width (clipped if it does not fit)
    let fit = ((m.width - 28.0) / 6.4) as usize;
    let name: String = if m.name.chars().count() > fit {
        m.name
            .chars()
            .take(fit.saturating_sub(1))
            .collect::<String>()
            + "…"
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
        FontId::new(23.0, FontFamily::Name(BOLD.into())),
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
            pos2(
                inner.right() - (m.width * 0.32).min(84.0),
                inner.top() + 20.0,
            ),
            vec2((m.width * 0.32).min(84.0), 36.0),
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

/// Section title inside a page: semibold, with a hairline under it and air above (the first
/// one of a page gets less).
pub fn title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal(ui);
    if ui.layout().is_horizontal() {
        // in a row with other things (a status, a button): the name alone, no rule
        return ui.label(
            egui::RichText::new(text)
                .size(16.0)
                .family(FontFamily::Name(BOLD.into()))
                .color(p.text),
        );
    }
    ui.add_space(10.0);
    let r = ui.label(
        egui::RichText::new(text)
            .size(16.0)
            .family(FontFamily::Name(BOLD.into()))
            .color(p.text),
    );
    let w = ui.available_width();
    let (line, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
    ui.painter().rect_filled(line, 0.0, p.stroke);
    // a short accent mark under the title, over the hairline
    let mark = Rect::from_min_size(line.min - vec2(0.0, 0.5), vec2(r.rect.width().min(w), 2.0));
    ui.painter()
        .rect_filled(mark, CornerRadius::same(1), p.accent.gamma_multiply(0.9));
    ui.add_space(2.0);
    r
}

/// Label of a group of the side menu.
pub fn nav_group(ui: &mut egui::Ui, text: &str) {
    let p = pal(ui);
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(text.to_uppercase())
                .size(10.5)
                .family(FontFamily::Name(BOLD.into()))
                .color(p.weak.gamma_multiply(0.8))
                .extra_letter_spacing(0.8),
        );
    });
    ui.add_space(1.0);
}

/// Small button that keeps a readable height (egui's own small button has no padding).
pub fn small(ui: &mut egui::Ui, text: impl Into<egui::WidgetText>) -> egui::Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = vec2(9.0, 3.0);
        ui.spacing_mut().interact_size.y = 22.0;
        ui.add(egui::Button::new(text.into().text_style(TextStyle::Small)))
    })
    .inner
}

/// One reading: small label above, value below, in its own tile.
pub fn stat(ui: &mut egui::Ui, label: &str, value: &str, color: Option<Color32>) -> egui::Response {
    let p = pal(ui);
    egui::Frame::new()
        .fill(p.track.gamma_multiply(0.55))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.label(
                    egui::RichText::new(label.to_uppercase())
                        .size(10.0)
                        .color(p.weak)
                        .extra_letter_spacing(0.6),
                );
                ui.label(
                    egui::RichText::new(value)
                        .size(15.0)
                        .family(FontFamily::Name(BOLD.into()))
                        .color(color.unwrap_or(p.text)),
                );
            });
        })
        .response
}

/// Background of a chart: its own fill, a hairline border and a light grid (quarters).
pub fn plot_bg(painter: &egui::Painter, rect: Rect, p: &Pal) {
    painter.rect(
        rect,
        CornerRadius::same(8),
        p.track.gamma_multiply(0.45),
        Stroke::new(1.0, p.stroke),
        egui::StrokeKind::Inside,
    );
    for q in 1..4 {
        let y = rect.top() + rect.height() * q as f32 / 4.0;
        painter.hline(
            rect.x_range().shrink(1.0),
            y,
            Stroke::new(1.0, p.stroke.gamma_multiply(0.55)),
        );
        let x = rect.left() + rect.width() * q as f32 / 4.0;
        painter.vline(
            x,
            rect.y_range().shrink(1.0),
            Stroke::new(1.0, p.stroke.gamma_multiply(0.3)),
        );
    }
}

/// A line with the area under it lightly filled.
pub fn area_line(painter: &egui::Painter, pts: Vec<egui::Pos2>, bottom: f32, color: Color32) {
    if pts.len() < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    let fill = color.gamma_multiply(0.13);
    for w in pts.windows(2) {
        let base = mesh.vertices.len() as u32;
        for q in [w[0], w[1], pos2(w[1].x, bottom), pos2(w[0].x, bottom)] {
            mesh.colored_vertex(q, fill);
        }
        mesh.add_triangle(base, base + 1, base + 2);
        mesh.add_triangle(base, base + 2, base + 3);
    }
    painter.add(mesh);
    painter.add(egui::Shape::line(pts, Stroke::new(1.6, color)));
}

/// Menu icons drawn here, all with the same line weight (the emoji of the fonts had uneven
/// weights). `r` is a 16×16 box.
pub fn glyph(p: &egui::Painter, r: Rect, name: &str, c: Color32) {
    let st = Stroke::new(1.5, c);
    let at = |x: f32, y: f32| pos2(r.left() + x, r.top() + y);
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], st);
    };
    let boxr = |x0: f32, y0: f32, x1: f32, y1: f32, rad: u8| {
        p.rect_stroke(
            Rect::from_min_max(at(x0, y0), at(x1, y1)),
            CornerRadius::same(rad),
            st,
            egui::StrokeKind::Middle,
        );
    };
    let path = |pts: &[(f32, f32)], closed: bool| {
        let v: Vec<_> = pts.iter().map(|q| at(q.0, q.1)).collect();
        if closed {
            p.add(egui::Shape::closed_line(v, st));
        } else {
            p.add(egui::Shape::line(v, st));
        }
    };
    match name {
        // four tiles
        "apps" => {
            for (x, y) in [(1.5, 1.5), (9.0, 1.5), (1.5, 9.0), (9.0, 9.0)] {
                boxr(x, y, x + 5.5, y + 5.5, 1);
            }
        }
        // power button
        "power" => {
            let n = 14;
            let pts: Vec<(f32, f32)> = (0..=n)
                .map(|i| {
                    let a = (-60.0 + 300.0 * i as f32 / n as f32).to_radians();
                    (8.0 + 6.0 * a.cos(), 8.8 - 6.0 * (-a).sin())
                })
                .collect();
            path(&pts, false);
            line((8.0, 1.5), (8.0, 7.5));
        }
        // list with bullets
        "list" => {
            for y in [3.5, 8.0, 12.5] {
                p.circle_filled(at(2.5, y), 1.3, c);
                line((6.0, y), (14.5, y));
            }
        }
        // processor with pins
        "chip" => {
            boxr(3.5, 3.5, 12.5, 12.5, 2);
            boxr(6.5, 6.5, 9.5, 9.5, 0);
            for q in [6.0, 10.0] {
                line((q, 1.0), (q, 3.5));
                line((q, 12.5), (q, 15.0));
                line((1.0, q), (3.5, q));
                line((12.5, q), (15.0, q));
            }
        }
        "bookmark" => path(
            &[
                (4.0, 1.5),
                (12.0, 1.5),
                (12.0, 14.5),
                (8.0, 11.0),
                (4.0, 14.5),
            ],
            true,
        ),
        // graphics card: board, fan, bracket
        "card" => {
            boxr(3.0, 4.0, 15.0, 11.5, 2);
            p.circle_stroke(at(9.5, 7.75), 2.2, st);
            line((1.0, 2.5), (1.0, 14.0));
            line((1.0, 4.0), (3.0, 4.0));
        }
        "monitor" => {
            boxr(1.5, 2.5, 14.5, 11.0, 2);
            line((8.0, 11.0), (8.0, 13.8));
            line((5.0, 13.8), (11.0, 13.8));
        }
        "flask" => {
            path(
                &[
                    (6.2, 1.5),
                    (6.2, 6.0),
                    (2.5, 13.0),
                    (3.3, 14.5),
                    (12.7, 14.5),
                    (13.5, 13.0),
                    (9.8, 6.0),
                    (9.8, 1.5),
                ],
                false,
            );
            line((5.0, 1.5), (11.0, 1.5));
            line((4.6, 10.5), (11.4, 10.5));
        }
        "chat" => path(
            &[
                (2.0, 2.5),
                (14.0, 2.5),
                (14.0, 10.5),
                (7.5, 10.5),
                (4.5, 13.8),
                (4.5, 10.5),
                (2.0, 10.5),
            ],
            true,
        ),
        // screen with a play mark
        "cast" => {
            boxr(1.5, 2.5, 14.5, 12.0, 2);
            path(&[(6.5, 5.0), (10.5, 7.25), (6.5, 9.5)], true);
            line((5.5, 14.5), (10.5, 14.5));
        }
        "sliders" => {
            line((1.5, 5.0), (14.5, 5.0));
            line((1.5, 11.0), (14.5, 11.0));
            p.circle_filled(at(5.5, 5.0), 2.3, c);
            p.circle_filled(at(10.5, 11.0), 2.3, c);
        }
        "check" => {
            boxr(1.5, 1.5, 14.5, 14.5, 3);
            path(&[(4.5, 8.2), (7.0, 10.8), (11.5, 5.4)], false);
        }
        other => {
            p.text(
                r.left_center(),
                egui::Align2::LEFT_CENTER,
                other,
                FontId::proportional(15.0),
                c,
            );
        }
    }
}

/// A "?" that shows an explanation when pointed at: for instructions that do not need to be
/// on the page all the time.
pub fn help(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let p = pal(ui);
    let (r, resp) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
    let c = if resp.hovered() { p.accent } else { p.weak };
    ui.painter()
        .circle_stroke(r.center(), 8.5, Stroke::new(1.2, c));
    ui.painter().text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        "?",
        FontId::new(12.0, FontFamily::Name(BOLD.into())),
        c,
    );
    resp.on_hover_text(text)
}
