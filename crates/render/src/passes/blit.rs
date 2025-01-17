// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at http://mozilla.org/MPL/2.0/.

use crate::{GlobalRenderResources, SWAPCHAIN_FORMAT};

pub struct BlitPass {
    bind_group_layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    render_bundle: wgpu::RenderBundle,
}

impl BlitPass {
    pub fn new(render_resources: &GlobalRenderResources, input: &wgpu::TextureView) -> Self {
        let bind_group_layout = create_bind_group_layout(&render_resources.device);
        let pipeline = create_blit_pipeline(&render_resources.device, &bind_group_layout);
        let render_bundle = create_blit_render_bundle(
            &render_resources.device,
            &bind_group_layout,
            &render_resources.linear_sampler,
            input,
            &pipeline,
        );

        Self {
            bind_group_layout,
            pipeline,
            render_bundle,
        }
    }

    pub fn run(&self, encoder: &mut wgpu::CommandEncoder, frame: &wgpu::TextureView) {
        let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: frame,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.8,
                        g: 0.8,
                        b: 0.8,
                        a: 1.0,
                    }),
                    store: true,
                },
            })],
            depth_stencil_attachment: None,
        });

        rpass.execute_bundles(Some(&self.render_bundle));
    }

    pub fn update(&mut self, render_resources: &GlobalRenderResources, input: &wgpu::TextureView) {
        self.render_bundle = create_blit_render_bundle(
            &render_resources.device,
            &self.bind_group_layout,
            &render_resources.linear_sampler,
            input,
            &self.pipeline,
        );
    }
}

fn create_bind_group_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::NonFiltering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    })
}

fn create_blit_pipeline(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[bind_group_layout],
        push_constant_ranges: &[],
    });

    let vert_shader = device.create_shader_module(wgpu::include_wgsl!("fullscreen.wgsl"));
    let frag_shader = device.create_shader_module(if cfg!(target_arch = "wasm32") {
        wgpu::include_wgsl!("blit-web.wgsl")
    } else {
        wgpu::include_wgsl!("blit-native.wgsl")
    });

    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &vert_shader,
            entry_point: "fullscreen",
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &frag_shader,
            entry_point: "main",
            targets: &[Some(SWAPCHAIN_FORMAT.into())],
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList, // doesn't matter
            strip_index_format: None,
            front_face: wgpu::FrontFace::Cw,
            cull_mode: Some(wgpu::Face::Front),
            unclipped_depth: false,
            polygon_mode: wgpu::PolygonMode::Fill,
            conservative: false,
        },
        depth_stencil: None,
        multisample: wgpu::MultisampleState {
            count: 1,
            mask: !0,
            alpha_to_coverage_enabled: false,
        },
        multiview: None,
    })
}

fn create_blit_render_bundle(
    device: &wgpu::Device,
    bind_group_layout: &wgpu::BindGroupLayout,
    linear_sampler: &wgpu::Sampler,
    input_texture: &wgpu::TextureView,
    blit_pipeline: &wgpu::RenderPipeline,
) -> wgpu::RenderBundle {
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: bind_group_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Sampler(linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(input_texture),
            },
        ],
    });

    let mut encoder = device.create_render_bundle_encoder(&wgpu::RenderBundleEncoderDescriptor {
        label: None,
        color_formats: &[Some(SWAPCHAIN_FORMAT)],
        depth_stencil: None,
        sample_count: 1,
        multiview: None,
    });

    encoder.set_pipeline(blit_pipeline);
    encoder.set_bind_group(0, &bind_group, &[]);
    encoder.draw(0..3, 0..1);
    encoder.finish(&wgpu::RenderBundleDescriptor { label: None })
}

// End of File
