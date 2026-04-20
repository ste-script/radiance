use crate::context::{ArcTextureViewSampler, Context, RenderTargetState};
use crate::render_target::RenderTargetId;
use crate::CommonNodeProps;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::num::NonZeroU32;
use std::string::String;

mod preprocess_shader;
use preprocess_shader::preprocess_shader;

const EFFECT_HEADER: &str = include_str!("effect_header.wgsl");
const EFFECT_FOOTER: &str = include_str!("effect_footer.wgsl");
const INTENSITY_INTEGRAL_PERIOD: f32 = 1024.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnimationMode {
    None,
    SineWave,
    BeatSync,
    Ramp,
}

impl Default for AnimationMode {
    fn default() -> Self {
        AnimationMode::None
    }
}

/// Properties of an EffectNode.
/// Fields that are None are set to their default when the shader is loaded.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EffectNodeProps {
    pub name: String,
    pub intensity: Option<f32>,
    pub frequency: Option<f32>,
    pub input_count: Option<u32>,
    #[serde(default)]
    pub animation_mode: AnimationMode,
}

impl From<&EffectNodeProps> for CommonNodeProps {
    fn from(props: &EffectNodeProps) -> Self {
        CommonNodeProps {
            input_count: props.input_count,
        }
    }
}

#[allow(clippy::large_enum_variant)]
pub enum EffectNodeState {
    Uninitialized,
    Ready(EffectNodeStateReady),
    Error_(String), // ambiguous_associated_items error triggered by derive_more::TryInto without the _
}

pub struct EffectNodeStateReady {
    // Cached props
    name: String,
    intensity: f32,
    frequency: f32,
    input_count: u32,

    // Computed Info
    intensity_integral: f32,

    // Read from the effect file:
    // How many channels this effect uses
    // Must be at least 1--the output channel.
    // 2 or greater means that the effect uses some intermediate channels
    channel_count: u32,

    // GPU resources
    bind_group_2_layout: wgpu::BindGroupLayout,
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    render_pipelines: Vec<wgpu::RenderPipeline>,

    // Paint states
    paint_states: HashMap<RenderTargetId, EffectNodePaintState>,
}

struct EffectNodePaintState {
    textures: Vec<ArcTextureViewSampler>,
    phase: usize,
    pass_output_indices: Vec<usize>,
    cached_input_texture_ids: Vec<usize>,
    bind_groups_2: Vec<wgpu::BindGroup>,
}

// The uniform buffer associated with the effect (agnostic to render target)
#[repr(C)]
#[derive(Default, Debug, Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    audio: [f32; 4],
    time: f32,
    frequency: f32,
    intensity: f32,
    intensity_integral: f32,
    resolution: [f32; 2],
    dt: f32,
    _padding: [u8; 4],
}

#[allow(dead_code)]
fn handle_shader_error(error: wgpu::Error) {
    eprintln!("wgpu error: {}\n", error);
}

// This is a state machine, it's more natural to use `match` than `if let`
#[allow(clippy::single_match)]
impl EffectNodeState {
    fn animated_intensity(mode: AnimationMode, time: f32, frequency: f32, audio_level: f32) -> f32 {
        match mode {
            AnimationMode::None => 0.0,
            AnimationMode::SineWave => {
                // `time` is in beats, so frequency controls cycles per beat.
                0.5 + 0.5 * (std::f32::consts::TAU * time * frequency).sin()
            }
            AnimationMode::BeatSync => (audio_level * 1.25).clamp(0.0, 1.0),
            // iTime in shaders wraps every 16 beats, so mirror that behavior here.
            AnimationMode::Ramp => (time * frequency / 16.0).fract(),
        }
    }

