fn main() -> eframe::Result {
    env_logger::init();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Cellular Automata Shader IDE")
            .with_inner_size([1400.0, 900.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "cellular-automata",
        options,
        Box::new(|cc| Ok(Box::new(cellular_automata::app::App::new(cc)))),
    )
}
