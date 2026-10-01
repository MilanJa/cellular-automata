use super::App;
use crate::shader::highlight::layouter;
use crate::shader::validate::ShaderFile;

fn editor_id(file: ShaderFile) -> egui::Id {
    egui::Id::new(("wgsl-editor", file.label()))
}

/// Character offset of the first character on 1-based `line`.
fn char_index_of_line(text: &str, line: usize) -> usize {
    text.split_inclusive('\n').take(line.saturating_sub(1)).map(|l| l.chars().count()).sum()
}

fn one_editor(app: &mut App, ui: &mut egui::Ui, file: ShaderFile, title: &str) {
    let dirty = app.state.editor.dirty;
    let header = format!("{title}{}", if dirty { " *" } else { "" });
    egui::CollapsingHeader::new(header).default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Apply (Ctrl+Enter)").clicked() {
                app.apply_shaders();
            }
            let n_err = app.state.errors.iter().filter(|e| e.file == file).count();
            if n_err > 0 {
                ui.colored_label(egui::Color32::from_rgb(230, 90, 90), format!("{n_err} error(s)"));
            }
        });
        let id = editor_id(file);
        // Jump the cursor when an error entry for this file was clicked.
        if let Some((_, line)) = app.pending_cursor.take_if(|(f, _)| *f == file) {
            let text = match file {
                ShaderFile::Rule => &app.state.editor.rule,
                ShaderFile::Render => &app.state.editor.render,
            };
            let idx = char_index_of_line(text, line);
            let mut st =
                egui::widgets::text_edit::TextEditState::load(ui.ctx(), id).unwrap_or_default();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(idx))));
            st.store(ui.ctx(), id);
            ui.memory_mut(|m| m.request_focus(id));
        }
        let text = match file {
            ShaderFile::Rule => &mut app.state.editor.rule,
            ShaderFile::Render => &mut app.state.editor.render,
        };
        let mut lay = layouter();
        let out = egui::TextEdit::multiline(text)
            .id(id)
            .code_editor()
            .font(egui::FontId::monospace(13.0))
            .desired_rows(14)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .layouter(&mut lay)
            .show(ui);
        if out.response.changed() {
            app.state.editor.dirty = true;
        }
    });
}

pub fn show(app: &mut App, ui: &mut egui::Ui) {
    one_editor(app, ui, ShaderFile::Rule, "Rule (WGSL)");
    one_editor(app, ui, ShaderFile::Render, "Render (WGSL)");
}

#[cfg(test)]
mod tests {
    use super::char_index_of_line;

    #[test]
    fn char_index_of_line_counts_chars_before_line() {
        let text = "ab\ncde\nf";
        assert_eq!(char_index_of_line(text, 1), 0);
        assert_eq!(char_index_of_line(text, 2), 3);
        assert_eq!(char_index_of_line(text, 3), 7);
        assert_eq!(char_index_of_line(text, 99), 8);
    }
}
