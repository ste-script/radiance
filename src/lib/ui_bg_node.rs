use crate::context::{ArcTextureViewSampler, Context};
use crate::render_target::RenderTargetId;
use crate::CommonNodeProps;
use serde::{Deserialize, Serialize};

/// Properties of a UiBgNode.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UiBgNodeProps {
    #[serde(default)]
    pub opacity: f32,
}

impl From<&UiBgNodeProps> for CommonNodeProps {
    fn from(_props: &UiBgNodeProps) -> Self {
        CommonNodeProps {
            input_count: Some(1),
        }
    }
}

pub struct UiBgNodeState {
}

impl UiBgNodeState {
    pub fn new(
        _ctx: &Context,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _props: &UiBgNodeProps,
    ) -> Self {
        Self {}
    }

    pub fn update(
        &mut self,
        _ctx: &Context,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _props: &mut UiBgNodeProps,
    ) {
    }

    pub fn paint(
        &mut self,
        ctx: &Context,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _encoder: &mut wgpu::CommandEncoder,
        _render_target_id: RenderTargetId,
        inputs: &[Option<ArcTextureViewSampler>],
    ) -> ArcTextureViewSampler {
        inputs
            .first()
            .cloned()
            .flatten()
            .unwrap_or_else(|| ctx.blank_texture().clone())
    }
}
