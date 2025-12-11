use std::collections::HashMap;

use radiance::{ArcTextureViewSampler, NodeId, RenderTarget, RenderTargetId};

#[derive(Debug)]
pub struct UiBg {
    _bg_shader_module: wgpu::ShaderModule,
    _bg_bind_group_1_layout: wgpu::BindGroupLayout,
    bg_bind_group_2_layout: wgpu::BindGroupLayout,
    _bg_render_pipeline_layout: wgpu::PipelineLayout,
    bg_render_pipeline: wgpu::RenderPipeline,
    bg_render_target: Option<(RenderTargetId, RenderTarget)>,
}

impl UiBg {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        // Set up WGPU resources for drawing the UI BG
        let bg_shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("BG output shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("bg.wgsl").into()),
        });
        let bg_bind_group_1_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0, // UpdateUniforms
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
                label: Some("bg bind group layout 1 (uniforms)"),
            });
        let bg_bind_group_2_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
                label: Some("bg bind group layout 2 (textures)"),
            });
        let bg_render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("bg render pipeline layout"),
                bind_group_layouts: &[&bg_bind_group_2_layout], // XXX &bg_bind_group_1_layout,
                push_constant_ranges: &[],
            });

        // Make BG render pipeline
        let bg_render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bg render pipeline"),
            layout: Some(&bg_render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &bg_shader_module,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &bg_shader_module,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
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
        });

        Self {
            _bg_shader_module: bg_shader_module,
            _bg_bind_group_1_layout: bg_bind_group_1_layout,
            bg_bind_group_2_layout,
            _bg_render_pipeline_layout: bg_render_pipeline_layout,
            bg_render_pipeline,
            bg_render_target: None,
        }
    }

    pub fn render_target(&self) -> Option<(&RenderTargetId, &RenderTarget)> {
        self.bg_render_target
            .as_ref()
            .map(|(render_target_id, render_target)| (render_target_id, render_target))
    }

    pub fn maybe_resize(&mut self, width: u32, height: u32) {
        if !self
            .bg_render_target
            .as_ref()
            .is_some_and(|(_, rt)| rt.width() == width && rt.height() == height)
        {
            self.bg_render_target = Some((
                RenderTargetId::gen(),
                RenderTarget::new(width, height, 1. / 60.),
            ));
        }
    }

    pub fn update<'a>(
        &self,
        props: &radiance::Props,
        paint_results: &'a HashMap<NodeId, ArcTextureViewSampler>,
    ) -> Vec<(NodeId, &'a ArcTextureViewSampler)> {
        // Collect and upload data related to the UI BG drawing
        let mut bg_textures: Vec<_> = paint_results
            .iter()
            .filter_map(|(&node_id, texture)| {
                props
                    .node_props
                    .get(&node_id)
                    .and_then(|node_state| <&radiance::UiBgNodeProps>::try_from(node_state).ok())
                    .map(|_ui_bg_node_state| (node_id, texture))
            })
            .collect();
        // Sort to maintain a stable superposition
        bg_textures.sort_by_key(|&(node_id, _)| node_id);
        bg_textures
    }

    pub fn render(
        &self,
        device: &wgpu::Device,
        render_pass: &mut wgpu::RenderPass,
        bg_textures: &[(NodeId, &ArcTextureViewSampler)],
    ) {
        // Draw the UI BG
        for (_, texture) in bg_textures.into_iter() {
            let bg_bind_group_2 = device.create_bind_group(&wgpu::BindGroupDescriptor {
                layout: &self.bg_bind_group_2_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&texture.view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&texture.sampler),
                    },
                ],
                label: Some("bg bind group"),
            });

            render_pass.set_pipeline(&self.bg_render_pipeline);
            render_pass.set_bind_group(0, &bg_bind_group_2, &[]);
            render_pass.draw(0..6, 0..1);
        }
    }
}
