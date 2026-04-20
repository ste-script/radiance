use crate::auto_dj::AutoDJFlowConfig;
use crate::context::{ArcTextureViewSampler, Context};
use crate::render_target::RenderTargetId;
use crate::CommonNodeProps;
use serde::{Deserialize, Serialize};

/// Properties of an AutoDJFlowNode.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AutoDJFlowNodeProps {
    #[serde(default)]
    pub config: AutoDJFlowConfig,
}

impl From<&AutoDJFlowNodeProps> for CommonNodeProps {
    fn from(_props: &AutoDJFlowNodeProps) -> Self {
        CommonNodeProps {
            input_count: Some(1),
        }
    }
}

pub struct AutoDJFlowNodeState {}

impl AutoDJFlowNodeState {
    pub fn new(
        _ctx: &Context,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _props: &AutoDJFlowNodeProps,
    ) -> Self {
        Self {}
    }

    pub fn update(
        &mut self,
        _ctx: &Context,
        _device: &wgpu::Device,
        _queue: &wgpu::Queue,
        _props: &mut AutoDJFlowNodeProps,
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