    fn setup_render_pipeline(
        ctx: &Context,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        props: &EffectNodeProps,
    ) -> Result<EffectNodeStateReady, String> {
        let name = &props.name;

        // Shader
        let source_name = format!("{name}.wgsl");
        let effect_source = ctx.fetch_library_content(&source_name).map_err(|_| {
            format!("Failed to read effect shader file from library: \"{source_name}\"")
        })?;

        let (effect_sources_processed, shader_input_count, default_frequency) =
            preprocess_shader(&effect_source)?;

        let input_count = match props.input_count {
            Some(input_count) => {
                if shader_input_count != input_count {
                    return Err(
                        "Shader input count does not match input count declared in graph"
                            .to_string(),
                    );
                }
                input_count
            }
            None => shader_input_count,
        };

        let channel_count: u32 = effect_sources_processed.len() as u32;

        // Default to 0 intensity if none given
        let intensity = props.intensity.unwrap_or(0.);
        let frequency = props.frequency.unwrap_or(default_frequency);

        let shader_sources = effect_sources_processed
            .iter()
            .map(|effect_source_processed| {
                format!(
                    "{}\n{}\n{}\n",
                    EFFECT_HEADER, effect_source_processed, EFFECT_FOOTER
                )
            });
        let shader_modules = shader_sources.enumerate().map(|(i, shader_source)| {
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(&format!("EffectNode {} channel {}", name, i)),
                source: wgpu::ShaderSource::Wgsl(shader_source.into()),
            });
            let result = pollster::block_on(device.pop_error_scope());
            if let Some(error) = result {
                return Err(format!(
                    "EffectNode shader compilation error: {} channel {}: {}\n",
                    name, i, error
                ));
            }
            Ok(shader_module)
        });

        // This is some serious rust wizardry. A Vec<Result> is quietly made into a Result<Vec>.
        let shader_modules: Result<Vec<wgpu::ShaderModule>, String> = shader_modules.collect();
        let shader_modules = shader_modules?;

        // The uniforms bind group:
        let bind_group_1_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0, // Uniforms
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some(&format!(
                    "EffectNode {} bind group layout 1 (uniforms)",
                    name
                )),
            });
        let bind_group_2_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0, // iSampler
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1, // iInputsTex
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: NonZeroU32::new(input_count),
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2, // iNoiseTex
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 3, // iChannelsTex
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: NonZeroU32::new(channel_count),
                    },
                ],
                label: Some(&format!(
                    "EffectNode {} bind group layout 2 (textures)",
                    name
                )),
            });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("Render Pipeline Layout"),
                bind_group_layouts: &[&bind_group_1_layout, &bind_group_2_layout],
                push_constant_ranges: &[],
            });

        // Create a render pipeline for each channel in an effect
        let render_pipelines: Vec<wgpu::RenderPipeline> = shader_modules
            .into_iter()
            .map(|shader_module| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some(&format!("EffectNode {} render pipeline", name)),
                    layout: Some(&render_pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &shader_module,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: Default::default(),
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &shader_module,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: wgpu::TextureFormat::Rgba16Float,
                            blend: None,
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: Default::default(),
                    }),
                    primitive: wgpu::PrimitiveState {
                        topology: wgpu::PrimitiveTopology::TriangleStrip,
                        strip_index_format: None,
                        front_face: wgpu::FrontFace::Ccw,
                        cull_mode: Some(wgpu::Face::Back),
                        polygon_mode: wgpu::PolygonMode::Fill,
                        unclipped_depth: false,
                        conservative: false,
                    },
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState {
                        count: 1,
                        mask: !0,
                        alpha_to_coverage_enabled: false,
                    },
                    multiview: None,
                    cache: None,
                })
            })
            .collect();

        // The update uniform buffer for this effect
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(&format!("EffectNode {} uniform buffer", name)),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            layout: &bind_group_1_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
            label: Some(&format!("EffectNode {} bind group 1 (uniforms)", name)),
        });

        // The sampler that will be used for texture access within the shaders
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::MirrorRepeat,
            address_mode_v: wgpu::AddressMode::MirrorRepeat,
            address_mode_w: wgpu::AddressMode::MirrorRepeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        Ok(EffectNodeStateReady {
            name: name.clone(),
            intensity,
            frequency,
            input_count,
            intensity_integral: 0.,
            channel_count,
            bind_group_2_layout,
            uniform_buffer,
            uniform_bind_group,
            sampler,
            render_pipelines,
            paint_states: HashMap::new(),
        })
    }

    fn build_pass_output_indices(texture_count: usize) -> Vec<usize> {
        let channel_count = texture_count - 1;
        let mut pass_output_indices = Vec::with_capacity(texture_count * channel_count);

        for phase in 0..texture_count {
            let mut slot_order: Vec<usize> = (0..texture_count)
                .map(|offset| (phase + offset) % texture_count)
                .collect();

            for channel in (0..channel_count).rev() {
                pass_output_indices.push(slot_order[channel_count]);
                slot_order.swap(channel, channel_count);
            }
        }

        pass_output_indices
    }

    fn new_paint_state(
        self_ready: &EffectNodeStateReady,
        _ctx: &Context,
        device: &wgpu::Device,
        _queue: &wgpu::Queue,
        render_target_state: &RenderTargetState,
    ) -> EffectNodePaintState {
        let texture_desc = wgpu::TextureDescriptor {
            size: wgpu::Extent3d {
                width: render_target_state.width(),
                height: render_target_state.height(),
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            label: None,
            view_formats: &[wgpu::TextureFormat::Rgba16Float],
        };

        let make_texture = || {
            let texture = device.create_texture(&texture_desc);
            let view = texture.create_view(&Default::default());
            let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                address_mode_w: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            });
            ArcTextureViewSampler::new(texture, view, sampler)
        };

        let texture_count = self_ready.channel_count as usize + 1;
        let textures: Vec<ArcTextureViewSampler> = (0..texture_count)
            .map(|_| make_texture())
            .collect();

        EffectNodePaintState {
            textures,
            phase: 0,
            pass_output_indices: Self::build_pass_output_indices(texture_count),
            cached_input_texture_ids: Vec::new(),
            bind_groups_2: Vec::new(),
        }
    }

    fn update_bind_group_2_cache(
        bind_group_2_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        input_count: usize,
        channel_count: usize,
        ctx: &Context,
        device: &wgpu::Device,
        render_target_state: &RenderTargetState,
        paint_state: &mut EffectNodePaintState,
        inputs: &[Option<ArcTextureViewSampler>],
    ) {
        let input_textures: Vec<&ArcTextureViewSampler> = (0..input_count)
            .map(|index| match inputs.get(index) {
                Some(Some(texture)) => texture,
                _ => ctx.blank_texture(),
            })
            .collect();
        let input_texture_ids: Vec<usize> = input_textures
            .iter()
            .map(|texture| std::sync::Arc::as_ptr(&texture.view) as usize)
            .collect();

        if input_texture_ids == paint_state.cached_input_texture_ids
            && !paint_state.bind_groups_2.is_empty()
        {
            return;
        }

        paint_state.cached_input_texture_ids = input_texture_ids;
        paint_state.bind_groups_2.clear();

        let input_binding: Vec<&wgpu::TextureView> = input_textures
            .iter()
            .map(|texture| texture.view.as_ref())
            .collect();
        let texture_count = paint_state.textures.len();
        paint_state
            .bind_groups_2
            .reserve(texture_count * channel_count);

        for phase in 0..texture_count {
            let mut slot_order: Vec<usize> = (0..texture_count)
                .map(|offset| (phase + offset) % texture_count)
                .collect();

            for channel in (0..channel_count).rev() {
                let channels: Vec<&wgpu::TextureView> = slot_order[..channel_count]
                    .iter()
                    .map(|&index| paint_state.textures[index].view.as_ref())
                    .collect();
                let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    layout: bind_group_2_layout,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::Sampler(sampler),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::TextureViewArray(
                                input_binding.as_slice(),
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 2,
                            resource: wgpu::BindingResource::TextureView(
                                &render_target_state.noise_texture().view,
                            ),
                        },
                        wgpu::BindGroupEntry {
                            binding: 3,
                            resource: wgpu::BindingResource::TextureViewArray(channels.as_slice()),
                        },
                    ],
                    label: Some("EffectNode bind group 2 (textures)"),
                });
                paint_state.bind_groups_2.push(bind_group);
                slot_order.swap(channel, channel_count);
            }
        }
    }

    fn update_paint_states(
        self_ready: &mut EffectNodeStateReady,
        ctx: &Context,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) {
        // See if we need to add or remove any paint states
        // (based on the context's render targets)

        self_ready
            .paint_states
            .retain(|id, _| ctx.render_target_states().contains_key(id));

        for (check_render_target_id, render_target_state) in ctx.render_target_states().iter() {
            if !self_ready.paint_states.contains_key(check_render_target_id) {
                self_ready.paint_states.insert(
                    *check_render_target_id,
                    Self::new_paint_state(self_ready, ctx, device, queue, render_target_state),
                );
            }
        }
    }

    pub fn new(
        ctx: &Context,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        props: &EffectNodeProps,
    ) -> Self {
        // TODO kick of shader compilation in the background instead of blocking
        match Self::setup_render_pipeline(ctx, device, queue, props) {
            Ok(mut new_obj_ready) => {
                Self::update_paint_states(&mut new_obj_ready, ctx, device, queue);
                Self::Ready(new_obj_ready)
            }
            Err(msg) => {
                eprintln!("Unable to configure EffectNode: {}", msg);
                Self::Error_(msg)
            }
        }
    }

    pub fn update(
        &mut self,
        ctx: &Context,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        props: &mut EffectNodeProps,
    ) {
        match self {
            EffectNodeState::Ready(self_ready) => {
                if props.name != self_ready.name {
                    *self = EffectNodeState::Error_(
                        "EffectNode name changed after construction".to_string(),
                    );
                    return;
                }
                match props.input_count {
                    Some(input_count) => {
                        // Caller passed in an input_count, we should validate it
                        if input_count != self_ready.input_count {
                            *self = EffectNodeState::Error_(
                                "EffectNode input_count changed after construction".to_string(),
                            );
                            return;
                        }
                    }
                    _ => {}
                }
                match props.intensity {
                    Some(intensity) => {
                        // Cache the intensity for when paint() is called
                        self_ready.intensity = intensity;
                    }
                    _ => {}
                }
                match props.frequency {
                    Some(frequency) => {
                        // Cache the frequency for when paint() is called
                        self_ready.frequency = frequency;
                    }
                    _ => {}
                }

                if props.animation_mode != AnimationMode::None {
                    self_ready.intensity = Self::animated_intensity(
                        props.animation_mode,
                        ctx.time,
                        self_ready.frequency,
                        ctx.audio.level,
                    );
                }

                // Report back to the caller what our props are
                self_ready.update_props(props);

                Self::update_paint_states(self_ready, ctx, device, queue);

                // Accumulate intensity_integral
                self_ready.intensity_integral = (self_ready.intensity_integral
                    + self_ready.intensity * ctx.dt)
                    % INTENSITY_INTEGRAL_PERIOD;
            }
            _ => {}
        }
    }

    pub fn paint(
        &mut self,
        ctx: &Context,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        render_target_id: RenderTargetId,
        inputs: &[Option<ArcTextureViewSampler>],
    ) -> ArcTextureViewSampler {
        match self {
            EffectNodeState::Ready(self_ready) => {
                let render_target_state = ctx
                    .render_target_state(render_target_id)
                    .expect("Call to paint() with a render target ID unknown to the context");
                let bind_group_2_layout = &self_ready.bind_group_2_layout;
                let sampler = &self_ready.sampler;
                let input_count = self_ready.input_count as usize;
                let channel_count = self_ready.channel_count as usize;
                let render_pipelines = &self_ready.render_pipelines;
                let uniform_bind_group = &self_ready.uniform_bind_group;
                let paint_state = self_ready.paint_states.get_mut(&render_target_id).expect("Call to paint() with a render target ID unknown to the node (did you call update() first?)");

                // Populate the uniforms
                {
                    let width = render_target_state.width();
                    let height = render_target_state.height();
                    let uniforms = Uniforms {
                        audio: [
                            ctx.audio.low,
                            ctx.audio.mid,
                            ctx.audio.high,
                            ctx.audio.level,
                        ],
                        time: ctx.time,
                        frequency: self_ready.frequency,
                        intensity: self_ready.intensity,
                        intensity_integral: self_ready.intensity_integral,
                        resolution: [width as f32, height as f32],
                        dt: render_target_state.dt(),
                        ..Default::default()
                    };
                    queue.write_buffer(
                        &self_ready.uniform_buffer,
                        0,
                        bytemuck::cast_slice(&[uniforms]),
                    );
                }

                Self::update_bind_group_2_cache(
                    bind_group_2_layout,
                    sampler,
                    input_count,
                    channel_count,
                    ctx,
                    device,
                    render_target_state,
                    paint_state,
                    inputs,
                );

                // Render all channels in reverse order
                let phase = paint_state.phase;
                for (pass_index, channel) in (0..channel_count).rev().enumerate() {
                    let bind_group_2_index = phase * channel_count + pass_index;
                    let output_texture_index = paint_state.pass_output_indices[bind_group_2_index];
                    let bind_group_2 = &paint_state.bind_groups_2[bind_group_2_index];

                    {
                        let mut render_pass =
                            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("EffectNode render pass"),
                                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                    view: paint_state.textures[output_texture_index].view.as_ref(),
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Load,
                                        store: wgpu::StoreOp::Store,
                                    },
                                    depth_slice: None,
                                })],
                                depth_stencil_attachment: None,
                                timestamp_writes: None,
                                occlusion_query_set: None,
                            });

                        render_pass.set_pipeline(&render_pipelines[channel]);
                        render_pass.set_bind_group(0, uniform_bind_group, &[]);
                        render_pass.set_bind_group(1, bind_group_2, &[]);
                        render_pass.draw(0..4, 0..1);
                    }
                }

                paint_state.phase = (paint_state.phase + 1) % paint_state.textures.len();
                paint_state.textures[paint_state.phase].clone()
            }
            _ => inputs
                .first()
                .cloned()
                .flatten()
                .unwrap_or_else(|| ctx.blank_texture().clone()),
        }
    }
}

impl EffectNodeStateReady {
    fn update_props(&self, props: &mut EffectNodeProps) {
        props.name.clone_from(&self.name);
        props.intensity = Some(self.intensity);
        props.frequency = Some(self.frequency);
        props.input_count = Some(self.input_count);
    }
}

//impl EffectNodePaintState {
//    pub fn new(width: u32, height: u32) -> EffectNodePaintState {
//        let texture_desc = wgpu::TextureDescriptor {
//            size: wgpu::Extent3d {
//                width,
//                height,
//                depth_or_array_layers: 1,
//            },
//            mip_level_count: 1,
//            sample_count: 1,
//            view_dimension: wgpu::TextureDimension::D2,
//            format: wgpu::TextureFormat::Rgba8UnormSrgb,
//            usage: wgpu::TextureUsages::COPY_SRC
//                | wgpu::TextureUsages::RENDER_ATTACHMENT
//                ,
//            label: None,
//        };
//        let texture = self.device.create_texture(&texture_desc);
//
//        EffectNodePaintState {
//            width,
//            height,
//            texture,
//        }
//    }
//
//}
