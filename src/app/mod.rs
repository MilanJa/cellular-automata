pub mod state;

pub struct App;

impl App {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        App
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.label("Cellular Automata Shader IDE");
    }
}
