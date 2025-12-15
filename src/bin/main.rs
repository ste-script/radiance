// For profiling (see also Cargo.toml)
//use jemallocator::Jemalloc;
//
//#[global_allocator]
//static GLOBAL: Jemalloc = Jemalloc;

extern crate nalgebra as na;

use serde_json::json;
use std::collections::hash_map::Entry;
use std::collections::HashMap;
use std::fs::{self, read_to_string, File};
use std::io::Write;
use std::iter;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

use radiance::{
    ArcTextureViewSampler, AutoDJ, Context, InsertionPoint, Mir, MusicInfo, NodeId, Props,
    RenderTarget, RenderTargetId,
};

mod ui;
use ui::{library, modal, modal_shown, mosaic, UiBg};
use ui::{BeatWidget, SpectrumWidget, WaveformWidget};

mod setup;
use setup::load_default_library;

mod winit_output;
use winit_output::WinitOutput;

const AUTOSAVE_INTERVAL_FRAMES: usize = 60 * 10;
const AUTOSAVE_FILENAME: &str = "autosave.json";

fn autosave(resource_dir: &Path, props: &Props) {
    let inner = || {
        let contents = serde_json::to_string(props).map_err(|e| format!("{:?}", e))?;
        let mut file =
            File::create(resource_dir.join(AUTOSAVE_FILENAME)).map_err(|e| format!("{:?}", e))?;
        file.write_all(contents.as_bytes())
            .map_err(|e| format!("{:?}", e))?;
        Ok(())
    };

    inner().unwrap_or_else(|msg: String| println!("Failed to write autosave file: {}", msg));
}

fn main() {
    env_logger::init();

    // Prepare wgpu
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        flags: wgpu::InstanceFlags::from_env_or_default(),
        memory_budget_thresholds: Default::default(),
        backend_options: wgpu::BackendOptions::from_env_or_default(),
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_features: wgpu::Features::TEXTURE_BINDING_ARRAY,
        // WebGL doesn't support all of wgpu's features, so if
        // we're building for the web we'll have to disable some.
        required_limits: if cfg!(target_arch = "wasm32") {
            wgpu::Limits::downlevel_webgl2_defaults()
        } else {
            wgpu::Limits {
                max_binding_array_elements_per_shader_stage: 500_000,
                ..Default::default()
            }
        },
        label: None,
        memory_hints: Default::default(),
        trace: wgpu::Trace::Off,
        experimental_features: Default::default(),
    }))
    .unwrap();

    // Prepare winit
    let event_loop = EventLoop::new().unwrap();
    event_loop.set_control_flow(ControlFlow::Poll);

    // Prepare & run app
    let mut app = App::new(instance, adapter, device, queue);
    event_loop.run_app(&mut app).unwrap();
}

struct App<'a> {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    mir: Mir,
    ctx: Context,
    props: Props,
    auto_dj_1: Option<AutoDJ>,
    auto_dj_2: Option<AutoDJ>,
    auto_dj_1_enabled: bool,
    auto_dj_2_enabled: bool,
    autosave_timer: usize,
    preview_render_target: (RenderTargetId, RenderTarget),
    waveform_texture: Option<egui::TextureId>,
    spectrum_texture: Option<egui::TextureId>,
    beat_texture: Option<egui::TextureId>,
    left_panel_expanded: bool,
    library_newly_opened: bool,
    insertion_point: InsertionPoint,
    preview_images: HashMap<NodeId, egui::TextureId>,
    winit_output: WinitOutput<'a>,
    app_ui: Option<AppUi>, // Stuff we can't make until we have a window
}

struct AppUi {
    egui_ctx: egui::Context,
    egui_state: egui_winit::State,
    egui_renderer: egui_wgpu::Renderer,
    window: Arc<winit::window::Window>,
    surface_config: wgpu::SurfaceConfiguration,
    surface: wgpu::Surface<'static>,
    waveform_widget: WaveformWidget,
    spectrum_widget: SpectrumWidget,
    beat_widget: BeatWidget,
    ui_bg: UiBg,
    can_draw: bool,
}

#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct BgUniforms {
    opacity: f32,
}

