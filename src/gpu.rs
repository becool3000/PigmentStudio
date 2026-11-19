use std::num::NonZeroU64;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use wgpu::util::DeviceExt;

pub const SIDE: u32 = 1024;
pub const PALETTE_WIDTH: u32 = 512;
pub const PALETTE_HEIGHT: u32 = 256;
pub const ATLAS_HEIGHT: u32 = SIDE + PALETTE_HEIGHT;
pub const HISTORY: usize = 8;
pub const MAX_COMMANDS: usize = 1024;
const PAINT_BYTES: u64 = SIDE as u64 * ATLAS_HEIGHT as u64 * 32;
const BRUSH_BYTES: u64 = 272;
const PNG_BYTES: u64 = SIDE as u64 * SIDE as u64 * 4;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Canvas,
    Palette,
}

impl Surface {
    pub fn bounds(self) -> [u32; 4] {
        match self {
            Self::Canvas => [0, 0, SIDE, SIDE],
            Self::Palette => [0, SIDE, PALETTE_WIDTH, PALETTE_HEIGHT],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Brush {
    Round,
    Flat,
}

impl Brush {
    pub fn index(self) -> usize {
        usize::from(self == Self::Flat)
    }
    fn load_start(self) -> u32 {
        if self == Self::Round {
            0
        } else {
            1
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Dab {
    bounds: [u32; 4],
    brush: [f32; 4],
    axis: [f32; 4],
    mode: [u32; 4],
    tubes: [u32; 4],
}

impl Dab {
    pub fn new(
        position: egui::Pos2,
        radius: f32,
        pressure: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
    ) -> Self {
        let [origin_x, origin_y, w, h] = surface.bounds();
        let extent = |coordinate: f32, limit: u32| {
            let start = (coordinate - radius * 1.07).floor().clamp(0., limit as f32) as u32;
            let end = (coordinate + radius * 1.07).ceil().clamp(0., limit as f32) as u32;
            (start, end - start)
        };
        let (x, width) = extent(position.x, w);
        let (y, height) = extent(position.y, h);
        let (sin, cos) = angle.sin_cos();
        Self {
            bounds: [x + origin_x, y + origin_y, width, height],
            brush: [
                position.x + origin_x as f32,
                position.y + origin_y as f32,
                radius,
                pressure,
            ],
            axis: [cos, sin, 0., 0.],
            mode: [0, brush.load_start(), 0, 0],
            tubes: [0; 4],
        }
    }

    pub fn ink(position: egui::Pos2, pressure: f32, dwell: bool) -> Self {
        let radius = Self::ink_radius(pressure);
        let mut dab = Self::new(
            position,
            radius,
            pressure,
            Surface::Canvas,
            Brush::Round,
            0.,
        );
        dab.mode = [9, 0, u32::from(dwell), 0];
        dab
    }

    pub fn ink_radius(pressure: f32) -> f32 {
        1.3 + 2.8 * pressure
    }

    fn operation(bounds: [u32; 4], operation: u32) -> Self {
        Self {
            bounds,
            brush: [0.; 4],
            axis: [0.; 4],
            mode: [operation, 0, 0, 0],
            tubes: [0; 4],
        }
    }
}

pub enum Command {
    Dab(Dab),
    Snapshot,
    Undo,
    Clear(Surface),
    Initialize,
    AdvanceInk,
    CheckTubes([u32; 4]),
    Tubes,
    SeedBenchmark,
    Service(Brush, BrushService),
    SaveSwatch(usize, [u32; 2]),
    RemoveSwatch(usize),
}

pub enum BrushService {
    Load(usize),
    Dip(usize),
    Wipe(f32),
}
#[derive(Clone, Copy)]
struct WetInk {
    bounds: [u32; 4],
    remaining: u32,
}

pub struct PaintGpu {
    ink: Option<WetInk>,
    ink_history: [Option<WetInk>; HISTORY],
    flow_ink: wgpu::ComputePipeline,
    canvas: wgpu::Buffer,
    state: wgpu::Buffer,
    history: Vec<wgpu::Buffer>,
    next_snapshot: usize,
    snapshots: usize,
    uniforms: wgpu::Buffer,
    stride: usize,
    bindings: wgpu::BindGroup,
    gather: wgpu::ComputePipeline,
    apply: wgpu::ComputePipeline,
    display: wgpu::ComputePipeline,
    choose_slots: wgpu::ComputePipeline,
    service: wgpu::ComputePipeline,
    preview: wgpu::ComputePipeline,
    output: wgpu::Texture,
}

impl PaintGpu {
    pub fn new(device: &wgpu::Device) -> Self {
        use wgpu::{
            BufferBindingType as BufferType, BufferUsages as BufferUsage,
            TextureUsages as TextureUsage,
        };
        let buffer = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let copies = BufferUsage::COPY_SRC | BufferUsage::COPY_DST;
        let ink_step = buffer(
            "Ink flow scratch",
            u64::from(SIDE) * u64::from(SIDE) * 8,
            BufferUsage::STORAGE,
        );
        let canvas = buffer("Pigment canvas", PAINT_BYTES, copies | BufferUsage::STORAGE);
        let history = (0..HISTORY)
            .map(|_| buffer("Undo snapshot", PAINT_BYTES + BRUSH_BYTES, copies))
            .collect();
        let state = buffer(
            "Brush loads and transfer",
            BRUSH_BYTES,
            copies | BufferUsage::STORAGE,
        );
        let band_data = crate::pigment::spectrum();
        let bands = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Spectral coefficients"),
            contents: bytemuck::cast_slice(&band_data),
            usage: BufferUsage::STORAGE,
        });
        let stride = size_of::<Dab>()
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as usize);
        let uniforms = buffer(
            "Dab commands",
            (stride * (MAX_COMMANDS + 4)) as u64,
            BufferUsage::UNIFORM | BufferUsage::COPY_DST,
        );
        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sRGB canvas"),
            size: wgpu::Extent3d {
                height: ATLAS_HEIGHT,
                ..extent()
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: TextureUsage::STORAGE_BINDING
                | TextureUsage::TEXTURE_BINDING
                | TextureUsage::COPY_SRC,
            view_formats: &[],
        });
        let buffer_layout =
            |binding, ty, has_dynamic_offset, min_binding_size| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty,
                    has_dynamic_offset,
                    min_binding_size,
                },
                count: None,
            };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Paint bindings"),
            entries: &[
                buffer_layout(0, BufferType::Storage { read_only: false }, false, None),
                buffer_layout(1, BufferType::Storage { read_only: false }, false, None),
                buffer_layout(
                    2,
                    BufferType::Uniform,
                    true,
                    NonZeroU64::new(size_of::<Dab>() as u64),
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::StorageTexture {
                        access: wgpu::StorageTextureAccess::WriteOnly,
                        format: wgpu::TextureFormat::Rgba8Unorm,
                        view_dimension: wgpu::TextureViewDimension::D2,
                    },
                    count: None,
                },
                buffer_layout(4, BufferType::Storage { read_only: true }, false, None),
                buffer_layout(5, BufferType::Storage { read_only: false }, false, None),
            ],
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Paint resources"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: canvas.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: state.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniforms,
                        offset: 0,
                        size: NonZeroU64::new(size_of::<Dab>() as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(
                        &output.create_view(&Default::default()),
                    ),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: bands.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: ink_step.as_entire_binding(),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::include_wgsl!("paint.wgsl"));
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Pigment compute layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let pipeline = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            canvas,
            ink: None,
            ink_history: [None; HISTORY],
            flow_ink: pipeline("flow_ink"),
            state,
            history,
            next_snapshot: 0,
            snapshots: 0,
            uniforms,
            stride,
            bindings,
            gather: pipeline("gather"),
            apply: pipeline("apply"),
            display: pipeline("display"),
            choose_slots: pipeline("choose_slots"),
            service: pipeline("service"),
            preview: pipeline("preview"),
            output,
        }
    }

    pub fn view(&self) -> wgpu::TextureView {
        self.output.create_view(&Default::default())
    }

    pub fn can_undo(&self) -> bool {
        self.snapshots > 0
    }

    pub fn ink_is_wet(&self) -> bool {
        self.ink.is_some()
    }

    fn snapshot(&mut self, encoder: &mut wgpu::CommandEncoder) {
        self.ink_history[self.next_snapshot] = self.ink;
        encoder.copy_buffer_to_buffer(
            &self.canvas,
            0,
            &self.history[self.next_snapshot],
            0,
            PAINT_BYTES,
        );
        encoder.copy_buffer_to_buffer(
            &self.state,
            0,
            &self.history[self.next_snapshot],
            PAINT_BYTES,
            BRUSH_BYTES,
        );
        self.next_snapshot = (self.next_snapshot + 1) % HISTORY;
        self.snapshots = (self.snapshots + 1).min(HISTORY);
    }

    pub fn encode(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        commands: &[Command],
    ) {
        assert!(commands.len() <= MAX_COMMANDS);
        let mut upload = Vec::new();
        let mut preview_needed = false;
        let mut placement = false;
        let mut dirty = [None; 3];
        for command in commands {
            let dab = match *command {
                Command::Snapshot => {
                    self.snapshot(encoder);
                    continue;
                }
                Command::Initialize => {
                    self.ink = None;
                    encoder.clear_buffer(&self.canvas, 0, None);
                    encoder.clear_buffer(&self.state, 0, None);
                    preview_needed = true;
                    Dab::operation([0, 0, SIDE, ATLAS_HEIGHT], 1)
                }
                Command::Clear(surface) => {
                    if surface == Surface::Canvas {
                        self.ink = None;
                    }
                    Dab::operation(surface.bounds(), 4)
                }
                Command::AdvanceInk => {
                    let Some(mut ink) = self.ink else { continue };
                    let [_, y, _, height] = ink.bounds;
                    ink.bounds[3] = (height + 1).min(SIDE - y);
                    let dab = Dab::operation(ink.bounds, 10);
                    ink.remaining -= 1;
                    self.ink = (ink.remaining > 0).then_some(ink);
                    dab
                }
                Command::Undo => {
                    if !self.can_undo() {
                        continue;
                    }
                    self.next_snapshot = (self.next_snapshot + HISTORY - 1) % HISTORY;
                    self.snapshots -= 1;
                    self.ink = self.ink_history[self.next_snapshot];
                    let history = &self.history[self.next_snapshot];
                    encoder.copy_buffer_to_buffer(history, 0, &self.canvas, 0, PAINT_BYTES);
                    encoder.copy_buffer_to_buffer(
                        history,
                        PAINT_BYTES,
                        &self.state,
                        0,
                        BRUSH_BYTES,
                    );
                    preview_needed = true;
                    Dab::operation([0, 0, SIDE, ATLAS_HEIGHT], 1)
                }
                Command::Dab(dab) => {
                    if dab.mode[0] == 9 {
                        if dab.brush[3] > 0.56 && dab.bounds[2] > 0 && dab.bounds[3] > 0 {
                            let bounds = self
                                .ink
                                .map_or(dab.bounds, |ink| union(ink.bounds, dab.bounds));
                            self.ink = Some(WetInk {
                                bounds,
                                remaining: 100,
                            });
                        }
                    } else {
                        preview_needed = true;
                    }
                    dab
                }
                Command::CheckTubes(tubes) => {
                    placement = true;
                    let mut dab = Dab::operation([0; 4], 8);
                    dab.tubes = tubes;
                    dab
                }
                Command::Tubes => Dab::operation([0, SIDE, PALETTE_WIDTH, 64], 2),
                Command::SeedBenchmark => Dab::operation(Surface::Canvas.bounds(), 7),
                Command::Service(brush, ref action) => {
                    preview_needed = true;
                    let mut dab = Dab::operation([0; 4], 5);
                    dab.mode[1] = brush.load_start();
                    match *action {
                        BrushService::Load(tube) => {
                            dab.mode[2] = tube as u32;
                            dab.brush[3] = 1150.;
                        }
                        BrushService::Dip(tube) => {
                            dab.mode[2] = tube as u32;
                            dab.brush[3] = 4600.;
                        }
                        BrushService::Wipe(fraction) => {
                            dab.mode[0] = 6;
                            dab.brush[3] = fraction;
                        }
                    }
                    dab
                }
                Command::SaveSwatch(slot, [x, y]) => {
                    let mut dab = Dab::operation(swatch_bounds(slot), 3);
                    dab.mode[2] = x;
                    dab.mode[3] = SIDE + y;
                    dab
                }
                Command::RemoveSwatch(slot) => Dab::operation(swatch_bounds(slot), 4),
            };
            let offset = self.upload(&mut upload, &dab);
            match dab.mode[0] {
                0 => {
                    self.dispatch(
                        encoder,
                        &self.gather,
                        offset,
                        [if dab.mode[1] == 0 { 1 } else { 3 }, 1],
                    );
                }
                8 => self.dispatch(encoder, &self.choose_slots, offset, [1, 1]),
                10 => self.dispatch(
                    encoder,
                    &self.flow_ink,
                    offset,
                    [dab.bounds[2].div_ceil(8), dab.bounds[3].div_ceil(8)],
                ),
                5 | 6 => {
                    self.dispatch(encoder, &self.service, offset, [1, 1]);
                    continue;
                }
                _ => {}
            }
            if dab.bounds[2] != 0 && dab.bounds[3] != 0 {
                if dab.mode[0] != 1 {
                    self.dispatch(
                        encoder,
                        &self.apply,
                        offset,
                        [dab.bounds[2].div_ceil(8), dab.bounds[3].div_ceil(8)],
                    );
                }
                if dab.mode[0] == 1 {
                    dirty = [
                        Some(Surface::Canvas.bounds()),
                        Some(Surface::Palette.bounds()),
                        Some([544, SIDE, crate::workbench::SWATCH_COUNT as u32, 1]),
                    ];
                } else {
                    let region = if dab.bounds[1] < SIDE {
                        0
                    } else if dab.bounds[0] < PALETTE_WIDTH {
                        1
                    } else {
                        2
                    };
                    dirty[region] =
                        Some(dirty[region].map_or(dab.bounds, |bounds| union(bounds, dab.bounds)));
                }
            }
        }
        for bounds in dirty.into_iter().flatten() {
            let offset = self.upload(&mut upload, &Dab::operation(bounds, 1));
            self.dispatch(
                encoder,
                &self.display,
                offset,
                [bounds[2].div_ceil(8), bounds[3].div_ceil(8)],
            );
        }
        if preview_needed || placement {
            let dab = Dab::operation([0; 4], if placement { 2 } else { 1 });
            let offset = self.upload(&mut upload, &dab);
            self.dispatch(encoder, &self.preview, offset, [1, 1]);
        }
        if !upload.is_empty() {
            queue.write_buffer(&self.uniforms, 0, &upload);
        }
    }

    fn upload(&self, upload: &mut Vec<u8>, dab: &Dab) -> u32 {
        let offset = upload.len();
        upload.extend_from_slice(bytemuck::bytes_of(dab));
        upload.resize(offset + self.stride, 0);
        offset as u32
    }

    fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        offset: u32,
        groups: [u32; 2],
    ) {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bindings, &[offset]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
    }

    pub fn placement_readback(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::Buffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Palette placement result"),
            size: 4,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                origin: wgpu::Origin3d {
                    x: 528,
                    y: SIDE,
                    z: 0,
                },
                ..self.output.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: None,
                    rows_per_image: None,
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        buffer
    }
    pub fn readback(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::Buffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("PNG readback"),
            size: PNG_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            self.output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIDE * 4),
                    rows_per_image: Some(SIDE),
                },
            },
            extent(),
        );
        buffer
    }

    #[cfg(test)]
    pub fn inspect(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> (wgpu::Buffer, wgpu::Buffer) {
        let read = |label, size| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            })
        };
        let pigment = read("Test pigment state", PAINT_BYTES);
        let brush = read("Test brush state", BRUSH_BYTES);
        encoder.copy_buffer_to_buffer(&self.canvas, 0, &pigment, 0, PAINT_BYTES);
        encoder.copy_buffer_to_buffer(&self.state, 0, &brush, 0, BRUSH_BYTES);
        (pigment, brush)
    }

    #[cfg(test)]
    pub fn reference_swatches(&self, queue: &wgpu::Queue, colors: &[[f32; 8]]) {
        queue.write_buffer(&self.canvas, 0, bytemuck::cast_slice(colors));
    }

    #[cfg(test)]
    pub fn refresh(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder) {
        let dab = Dab::operation(Surface::Canvas.bounds(), 1);
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&dab));
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.display);
        pass.set_bind_group(0, &self.bindings, &[0]);
        pass.dispatch_workgroups(SIDE / 8, SIDE / 8, 1);
    }
}

