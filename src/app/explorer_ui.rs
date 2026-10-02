//! The rule explorer window: a grid of live low-resolution Life-like simulations.

use std::sync::Arc;

use eframe::egui_wgpu;
use eframe::wgpu;

use super::App;
use super::explorer::{LifelikeRule, random_batch};
use super::state::PresetSource;
use crate::preset::builtin::{BUILTINS, load_builtin};
use crate::preset::{InitPattern, Mode, Preset};
use crate::shader::assemble::{DEFAULT_POST, assemble_post, assemble_render, assemble_rule};
use crate::shader::params::{pack_params, params_wgsl, parse_params};
use crate::shader::validate::{ShaderFile, validate};
use crate::sim::{GpuContext, SimConfig, Simulation};

const COLUMNS: usize = 4;
const COUNT: usize = 16;
const GRID: u32 = 64;
const THUMB_PX: u32 = 128;

struct Thumb {
    rule: LifelikeRule,
    sim: Simulation,
    texture: egui::TextureId,
}

pub struct Explorer {
    thumbs: Vec<Thumb>,
    seed: u64,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    ctx: Arc<GpuContext>,
}

impl Explorer {
    pub fn new(ctx: Arc<GpuContext>, renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>, seed: u64) -> Self {
        let mut e = Explorer { thumbs: Vec::new(), seed, renderer, ctx };
        e.shuffle(seed);
        e
    }

    /// Replaces every thumbnail with a fresh random rule.
    pub fn shuffle(&mut self, seed: u64) {
        self.free_textures();
        self.seed = seed;
        let render_src = load_builtin(&BUILTINS[2]).render; // Life's age-coloured render
        let specs = parse_params(&render_src).unwrap_or_default();
        let pw = params_wgsl(&specs);
        let (Ok(render), Ok(post)) = (
            validate(ShaderFile::Render, &assemble_render(&render_src, &pw)),
            validate(ShaderFile::Post, &assemble_post(DEFAULT_POST, &pw)),
        ) else {
            return;
        };
        self.thumbs = random_batch(seed, COUNT)
            .into_iter()
            .enumerate()
            .filter_map(|(i, rule)| {
                let config = SimConfig {
                    mode: Mode::TwoD,
                    width: GRID,
                    height: GRID,
                    init: InitPattern::Random { density: 0.4 },
                    seed: seed as u32 ^ i as u32,
                };
                let mut sim = Simulation::new(self.ctx.clone(), config);
                sim.disable_history(); // thumbnails never rewind
                let rule_a = validate(ShaderFile::Rule, &assemble_rule(&rule.rule_wgsl(), &pw)).ok()?;
                sim.set_pipelines(&rule_a, &render, &post).ok()?;
                sim.set_params(pack_params(&specs, &Default::default()));
                sim.ensure_scene_size(THUMB_PX, THUMB_PX);
                let view = sim.scene_view()?;
                let texture =
                    self.renderer.write().register_native_texture(&self.ctx.device, view, wgpu::FilterMode::Nearest);
                Some(Thumb { rule, sim, texture })
            })
            .collect();
    }

    fn free_textures(&mut self) {
        {
            let mut r = self.renderer.write();
            for t in &self.thumbs {
                r.free_texture(&t.texture);
            }
        }
        self.thumbs.clear();
    }

    /// Advances every thumbnail one step and re-renders it.
    pub fn tick(&mut self) {
        let mut enc =
            self.ctx.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("ca explorer") });
        for t in &mut self.thumbs {
            t.sim.poll_pipeline_check();
            t.sim.step(&mut enc, 1);
            t.sim.render_scene(&mut enc);
        }
        self.ctx.queue.submit([enc.finish()]);
    }

    /// Draws the grid; returns the rule the user clicked, if any.
    pub fn ui(&mut self, ui: &mut egui::Ui) -> Option<LifelikeRule> {
        let mut picked = None;
        egui::Grid::new("explorer-grid").spacing([8.0, 8.0]).show(ui, |ui| {
            for (i, t) in self.thumbs.iter().enumerate() {
                ui.vertical(|ui| {
                    let img = egui::Image::new((t.texture, egui::vec2(THUMB_PX as f32, THUMB_PX as f32)))
                        .corner_radius(4)
                        .sense(egui::Sense::click());
                    if ui.add(img).on_hover_text("Click to load this rule").clicked() {
                        picked = Some(t.rule);
                    }
                    ui.label(egui::RichText::new(t.rule.notation()).monospace().small());
                });
                if (i + 1) % COLUMNS == 0 {
                    ui.end_row();
                }
            }
        });
        picked
    }
}

impl Drop for Explorer {
    fn drop(&mut self) {
        self.free_textures();
    }
}

/// A preset for `rule`: generated rule shader plus Life's render shader.
pub fn preset_for_rule(rule: LifelikeRule) -> Preset {
    let mut p = load_builtin(&BUILTINS[2]);
    p.meta.name = format!("Life-like {}", rule.notation());
    p.meta.init = InitPattern::Random { density: 0.4 };
    p.rule = rule.rule_wgsl();
    p
}

/// The explorer window (when open): ticks the thumbnails, shows them, handles Shuffle and picks.
pub fn window(app: &mut App, ctx: &egui::Context) {
    let Some(explorer) = app.explorer.as_mut() else { return };
    explorer.tick();
    let mut open = true;
    let mut picked = None;
    let mut shuffle = false;
    egui::Window::new("Rule explorer")
        .open(&mut open)
        .resizable(false)
        .default_pos(ctx.content_rect().center() - egui::vec2(300.0, 320.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Random Life-like rules, running live. Click one to load it.").weak());
                if ui.button("Shuffle").clicked() {
                    shuffle = true;
                }
            });
            picked = explorer.ui(ui);
        });
    if shuffle {
        let seed = explorer.seed.wrapping_add(1);
        explorer.shuffle(seed);
    }
    if let Some(rule) = picked {
        app.load_preset(preset_for_rule(rule), PresetSource::Imported);
        app.state.modified = true;
    }
    if !open {
        app.explorer = None;
    }
    ctx.request_repaint();
}