impl App<'_> {
    fn new(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Self {
        let resource_dir = directories::ProjectDirs::from("", "", "Radiance")
            .unwrap()
            .data_local_dir()
            .to_owned();

        if !resource_dir.exists() {
            fs::create_dir_all(&resource_dir).expect("Failed to create resource directory");
        }

        println!("Resource directory is: {}", resource_dir.display());

        load_default_library(&resource_dir);

        // RADIANCE, WOO

        // Make a Mir
        let mir = Mir::new();

        // Make context
        let ctx = Context::new(resource_dir.clone(), &device, &queue);

        let read_autosave_file = || {
            let contents = read_to_string(resource_dir.join(AUTOSAVE_FILENAME))
                .map_err(|e| format!("{:?}", e))?;
            serde_json::from_str(contents.as_str()).map_err(|e| format!("{:?}", e))
        };

        let props = read_autosave_file().unwrap_or_else(|err_string| {
            println!("Failed to read autosave file ({})", err_string);

            // Make a graph
            let node1_id: NodeId =
                serde_json::from_value(json!("node_TW+qCFNoz81wTMca9jRIBg")).unwrap();
            let node2_id: NodeId =
                serde_json::from_value(json!("node_IjPuN2HID3ydxcd4qOsCuQ")).unwrap();
            let node3_id: NodeId =
                serde_json::from_value(json!("node_mW00lTCmDH/03tGyNv3iCQ")).unwrap();
            let node4_id: NodeId =
                serde_json::from_value(json!("node_EdpVLI4KG5JEBRNSgKUzsw")).unwrap();
            let node5_id: NodeId =
                serde_json::from_value(json!("node_I6AAXBaZKvSUfArs2vBr4A")).unwrap();
            let node6_id: NodeId =
                serde_json::from_value(json!("node_I6AAXBaZKvSUfAxs2vBr4A")).unwrap();
            let output_node_id: NodeId =
                serde_json::from_value(json!("node_KSvPLGkiJDT+3FvPLf9JYQ")).unwrap();
            serde_json::from_value(json!({
                "graph": {
                    "nodes": [
                        node1_id,
                        node2_id,
                        node3_id,
                        node4_id,
                        node5_id,
                        node6_id,
                        output_node_id,
                    ],
                    "edges": [
                        {
                            "from": node1_id,
                            "to": node2_id,
                            "input": 0,
                        },
                        {
                            "from": node2_id,
                            "to": node5_id,
                            "input": 1,
                        },
                        {
                            "from": node3_id,
                            "to": node4_id,
                            "input": 0,
                        },
                        {
                            "from": node4_id,
                            "to": node5_id,
                            "input": 0,
                        },
                        {
                            "from": node5_id,
                            "to": output_node_id,
                            "input": 0,
                        },
                        {
                            "from": node6_id,
                            "to": node1_id,
                            "input": 0,
                        },
                    ],
                },
                "node_props": {
                    node1_id.to_string(): {
                        "type": "EffectNode",
                        "name": "purple",
                        "input_count": 1,
                        "intensity": 1.0,
                    },
                    node2_id.to_string(): {
                        "type": "EffectNode",
                        "name": "droste",
                        "input_count": 1,
                        "intensity": 1.0,
                    },
                    node3_id.to_string(): {
                        "type": "EffectNode",
                        "name": "wwave",
                        "input_count": 1,
                        "intensity": 0.6,
                        "frequency": 0.25,
                    },
                    node4_id.to_string(): {
                        "type": "EffectNode",
                        "name": "zoomin",
                        "input_count": 1,
                        "intensity": 0.3,
                        "frequency": 1.0
                    },
                    node5_id.to_string(): {
                        "type": "EffectNode",
                        "name": "uvmap",
                        "input_count": 2,
                        "intensity": 0.2,
                        "frequency": 0.0
                    },
                    node6_id.to_string(): {
                        "type": "ImageNode",
                        "name": "logo.png",
                        "intensity": 1.0,
                    },
                    output_node_id.to_string(): {
                        "type": "UiBgNode",
                        "opacity": 0.2,
                    }
                },
                "time": 0.,
                "dt": 0.03,
            }))
            .unwrap()
        });

        println!("Props: {}", serde_json::to_string(&props).unwrap());

        // Make render targets
        let preview_render_target = (
            serde_json::from_value(json!("rt_LVrjzxhXrGU7SqFo+85zkw")).unwrap(),
            RenderTarget::new(256, 256, 1. / 60.),
        );

        let winit_output = WinitOutput::new(&device);

        App {
            instance,
            adapter,
            device,
            queue,
            mir,
            ctx,
            props,
            auto_dj_1: None,
            auto_dj_2: None,
            auto_dj_1_enabled: false,
            auto_dj_2_enabled: false,
            autosave_timer: 0,
            preview_render_target,
            waveform_texture: None,
            spectrum_texture: None,
            beat_texture: None,
            left_panel_expanded: false,
            library_newly_opened: false,
            insertion_point: Default::default(),
            preview_images: Default::default(),
            winit_output,
            app_ui: None,
        }
    }

    // returns true if present() was called (forcing vsync)
    fn update(&mut self, event_loop: &ActiveEventLoop) -> bool {
        let mut did_vsync = false;

        // Update
        let music_info = self.mir.poll();
        self.props.time = music_info.time;
        self.props.dt = music_info.tempo * (1. / 60.);
        self.props.audio = music_info.audio.clone();

        // See if we need to (re-)create the UI BG render target
        if let Some(app_ui) = &mut self.app_ui {
            let wgpu::SurfaceConfiguration { width, height, .. } = app_ui.surface_config;
            app_ui.ui_bg.create_or_update_render_target(width, height);
        }

        // Merge our render list (preview + bg) and the winit_output render list:
        let (preview_id, preview_rt) = &self.preview_render_target;
        let preview = Some((preview_id, preview_rt));
        let render_target_list = preview
            .into_iter()
            .chain(
                self.app_ui
                    .as_ref()
                    .map(|app_ui| app_ui.ui_bg.render_target())
                    .into_iter(),
            )
            .chain(self.winit_output.render_targets_iter())
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        self.auto_dj_1.as_mut().map(|a| {
            a.update(&mut self.props);

            // Uncheck the checkbox if we broke the AutoDJ
            if a.is_broken() {
                self.auto_dj_1_enabled = false;
            }
        });
        self.auto_dj_2.as_mut().map(|a| {
            a.update(&mut self.props);

            // Uncheck the checkbox if we broke the AutoDJ
            if a.is_broken() {
                self.auto_dj_2_enabled = false;
            }
        });

        self.ctx.update(
            &self.device,
            &self.queue,
            &mut self.props,
            &render_target_list,
        );

        // Autosave if necessary
        // TODO: consider moving this to a background thread
        if self.autosave_timer == 0 {
            autosave(&self.ctx.resource_dir, &self.props);
            self.autosave_timer = AUTOSAVE_INTERVAL_FRAMES;
        } else {
            self.autosave_timer -= 1;
        }

        // Paint the previews
        let radiance_preview_paint_results = {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Encoder"),
                });

            let (preview_render_target_id, _) = self.preview_render_target;
            let results = self.ctx.paint(
                &self.device,
                &self.queue,
                &mut encoder,
                preview_render_target_id,
            );

            self.queue.submit(iter::once(encoder.finish()));
            results
        };

        // Paint the UI BG
        let radiance_ui_bg_paint_results = {
            let mut encoder = self
                .device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("Encoder"),
                });

            if let Some((&bg_render_target_id, _)) = self
                .app_ui
                .as_ref()
                .map(|app_ui| app_ui.ui_bg.render_target())
            {
                let results =
                    self.ctx
                        .paint(&self.device, &self.queue, &mut encoder, bg_render_target_id);

                self.queue.submit(iter::once(encoder.finish()));
                results
            } else {
                Default::default()
            }
        };

        // Run the UI
        {
            let Some(app_ui) = &mut self.app_ui else {
                return did_vsync;
            };
            let raw_input = app_ui.egui_state.take_egui_input(&app_ui.window);
            app_ui.egui_ctx.begin_pass(raw_input);
        }
        self.ui(&music_info, &radiance_preview_paint_results);

        let app_ui = self.app_ui.as_mut().unwrap();
        let full_output = app_ui.egui_ctx.end_pass();

        app_ui
            .egui_state
            .handle_platform_output(&app_ui.window, full_output.platform_output);

        // Construct or destroy the AutoDJs
        match (self.auto_dj_1_enabled, &mut self.auto_dj_1) {
            (false, Some(_)) => {
                self.auto_dj_1 = None;
            }
            (true, None) => {
                self.auto_dj_1 = Some(AutoDJ::new());
            }
            _ => {}
        }
        match (self.auto_dj_2_enabled, &mut self.auto_dj_2) {
            (false, Some(_)) => {
                self.auto_dj_2 = None;
            }
            (true, None) => {
                self.auto_dj_2 = Some(AutoDJ::new());
            }
            _ => {}
        }

        // Update & paint other windows
        if self.winit_output.update(
            event_loop,
            &mut self.ctx,
            &mut self.props,
            &self.instance,
            &self.adapter,
            &self.device,
            &self.queue,
        ) {
            did_vsync = true;
        }

        app_ui.ui_bg.update(
            &self.device,
            &self.queue,
            &self.props,
            &radiance_ui_bg_paint_results,
        );

        // UI GPU update
        let tris = app_ui
            .egui_ctx
            .tessellate(full_output.shapes, full_output.pixels_per_point);

        for (id, image_delta) in &full_output.textures_delta.set {
            app_ui
                .egui_renderer
                .update_texture(&self.device, &self.queue, *id, image_delta);
        }

        let screen_descriptor = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [app_ui.surface_config.width, app_ui.surface_config.height],
            pixels_per_point: app_ui.window.scale_factor() as f32,
        };

        // See if we can present (window is not occluded)
        if !app_ui.can_draw {
            return did_vsync;
        }
        app_ui.can_draw = false;
        app_ui.window.request_redraw();

        let output = app_ui.surface.get_current_texture().unwrap();
        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("Render Encoder"),
            });
        app_ui.egui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &tris,
            &screen_descriptor,
        );
        {
            let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("Render Pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(from_srgb(25, 25, 25, 255)),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw background
            app_ui.ui_bg.render(&self.device, &mut render_pass);

            // Draw EGUI
            app_ui.egui_renderer.render(
                &mut render_pass.forget_lifetime(),
                &tris,
                &screen_descriptor,
            );
        }
        self.queue.submit(std::iter::once(encoder.finish()));

        for id in &full_output.textures_delta.free {
            app_ui.egui_renderer.free_texture(id);
        }
        app_ui.window.pre_present_notify();
        output.present();
        did_vsync = true;
        did_vsync
    }

    fn ui(
        &mut self,
        music_info: &MusicInfo,
        radiance_paint_results: &HashMap<NodeId, ArcTextureViewSampler>,
    ) {
        fn update_or_register_native_texture(
            egui_renderer: &mut egui_wgpu::Renderer,
            device: &wgpu::Device,
            native_texture: &wgpu::TextureView,
            egui_texture: &mut Option<egui::TextureId>,
        ) {
            match egui_texture {
                None => {
                    *egui_texture = Some(egui_renderer.register_native_texture(
                        device,
                        native_texture,
                        wgpu::FilterMode::Linear,
                    ));
                }
                Some(egui_texture) => {
                    egui_renderer.update_egui_texture_from_wgpu_texture(
                        device,
                        native_texture,
                        wgpu::FilterMode::Linear,
                        *egui_texture,
                    );
                }
            }
        }

        let app_ui = self.app_ui.as_mut().unwrap();

        let waveform_size = egui::vec2(330., 65.);
        let spectrum_size = egui::vec2(330., 65.);
        let beat_size = egui::vec2(65., 65.);
        {
            for node_id in self.props.graph.nodes.iter() {
                let native_texture = &radiance_paint_results.get(&node_id).unwrap().view;
                match self.preview_images.entry(*node_id) {
                    Entry::Vacant(e) => {
                        e.insert(app_ui.egui_renderer.register_native_texture(
                            &self.device,
                            native_texture,
                            wgpu::FilterMode::Linear,
                        ));
                    }
                    Entry::Occupied(e) => {
                        app_ui.egui_renderer.update_egui_texture_from_wgpu_texture(
                            &self.device,
                            native_texture,
                            wgpu::FilterMode::Linear,
                            *e.get(),
                        );
                    }
                }
            }

            for (node_id, egui_texture_id) in self.preview_images.iter() {
                if !self.props.graph.nodes.contains(node_id) {
                    app_ui.egui_renderer.free_texture(egui_texture_id);
                }
            }

            // Update & paint widgets

            let waveform_native_texture = app_ui.waveform_widget.paint(
                &self.device,
                &self.queue,
                waveform_size,
                &music_info.audio,
                music_info.uncompensated_unscaled_time,
            );

            update_or_register_native_texture(
                &mut app_ui.egui_renderer,
                &self.device,
                &waveform_native_texture.view,
                &mut self.waveform_texture,
            );

            let spectrum_native_texture = app_ui.spectrum_widget.paint(
                &self.device,
                &self.queue,
                spectrum_size,
                &music_info.spectrum,
            );

            update_or_register_native_texture(
                &mut app_ui.egui_renderer,
                &self.device,
                &spectrum_native_texture.view,
                &mut self.spectrum_texture,
            );

            let beat_native_texture = app_ui.beat_widget.paint(
                &self.device,
                &self.queue,
                beat_size,
                music_info.unscaled_time,
            );

            update_or_register_native_texture(
                &mut app_ui.egui_renderer,
                &self.device,
                &beat_native_texture.view,
                &mut self.beat_texture,
            );
        }

        let left_panel_response = egui::SidePanel::left("left").show_animated(
            &app_ui.egui_ctx,
            self.left_panel_expanded,
            |ui| library::library_ui(ui, &self.ctx, self.library_newly_opened),
        );

        let full_rect = app_ui.egui_ctx.available_rect();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(&app_ui.egui_ctx, |ui| {
                let modal_id = ui.make_persistent_id("modal");
                let modal_shown = modal_shown(&app_ui.egui_ctx, modal_id);

                let egui::InnerResponse {
                    inner: mosaic_response,
                    ..
                } = ui.scope_builder(
                    {
                        let mut builder = egui::UiBuilder::default().max_rect(full_rect);
                        builder.disabled = modal_shown;
                        builder
                    },
                    |ui| {
                        let egui::containers::scroll_area::ScrollAreaOutput {
                            inner: mosaic_response,
                            ..
                        } = egui::containers::scroll_area::ScrollArea::both()
                            .auto_shrink(false)
                            .show(ui, |ui| {
                                ui.add(mosaic(
                                    "mosaic",
                                    &mut self.props,
                                    self.ctx.node_states(),
                                    &self.preview_images,
                                    &mut self.insertion_point,
                                    modal_id,
                                ))
                            });
                        mosaic_response
                    },
                );

                ui.scope_builder(
                    {
                        let mut builder = egui::UiBuilder::default().max_rect(full_rect);
                        builder.disabled = modal_shown;
                        builder
                    },
                    |ui| {
                        egui::Frame::NONE
                            .fill(egui::Color32::from_rgba_premultiplied(25, 25, 25, 250))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.set_min_width(ui.available_width());
                                    ui.image((self.waveform_texture.unwrap(), waveform_size));
                                    ui.image((self.spectrum_texture.unwrap(), spectrum_size));
                                    ui.image((self.beat_texture.unwrap(), beat_size));
                                    ui.checkbox(&mut self.auto_dj_1_enabled, "Auto DJ 1");
                                    ui.checkbox(&mut self.auto_dj_2_enabled, "Auto DJ 2");

                                    ui.label("Global timescale:");
                                    let timescales: &[f32] = &[0.125, 0.25, 0.5, 1., 2., 4., 8.];
                                    fn str_for_timescale(timescale: f32) -> String {
                                        if timescale < 1. {
                                            format!("{}x slower", 1. / timescale)
                                        } else if timescale == 1. {
                                            "1x".to_owned()
                                        } else if timescale > 1. {
                                            format!("{}x faster", timescale)
                                        } else {
                                            format!("{}", timescale)
                                        }
                                    }
                                    egui::ComboBox::from_id_salt("global timescale")
                                        .selected_text(
                                            str_for_timescale(self.mir.global_timescale).as_str(),
                                        )
                                        .show_ui(ui, |ui| {
                                            for &timescale in timescales.iter() {
                                                ui.selectable_value(
                                                    &mut self.mir.global_timescale,
                                                    timescale,
                                                    str_for_timescale(timescale).as_str(),
                                                );
                                            }
                                        });
                                    ui.label("Latency compensation:");
                                    ui.add(
                                        egui::DragValue::new(&mut self.mir.latency_compensation)
                                            .speed(0.001)
                                            .fixed_decimals(3)
                                            .suffix("s")
                                            .range(0. ..=1.),
                                    );
                                });
                            });

                        if !self.left_panel_expanded && ui.input(|i| i.key_pressed(egui::Key::A)) {
                            self.left_panel_expanded = true;
                            self.library_newly_opened = true;
                        }

                        if let Some(egui::InnerResponse {
                            inner: library_response,
                            response: _,
                        }) = left_panel_response
                        {
                            // Reset the focus flag after it's been used
                            self.library_newly_opened = false;

                            match library_response {
                                library::LibraryResponse::AddNode(node_props) => {
                                    let new_node_id = NodeId::gen();
                                    self.props.node_props.insert(new_node_id, node_props);
                                    self.props
                                        .graph
                                        .insert_node(new_node_id, &self.insertion_point);
                                    self.left_panel_expanded = false;
                                    mosaic_response.request_focus();
                                }
                                library::LibraryResponse::Close => {
                                    self.left_panel_expanded = false;
                                    mosaic_response.request_focus();
                                }
                                library::LibraryResponse::None => {}
                            }
                        }
                    },
                );

                if modal_shown {
                    ui.scope_builder(egui::UiBuilder::default().max_rect(full_rect), |ui| {
                        ui.add(modal(
                            modal_id,
                            &mut self.props,
                            self.ctx.node_states(),
                            &self.preview_images,
                        ));
                    });
                }
            });
    }

    fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        let Some(app_ui) = &mut self.app_ui else {
            return;
        };
        if new_size.width > 0 && new_size.height > 0 {
            app_ui.surface_config.width = new_size.width;
            app_ui.surface_config.height = new_size.height;
            app_ui
                .surface
                .configure(&self.device, &app_ui.surface_config);
        }
    }
}