fn union(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let x = a[0].min(b[0]);
    let y = a[1].min(b[1]);
    [
        x,
        y,
        (a[0] + a[2]).max(b[0] + b[2]) - x,
        (a[1] + a[3]).max(b[1] + b[3]) - y,
    ]
}

pub fn swatch_bounds(slot: usize) -> [u32; 4] {
    [544 + slot as u32, SIDE, 1, 1]
}

pub fn atlas_uv([x, y, width, height]: [u32; 4]) -> egui::Rect {
    egui::Rect::from_min_max(
        egui::pos2(
            (x as f32 + 0.5) / SIDE as f32,
            (y as f32 + 0.5) / ATLAS_HEIGHT as f32,
        ),
        egui::pos2(
            (x as f32 + width as f32 - 0.5) / SIDE as f32,
            (y as f32 + height as f32 - 0.5) / ATLAS_HEIGHT as f32,
        ),
    )
}

pub fn placement_result(buffer: wgpu::Buffer) -> Receiver<Option<bool>> {
    let (sender, result) = mpsc::channel();
    let mapped = buffer.clone();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |outcome| {
            let placed = outcome
                .ok()
                .map(|()| mapped.slice(..).get_mapped_range()[0] != 0);
            if placed.is_some() {
                mapped.unmap();
            }
            let _ = sender.send(placed);
        });
    result
}

