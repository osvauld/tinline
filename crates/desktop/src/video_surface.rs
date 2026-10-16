//! The peer's video picture: I420 planes uploaded into textures that live as long as the call, converted to RGB in the
//! fragment shader (BT.601 limited, as `video::convert::i420_to_rgba`), drawn by iced's wgpu Shader widget.
//!
//! Why not `image::Handle::from_rgba` per frame: iced_wgpu uploads raster images of 2 MiB or more on a worker thread
//! and draws nothing for that handle until the upload finishes (`image/cache.rs`, `MAX_SYNC_SIZE`). A new handle every
//! frame therefore leaves the stage empty (the backdrop shows) for part of every frame once the picture is that big:
//! 540x960 and up at 4 bytes a pixel, which is every HD portrait call from a phone. This widget never has a frame
//! without a picture: until the next upload lands, the last one stays on screen.

use iced::widget::shader::{self, Pipeline, Primitive, Viewport};
use iced::{mouse, wgpu, Element, Length, Rectangle};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static SEQ: AtomicU64 = AtomicU64::new(1);

/// One decoded picture: tightly packed I420 (`video::I420` layout).
pub struct Picture {
    pub width: u32,
    pub height: u32,
    data: Vec<u8>,
    /// Which picture this is; the GPU side uploads each one once.
    seq: u64,
}

impl std::fmt::Debug for Picture {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Picture {}x{} #{}", self.width, self.height, self.seq)
    }
}

impl Picture {
    /// `None` when `data` is not `width * height` of I420.
    pub fn new(width: u32, height: u32, data: Vec<u8>) -> Option<Picture> {
        let ok = width > 0 && height > 0 && data.len() == video::I420::len(width as usize, height as usize);
        ok.then(|| Picture { width, height, data, seq: SEQ.fetch_add(1, Ordering::Relaxed) })
    }

    fn planes(&self) -> (&[u8], &[u8], &[u8]) {
        let (w, h) = (self.width as usize, self.height as usize);
        let (cw, ch) = video::I420::chroma(w, h);
        let (y, rest) = self.data.split_at(w * h);
        let (u, v) = rest.split_at(cw * ch);
        (y, u, v)
    }
}

struct Prog {
    pic: Arc<Picture>,
}

impl<M> shader::Program<M> for Prog {
    type State = ();
    type Primitive = Draw;

    fn draw(&self, _: &(), _: mouse::Cursor, _: Rectangle) -> Draw {
        Draw { pic: self.pic.clone() }
    }
}

#[derive(Debug)]
pub struct Draw {
    pic: Arc<Picture>,
}

/// The three planes as `R8Unorm` textures for one picture size.
struct Planes {
    width: u32,
    height: u32,
    y: wgpu::Texture,
    u: wgpu::Texture,
    v: wgpu::Texture,
    bind: wgpu::BindGroup,
}

pub struct Gpu {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    uniforms: wgpu::Buffer,
    srgb: bool,
    planes: Option<Planes>,
    /// The picture in `planes`.
    seq: u64,
}

fn plane(device: &wgpu::Device, w: u32, h: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("video plane"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn upload(queue: &wgpu::Queue, t: &wgpu::Texture, w: usize, h: usize, bytes: &[u8]) {
    queue.write_texture(
        t.as_image_copy(),
        bytes,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(w as u32), rows_per_image: Some(h as u32) },
        wgpu::Extent3d { width: w as u32, height: h as u32, depth_or_array_layers: 1 },
    );
}

impl Pipeline for Gpu {
    fn new(device: &wgpu::Device, _: &wgpu::Queue, format: wgpu::TextureFormat) -> Gpu {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("video surface"),
            source: wgpu::ShaderSource::Wgsl(std::borrow::Cow::Borrowed(include_str!("video_surface.wgsl"))),
        });
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("video surface"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                tex(2),
                tex(3),
                tex(4),
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("video surface"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("video surface"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleStrip, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })],
            }),
            multiview: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("video surface uniforms"),
            size: 32,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Gpu { pipeline, layout, sampler, uniforms, srgb: format.is_srgb(), planes: None, seq: 0 }
    }
}

impl Primitive for Draw {
    type Pipeline = Gpu;

    fn prepare(&self, p: &mut Gpu, device: &wgpu::Device, queue: &wgpu::Queue, bounds: &Rectangle, _: &Viewport) {
        let pic = &*self.pic;
        let (w, h) = (pic.width, pic.height);
        let (cw, ch) = video::I420::chroma(w as usize, h as usize);
        if p.planes.as_ref().is_none_or(|t| (t.width, t.height) != (w, h)) {
            let (y, u, v) = (plane(device, w, h), plane(device, cw as u32, ch as u32), plane(device, cw as u32, ch as u32));
            let view = |t: &wgpu::Texture| t.create_view(&Default::default());
            let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("video surface"),
                layout: &p.layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: p.uniforms.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&p.sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view(&y)) },
                    wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&view(&u)) },
                    wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&view(&v)) },
                ],
            });
            p.planes = Some(Planes { width: w, height: h, y, u, v, bind });
            p.seq = 0;
        }
        let t = p.planes.as_ref().expect("planes made above");
        if p.seq != pic.seq {
            let (py, pu, pv) = pic.planes();
            upload(queue, &t.y, w as usize, h as usize, py);
            upload(queue, &t.u, cw, ch, pu);
            upload(queue, &t.v, cw, ch, pv);
            p.seq = pic.seq;
        }
        // Contain: the picture's box inside the widget, as fractions of it (the viewport is the widget).
        let s = (bounds.width / w as f32).min(bounds.height / h as f32);
        let (fx, fy) = (w as f32 * s / bounds.width.max(1.0), h as f32 * s / bounds.height.max(1.0));
        let u = [fx, fy, if p.srgb { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0, 0.0, 0.0];
        let mut bytes = [0u8; 32];
        for (i, v) in u.iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_ne_bytes());
        }
        queue.write_buffer(&p.uniforms, 0, &bytes);
    }

    fn draw(&self, p: &Gpu, pass: &mut wgpu::RenderPass<'_>) -> bool {
        let Some(t) = &p.planes else { return true };
        pass.set_pipeline(&p.pipeline);
        pass.set_bind_group(0, &t.bind, &[]);
        pass.draw(0..4, 0..1);
        true
    }
}

/// The picture, contained in its parent and centred.
pub fn surface<'a, M: 'a>(pic: Arc<Picture>) -> Element<'a, M> {
    iced::widget::shader(Prog { pic }).width(Length::Fill).height(Length::Fill).into()
}
