//! General settings tab.

use crate::app::{card, dropdown, save_bar, THEMES};
use crate::theme::Theme;
use eframe::egui::{Context, RichText, TextEdit, Ui};

pub fn render_general_tab(
    ui: &mut Ui,
    theme: &Theme,
    ctx: &Context,
    app: &mut crate::app::SettingsApp,
) {
    ui.add_space(4.0);
    card(ui, theme, |ui| {
        dropdown(ui, theme, "theme", "Theme", THEMES, &mut app.theme_index);
        ui.add_space(4.0);

        ui.label(RichText::new("Monitor").color(theme.text_dim).size(13.0));
        ui.add_space(2.0);
        let mut monitor = app.config.monitor.clone().unwrap_or_default();
        let response = ui.add(
            TextEdit::singleline(&mut monitor)
                .hint_text("Default")
                .desired_width(ui.available_width())
                .font(eframe::egui::FontId::proportional(13.0)),
        );
        if response.changed() {
            app.config.monitor = if monitor.is_empty() {
                None
            } else {
                Some(monitor)
            };
        }
    });

    save_bar(ui, theme, ctx, app);
}
