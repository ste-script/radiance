use egui::{Align, DragValue, Layout, TextureId, Ui};
use radiance::{AutoDJFlowNodeProps, AutoDJFlowNodeState};

const NORMAL_HEIGHT: f32 = 240.;
const NORMAL_WIDTH: f32 = 220.;

pub struct AutoDJFlowNodeTile<'a> {
    _preview_image: TextureId,
    config: &'a mut radiance::AutoDJFlowConfig,
}

impl<'a> AutoDJFlowNodeTile<'a> {
    /// Returns a Vec with one entry for each props.input_count
    /// corresponding to the minimum allowable height for that input port.
    /// If there are no input ports, this function should return a 1-element Vec.
    pub fn min_input_heights(_props: &AutoDJFlowNodeProps) -> Vec<f32> {
        (0..1).map(|_| NORMAL_HEIGHT).collect()
    }

    /// Calculates the width of the tile, given its height.
    pub fn width_for_height(_props: &AutoDJFlowNodeProps, height: f32) -> f32 {
        NORMAL_WIDTH.min(0.8 * height)
    }

    /// Creates a new visual tile
    /// (builder pattern; this is not a stateful UI component)
    pub fn new(
        props: &'a mut AutoDJFlowNodeProps,
        _state: &'a AutoDJFlowNodeState,
        preview_image: TextureId,
    ) -> Self {
        Self {
            _preview_image: preview_image,
            config: &mut props.config,
        }
    }

    /// Render the contents of the AutoDJFlowNodeTile (presumably into a Tile)
    pub fn add_contents(self, ui: &mut Ui) {
        let AutoDJFlowNodeTile { config, .. } = self;

        ui.heading("Auto DJ Flow");
        ui.checkbox(&mut config.enabled, "Enabled");

        ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
            ui.label("Stable range (frames)");
            ui.horizontal(|ui| {
                ui.label("Min");
                ui.add(DragValue::new(&mut config.timing.stable_timer_min).range(1..=20000));
                ui.label("Max");
                ui.add(DragValue::new(&mut config.timing.stable_timer_max).range(1..=20000));
            });

            if config.timing.stable_timer_max < config.timing.stable_timer_min {
                config.timing.stable_timer_max = config.timing.stable_timer_min;
            }

            ui.label("Crossfade (frames)");
            ui.add(DragValue::new(&mut config.timing.crossfade_timer).range(1..=20000));

            ui.separator();
            ui.label(format!("Scenes: {}", config.scenes.len()));
            if !config.scenes.is_empty() {
                if config.active_scene >= config.scenes.len() {
                    config.active_scene = config.scenes.len() - 1;
                }
                ui.label(format!(
                    "Active scene: {}",
                    config.scenes[config.active_scene].name
                ));
            }
        });
    }
}
