//! Cloud field in Iced's existing wgpu renderer. No new rendering dependency.
use crate::cloud;
use iced::{
    Rectangle, mouse, wgpu,
    widget::shader::{self, Pipeline, Primitive, Viewport},
};
use std::borrow::Cow;
#[derive(Clone)]
pub struct Program {
    pub field: cloud::Field,
    pub dark: bool,
    pub clouds: bool,
    pub group: bool,
}
impl<M> shader::Program<M> for Program {
    type State = ();
    type Primitive = Draw;
    fn draw(&self, _: &(), _: mouse::Cursor, bounds: Rectangle) -> Draw {
        let w = bounds.width.max(1.) as f64;
        let h = bounds.height.max(1.) as f64;
        let f = self.field.frame(w, h, self.dark, self.group);
        let mut data = [[0_f32; 4]; 17];
        data[0] = [w as f32, h as f32, f.tau as f32, f.warp as f32];
        // Reduced motion freezes existing texture time/warp without changing the field preset.
        data[1] = [f.cap as f32, 1.9, 0.35, if self.clouds { 1. } else { 0. }];
        data[2] = [
            f.bg[0] as f32,
            f.bg[1] as f32,
            f.bg[2] as f32,
            if self.dark { 1.55 } else { 1. },
        ];
        data[3] = [
            if self.dark { 0.45 } else { 0. },
            if self.dark { 0.075 } else { 0.13 },
            0.,
            f.fade as f32,
        ];
        let short = w.min(h);
        let patch = |y: f64, rx: f64, ry: f64| {
            [
                0.,
                ((y - h / 2.) / short) as f32,
                (rx / short) as f32,
                (ry / short) as f32,
            ]
        };
        // Header/identity, group host line, controls. Never drop a third patch.
        data[4] = patch(
            if self.group { 80. } else { h * 0.50 },
            w * 0.44,
            if self.group { 90. } else { 120. },
        );
        data[5] = patch(h - 172., w * 0.42, 48.);
        data[6] = patch(h - 82., w * 0.47, 92.);
        for (i, l) in f.lobes.iter().enumerate() {
            data[7 + i] = [l.x as f32, l.y as f32, l.sigma as f32, l.alpha as f32];
            data[12 + i] = [l.lab[0] as f32, l.lab[1] as f32, l.lab[2] as f32, 0.];
        }
        Draw { data }
    }
}
#[derive(Debug)]
pub struct Draw {
    data: [[f32; 4]; 17],
}
pub struct Gpu {
    pipeline: wgpu::RenderPipeline,
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
    srgb: bool,
}
impl Pipeline for Gpu {
    fn new(device: &wgpu::Device, _: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Tinline cloud field"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(include_str!("cloud.wgsl"))),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Tinline cloud field"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview: None,
            cache: None,
        });
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("cloud uniforms"),
            size: 272,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cloud uniforms"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });
        Self {
            pipeline,
            buffer,
            bind,
            srgb: format.is_srgb(),
        }
    }
}
impl Primitive for Draw {
    type Pipeline = Gpu;
    fn prepare(
        &self,
        pipeline: &mut Gpu,
        _: &wgpu::Device,
        queue: &wgpu::Queue,
        _: &Rectangle,
        _: &Viewport,
    ) {
        let mut data = self.data;
        data[3][2] = if pipeline.srgb { 1. } else { 0. };
        let mut bytes = [0_u8; 272];
        for (i, v) in data.iter().flatten().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        queue.write_buffer(&pipeline.buffer, 0, &bytes);
    }
    fn draw(&self, pipeline: &Gpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        pass.set_pipeline(&pipeline.pipeline);
        pass.set_bind_group(0, &pipeline.bind, &[]);
        pass.draw(0..3, 0..1);
        true
    }
}
/// Compile/link on the available adapter without opening a window or microphone.
pub fn validate() -> Result<(), String> {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .map_err(|e| e.to_string())?
        .block_on(async {
            let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
            let adapter = instance
                .request_adapter(&wgpu::RequestAdapterOptions::default())
                .await
                .map_err(|e| e.to_string())?;
            let (device, queue) = adapter
                .request_device(&wgpu::DeviceDescriptor::default())
                .await
                .map_err(|e| e.to_string())?;
            device.push_error_scope(wgpu::ErrorFilter::Validation);
            let _ = Gpu::new(&device, &queue, wgpu::TextureFormat::Bgra8UnormSrgb);
            if let Some(e) = device.pop_error_scope().await {
                return Err(e.to_string());
            }
            println!("Cloud WGSL pipeline validated on {:?}", adapter.get_info());
            Ok(())
        })
}
