//! Renders the tutorial presets to the PNGs that `docs/tutorial/` embeds:
//!
//!     cargo run --example render_docs
//!
//! Needs a GPU adapter. Every preset in `TUTORIAL` is run headlessly for a number of steps and
//! exported through its render and post shaders, exactly like **Image → Export PNG** does.

use std::collections::BTreeMap;
use std::path::Path;

use cellular_automata::app::state::{build_shaders, preset_to_state, resolve_values};
use cellular_automata::preset::builtin::{Builtin, TUTORIAL, load_builtin};
use cellular_automata::shader::params::pack_params;
use cellular_automata::sim::{GpuContext, Simulation};
use eframe::wgpu;

/// Tutorial preset id, simulation steps before the picture, pixels per cell, output name.
const SHOTS: &[(&str, u32, u32, &str)] = &[
    ("tut03_majority", 6, 2, "03-majority"),
    ("tut04_life", 150, 2, "04-life"),
    ("tut05_colour", 120, 2, "05-colour"),
    ("tut06_seeds", 400, 2, "06-seeds"),
    ("tut07_gray_scott", 3200, 2, "07-gray-scott"),
    ("tut08_elementary", 255, 1, "08-elementary"),
];

const OUT_DIR: &str = "docs/tutorial/images";

fn main() -> anyhow::Result<()> {
    let out_dir = Path::new(OUT_DIR);
    std::fs::create_dir_all(out_dir)?;
    let instance = wgpu::Instance::default();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))?;
    let ctx = GpuContext::new(device, queue, wgpu::TextureFormat::Bgra8UnormSrgb);
    for (id, steps, scale, name) in SHOTS {
        let builtin = tutorial(id)?;
        let png = render(&ctx, builtin, *steps, *scale)?;
        let path = out_dir.join(format!("{name}.png"));
        std::fs::write(&path, png)?;
        println!("wrote {}", path.display());
    }
    Ok(())
}

fn tutorial(id: &str) -> anyhow::Result<&'static Builtin> {
    TUTORIAL.iter().find(|b| b.id == id).ok_or_else(|| anyhow::anyhow!("no tutorial preset `{id}`"))
}

/// Runs `builtin` for `steps` steps from its own init and returns the exported PNG bytes.
fn render(ctx: &std::sync::Arc<GpuContext>, builtin: &Builtin, steps: u32, scale: u32) -> anyhow::Result<Vec<u8>> {
    let preset = load_builtin(builtin);
    let (editor, config, _, toml_params) = preset_to_state(&preset);
    let built = build_shaders(&editor).map_err(|e| anyhow::anyhow!("{}: {e:?}", builtin.id))?;
    let mut sim = Simulation::new(ctx.clone(), config);
    sim.disable_history();
    sim.set_pipelines(&built.rule, &built.render, &built.post, built.seed.as_ref())
        .map_err(|e| anyhow::anyhow!("{}: {e:?}", builtin.id))?;
    let values = resolve_values(&built.specs, &toml_params, &BTreeMap::new());
    sim.set_params(pack_params(&built.specs, &values));
    // Step in chunks so no single command buffer grows huge.
    let mut remaining = steps;
    while remaining > 0 {
        let n = remaining.min(64);
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        sim.step(&mut encoder, n);
        ctx.queue.submit([encoder.finish()]);
        remaining -= n;
    }
    sim.start_export(scale, format!("{}.png", builtin.id)).map_err(anyhow::Error::msg)?;
    let image = loop {
        let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
        if let Some(result) = sim.poll_export() {
            break result.map_err(anyhow::Error::msg)?;
        }
    };
    image.to_png()
}
