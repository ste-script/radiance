use egui::{vec2, Align, Layout, Slider, TextureId, Ui};
use radiance::{UiBgNodeProps, UiBgNodeState};

const PREVIEW_ASPECT_RATIO: f32 = 1.;
const NORMAL_HEIGHT: f32 = 300.;
const NORMAL_WIDTH: f32 = 220.;

pub struct UiBgNodeTile<'a> {
    preview_image: TextureId,
    opacity: &'a mut f32,
}

impl<'a> UiBgNodeTile<'a> {
    /// Returns a Vec with one entry for each props.input_count
    /// corresponding to the minimum allowable height for that input port.
    /// If there are no input ports, this function should return a 1-element Vec.
    pub fn min_input_heights(_props: &UiBgNodeProps) -> Vec<f32> {
        // TODO Simplify this to just be a single f32
        (0..1).map(|_| NORMAL_HEIGHT).collect()
    }

    /// Calculates the width of the tile, given its height.
    pub fn width_for_height(_props: &UiBgNodeProps, height: f32) -> f32 {
        NORMAL_WIDTH.min(0.5 * height)
    }

    /// Creates a new visual tile
    /// (builder pattern; this is not a stateful UI component)
    pub fn new(
        props: &'a mut UiBgNodeProps,
        _state: &'a UiBgNodeState,
        preview_image: TextureId,
    ) -> Self {
        UiBgNodeTile {
            preview_image,
            opacity: &mut props.opacity,
        }
    }

    /// Render the contents of the UiBgNodeTile (presumably into a Tile)
    pub fn add_contents(self, ui: &mut Ui) {
        let UiBgNodeTile {
            preview_image,
            opacity,
        } = self;

        ui.heading("UI BG");
        // Preserve aspect ratio
        ui.with_layout(
            Layout::bottom_up(Align::Center).with_cross_justify(true),
            |ui| {
                ui.spacing_mut().slider_width = ui.available_width();
                ui.add(Slider::new(opacity, 0.0..=1.0).show_value(false));
                ui.centered_and_justified(|ui| {
                    let image_size = ui.available_size();
                    let image_size = (image_size * vec2(1., 1. / PREVIEW_ASPECT_RATIO)).min_elem()
                        * vec2(1., PREVIEW_ASPECT_RATIO);
                    ui.image((preview_image, image_size));
                });
            },
        );
    }
}