impl AppUi {
    fn new(app: &App, window: winit::window::Window) -> Self {
        // Make egui context
        let egui_ctx = egui::Context::default();
        egui_ctx.set_theme(egui::Theme::Dark);
        egui_ctx.style_mut(|style| {
            style.interaction.selectable_labels = false;
            style.visuals.handle_shape = egui::style::HandleShape::Circle;
        });

        // Make egui state
        let egui_state = egui_winit::State::new(
            egui_ctx.clone(),
            egui::ViewportId::ROOT,
            &window,
            None,
            None,
            None,
        );

        let window = Arc::new(window);

        let size = window.inner_size();
        let surface = app.instance.create_surface(window.clone()).unwrap();
        let surface_caps = surface.get_capabilities(&app.adapter);
        let surface_format = surface_caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or(surface_caps.formats[0]);

        let surface_config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface_format,
            width: size.width,
            height: size.height,
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: surface_caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };

        surface.configure(&app.device, &surface_config);

        let egui_renderer = egui_wgpu::Renderer::new(
            &app.device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

        // Make widgets
        let pixels_per_point = window.scale_factor() as f32;
        let waveform_widget = WaveformWidget::new(&app.device, pixels_per_point);
        let spectrum_widget = SpectrumWidget::new(&app.device, pixels_per_point);
        let beat_widget = BeatWidget::new(&app.device, pixels_per_point);

        // Make BG
        let ui_bg = UiBg::new(&app.device, surface_format);

        AppUi {
            window,
            surface_config,
            surface,
            egui_ctx,
            egui_state,
            egui_renderer,
            waveform_widget,
            spectrum_widget,
            beat_widget,
            ui_bg,
            can_draw: false,
        }
    }
}

