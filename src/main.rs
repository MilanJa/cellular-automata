#[cfg(not(target_arch = "wasm32"))]
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

/// Browser entry point: mounts the app on the `#ca_canvas` element with WebGPU only
/// (compute shaders have no WebGL fallback). `?preset=<id>` selects the startup preset.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast;
    use eframe::{egui_wgpu, wgpu};

    eframe::WebLogger::init(log::LevelFilter::Warn).ok();

    let mut setup = egui_wgpu::WgpuSetupCreateNew::without_display_handle();
    setup.instance_descriptor.backends = wgpu::Backends::BROWSER_WEBGPU;
    let web_options = eframe::WebOptions {
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: egui_wgpu::WgpuConfiguration {
            wgpu_setup: egui_wgpu::WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        ..Default::default()
    };

    wasm_bindgen_futures::spawn_local(async move {
        let document = web_sys::window()
            .and_then(|w| w.document())
            .expect("no document");
        let canvas = document
            .get_element_by_id("ca_canvas")
            .expect("missing #ca_canvas")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#ca_canvas is not a canvas");
        let start = cellular_automata::platform::startup_preset_from_url();
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                web_options,
                Box::new(move |cc| {
                    Ok(Box::new(cellular_automata::app::App::with_preset(cc, start.as_deref())))
                }),
            )
            .await;
        if let Some(loading) = document.get_element_by_id("loading") {
            match result {
                Ok(()) => loading.remove(),
                Err(e) => {
                    loading.set_inner_html(
                        "<p><b>Could not start.</b> This app needs a browser with WebGPU \
                         (recent Chrome or Edge, Firefox 141+, Safari 26+).</p>",
                    );
                    log::error!("failed to start eframe: {e:?}");
                }
            }
        }
    });
}
