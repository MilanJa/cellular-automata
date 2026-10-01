fn main() -> eframe::Result {
    env_logger::init();
    // Optional: `--preset <builtin-id | folder>` selects the preset loaded at startup.
    let mut args = std::env::args().skip(1);
    let mut start_preset: Option<String> = None;
    while let Some(a) = args.next() {
        match a.as_str() {
            "--preset" | "-p" => start_preset = args.next(),
            "--help" | "-h" => {
                println!("usage: cellular-automata [--preset <builtin-id | folder>]");
                return Ok(());
            }
            _ => {}
        }
    }
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
        Box::new(move |cc| {
            Ok(Box::new(cellular_automata::app::App::with_preset(cc, start_preset.as_deref())))
        }),
    )
}