impl ApplicationHandler for App<'_> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.app_ui.is_none() {
            let window_attributes = Window::default_attributes()
                .with_title("Radiance")
                .with_maximized(true);

            let window = event_loop.create_window(window_attributes).unwrap();
            self.app_ui = Some(AppUi::new(&self, window));
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(app_ui) = &mut self.app_ui else {
            return;
        };

        if self
            .winit_output
            .window_event(window_id, &event, &self.device)
        {
            // Event handled by another window
            return;
        }

        if window_id != app_ui.window.id() {
            return;
        }

        let response = app_ui.egui_state.on_window_event(&app_ui.window, &event);

        if response.consumed {
            return;
        }

        match event {
            WindowEvent::CloseRequested => {
                // This assignment prevents the app from segfaulting on exit
                // I think this is a bug in winit that may be fixed in a future version.
                self.app_ui = None;
                event_loop.exit();
            }
            WindowEvent::Resized(physical_size) => {
                self.resize(physical_size);
            }
            WindowEvent::RedrawRequested => {
                app_ui.can_draw = true;
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let did_vsync = self.update(event_loop);
        if did_vsync {
            event_loop.set_control_flow(ControlFlow::Poll);
        } else {
            // If we didn't vsync as part of rendering (e.g. all windows occluded,)
            // fall back to a 60 FPS rate timed on the CPU
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_secs_f64(1. / 60.),
            ));
        }
    }
}

// From https://github.com/three-rs/three/blob/07e47da5e0673aa9a16526719e16debd59040eec/src/color.rs#L39
fn from_srgb(r: u8, g: u8, b: u8, a: u8) -> wgpu::Color {
    let f = |xu| {
        let x = xu as f64 / 255.0;
        if x > 0.04045 {
            ((x + 0.055) / 1.055).powf(2.4)
        } else {
            x / 12.92
        }
    };
    wgpu::Color {
        r: f(r),
        g: f(g),
        b: f(b),
        a: f(a),
    }
}