fn extent() -> wgpu::Extent3d {
    wgpu::Extent3d {
        width: SIDE,
        height: SIDE,
        depth_or_array_layers: 1,
    }
}

pub fn export(buffer: wgpu::Buffer, path: PathBuf) -> Receiver<Result<PathBuf, String>> {
    let (done, result) = mpsc::channel();
    let mapped_buffer = buffer.clone();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |mapped| {
            if let Err(error) = mapped {
                let _ = done.send(Err(format!("Could not read the canvas: {error}")));
                return;
            }
            std::thread::spawn(move || {
                let pixels = mapped_buffer.slice(..).get_mapped_range();
                let result = write_png(&path, &pixels).map(|()| path);
                drop(pixels);
                mapped_buffer.unmap();
                let _ = done.send(result);
            });
        });
    result
}

fn write_png(path: &std::path::Path, pixels: &[u8]) -> Result<(), String> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    let mut png = png::Encoder::new(file.as_file_mut(), SIDE, SIDE);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    png.set_source_srgb(png::SrgbRenderingIntent::RelativeColorimetric);
    let mut writer = png.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(pixels).map_err(|e| e.to_string())?;
    writer.finish().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub async fn device(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<(wgpu::Device, wgpu::Queue, wgpu::Adapter), String> {
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: surface,
            force_fallback_adapter: false,
        })
        .await
        .ok_or("No Direct3D 12 adapter found. Check your GPU driver.")?;
    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: Some("Pigment GPU"),
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(|e| format!("Could not start Direct3D 12. Check your GPU driver. {e}"))?;
    Ok((device, queue, adapter))
}
