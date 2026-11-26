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
pub const BRUSH_COUNT: usize = 6;
pub const BRUSH_LANES: usize = 14;
pub const BRUSH_LOAD_BYTES: usize = BRUSH_LANES * 32;
pub const BRUSH_BYTES: u64 = BRUSH_LOAD_BYTES as u64 + 144;
pub const LEGACY_BRUSH_BYTES: usize = 272;
const INK_BYTES: u64 = SIDE as u64 * SIDE as u64 * 8;
const DRY_BYTES: u64 = SIDE as u64 * SIDE as u64 * 32;
const SUBSTRATE_BYTES: u64 = SIDE as u64 * SIDE as u64 * 64;
const REF_BYTES: u64 = SIDE as u64 * SIDE as u64 * 4;
const SKETCH_BYTES: u64 = SIDE as u64 * SIDE as u64 * 4;
const PNG_BYTES: u64 = SIDE as u64 * SIDE as u64 * 4;
pub const PROJECT_BYTES: u64 =
    PAINT_BYTES + DRY_BYTES + SUBSTRATE_BYTES + INK_BYTES + SKETCH_BYTES + REF_BYTES + BRUSH_BYTES;

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Brush {
    Round,
    Flat,
    Filbert,
    Fan,
    PaletteKnife,
    SoftMop,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushLane {
    Left,
    Middle,
    Right,
}

impl BrushLane {
    fn selector(self) -> u32 {
        match self {
            Self::Left => 1,
            Self::Middle => 2,
            Self::Right => 3,
        }
    }
}

impl Brush {
    pub const ALL: [Self; BRUSH_COUNT] = [
        Self::Round,
        Self::Flat,
        Self::Filbert,
        Self::Fan,
        Self::PaletteKnife,
        Self::SoftMop,
    ];

    pub fn is_directional(self) -> bool {
        matches!(
            self,
            Self::Flat | Self::Filbert | Self::Fan | Self::PaletteKnife
        )
    }

    pub fn size_index(self) -> usize {
        usize::from(self.is_directional())
    }

    pub fn index(self) -> usize {
        self.profile_id() as usize
    }

    pub fn legacy_reservoir(self) -> Self {
        if self.is_directional() {
            Self::Flat
        } else {
            Self::Round
        }
    }

    pub fn load_start(self) -> u32 {
        match self {
            Self::Round => 0,
            Self::Flat => 1,
            Self::Filbert => 4,
            Self::Fan => 7,
            Self::PaletteKnife => 10,
            Self::SoftMop => 13,
        }
    }

    pub fn load_lanes(self) -> std::ops::Range<u32> {
        let start = self.load_start();
        start..start + if self.is_directional() { 3 } else { 1 }
    }

    pub fn profile_id(self) -> u32 {
        match self {
            Self::Round => 0,
            Self::Flat => 1,
            Self::Filbert => 2,
            Self::Fan => 3,
            Self::PaletteKnife => 4,
            Self::SoftMop => 5,
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
    sweep: [f32; 4],
}

#[derive(Clone, Copy)]
pub struct BrushContact {
    pub point: egui::Pos2,
    pub radius: f32,
    pub pressure: f32,
    pub angle: f32,
}

impl Dab {
    pub fn use_legacy_reservoir(&mut self, brush: Brush) {
        if self.mode[0] == 0 {
            self.mode[1] = brush.legacy_reservoir().load_start();
        }
    }

    #[cfg(test)]
    pub fn new(
        position: egui::Pos2,
        radius: f32,
        pressure: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
    ) -> Self {
        Self::new_tiled(position, radius, pressure, surface, brush, angle, false)
    }

    pub fn new_tiled(
        position: egui::Pos2,
        radius: f32,
        pressure: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
        tiled: bool,
    ) -> Self {
        let wrap = tiled && surface == Surface::Canvas;
        let (bounds, center) =
            Self::footprint_bounds(position, egui::Vec2::splat(radius * 1.07), surface, wrap);
        let (sin, cos) = angle.sin_cos();
        Self {
            bounds,
            brush: [center.x, center.y, radius, pressure],
            axis: [cos, sin, 1.0, 0.0],
            mode: [0, brush.load_start(), brush.profile_id(), u32::from(wrap)],
            tubes: [0; 4],
            sweep: [0.0; 4],
        }
    }

    /// The start and end axes share `axis`. The sweep stores displacement,
    /// end radius and end pressure. A zero end radius denotes a stationary dab.
    pub fn swept_contact(
        start: BrushContact,
        end: BrushContact,
        brush: Brush,
        surface: Surface,
        tiled: bool,
    ) -> Self {
        let wrap = tiled && surface == Surface::Canvas;
        let delta = end.point - start.point;
        let radius = start.radius.max(end.radius);
        let (start_sin, start_cos) = start.angle.sin_cos();
        let (end_sin, end_cos) = end.angle.sin_cos();
        let x = start_cos.abs().max(end_cos.abs());
        let y = start_sin.abs().max(end_sin.abs());
        // Bound the rectangular blade between both endpoints. Sine and cosine
        // change by at most the angle to the nearer endpoint.
        let depth = match brush {
            Brush::Flat => 0.36,
            Brush::Filbert => 0.52,
            Brush::Fan => 0.51,
            _ => 0.22,
        };
        let turn_margin = (1.0 + depth) * 0.5 * (end.angle - start.angle).abs();
        let footprint = egui::vec2(x + depth * y, y + depth * x);
        let pad = (footprint + egui::Vec2::splat(turn_margin)) * radius + 0.5 * delta.abs();
        let (bounds, center) =
            Self::footprint_bounds(start.point + delta * 0.5, pad, surface, wrap);
        Self {
            bounds,
            brush: [center.x, center.y, start.radius, start.pressure],
            axis: [start_cos, start_sin, end_cos, end_sin],
            mode: [0, brush.load_start(), brush.profile_id(), u32::from(wrap)],
            tubes: [0; 4],
            sweep: [delta.x, delta.y, end.radius, end.pressure],
        }
    }

    fn footprint_bounds(
        position: egui::Pos2,
        pad: egui::Vec2,
        surface: Surface,
        wrap: bool,
    ) -> ([u32; 4], egui::Pos2) {
        let [origin_x, origin_y, w, h] = surface.bounds();
        let (pos_x, pos_y, x, width, y, height) = if wrap {
            let px = position.x.rem_euclid(w as f32);
            let py = position.y.rem_euclid(h as f32);
            let x0 = (px - pad.x).floor() as i32;
            let x1 = (px + pad.x).ceil() as i32;
            let y0 = (py - pad.y).floor() as i32;
            let y1 = (py + pad.y).ceil() as i32;
            (
                px,
                py,
                x0.rem_euclid(w as i32) as u32,
                ((x1 - x0) as u32).clamp(1, w),
                y0.rem_euclid(h as i32) as u32,
                ((y1 - y0) as u32).clamp(1, h),
            )
        } else {
            let extent = |coordinate: f32, pad: f32, limit: u32| {
                let start = (coordinate - pad).floor().clamp(0., limit as f32) as u32;
                let end = (coordinate + pad).ceil().clamp(0., limit as f32) as u32;
                (start, end - start)
            };
            let (x, width) = extent(position.x, pad.x, w);
            let (y, height) = extent(position.y, pad.y, h);
            (position.x, position.y, x, width, y, height)
        };
        (
            [x + origin_x, y + origin_y, width, height],
            egui::pos2(pos_x + origin_x as f32, pos_y + origin_y as f32),
        )
    }

    #[cfg(test)]
    pub fn ink(position: egui::Pos2, pressure: f32, dwell: bool) -> Self {
        Self::ink_tiled(position, pressure, dwell, false)
    }

    pub fn ink_tiled(position: egui::Pos2, pressure: f32, dwell: bool, tiled: bool) -> Self {
        let radius = Self::ink_radius(pressure);
        let mut dab = Self::new_tiled(
            position,
            radius,
            pressure,
            Surface::Canvas,
            Brush::Round,
            0.,
            tiled,
        );
        dab.mode[0] = 9;
        dab.mode[2] = u32::from(dwell);
        dab
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_load(
        position: egui::Pos2,
        radius: f32,
        pressure: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
        tiled: bool,
        load_ratio: f32,
    ) -> Self {
        let mut dab = Self::new_tiled(position, radius, pressure, surface, brush, angle, tiled);
        dab.axis[2] = load_ratio.clamp(0.0, 1.0);
        dab
    }

    pub fn set_refined_tip(&mut self, weight: f32) {
        self.axis[3] = weight;
    }

    pub fn charcoal(
        position: egui::Pos2,
        radius: f32,
        pressure: f32,
        erase: bool,
        tiled: bool,
    ) -> Self {
        let mut dab = Self::new_tiled(
            position,
            radius,
            pressure,
            Surface::Canvas,
            Brush::Round,
            0.,
            tiled,
        );
        dab.mode[0] = 15;
        dab.mode[2] = 0;
        dab.axis[0] = if erase { -1.0 } else { 1.0 };
        dab
    }

    pub fn ink_radius(pressure: f32) -> f32 {
        1.3 + 2.8 * pressure
    }

    pub fn operation(bounds: [u32; 4], operation: u32) -> Self {
        Self {
            bounds,
            brush: [0.; 4],
            axis: [0.; 4],
            mode: [operation, 0, 0, 0],
            tubes: [0; 4],
            sweep: [0.0; 4],
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
    SaveSwatch(usize, Surface, [u32; 2]),
    RemoveSwatch(usize),
    EquipRack(usize, [f32; 8]),
    DryCanvas,
    ImportUnderlayer(std::sync::Arc<Vec<u32>>),
    ImportReference(std::sync::Arc<Vec<u32>>),
    ClearSketch,
    ClearReference,
    LoadProject(std::sync::Arc<Vec<u8>>),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ActiveLayer {
    Paint,
    Sketch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layers {
    pub active: ActiveLayer,
    pub paint_visible: bool,
    pub sketch_visible: bool,
    pub sketch_opacity: f32,
    pub reference_visible: bool,
    pub reference_opacity: f32,
    pub has_reference: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            active: ActiveLayer::Paint,
            paint_visible: true,
            sketch_visible: true,
            sketch_opacity: 1.0,
            reference_visible: true,
            reference_opacity: 0.65,
            has_reference: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExportKind {
    Color,
    Transparent,
    Height,
    Normal,
}

impl ExportKind {
    pub fn is_srgb(self) -> bool {
        matches!(self, Self::Color | Self::Transparent)
    }

    pub fn shader_mode(self) -> u32 {
        match self {
            Self::Color => 0,
            Self::Transparent => 1,
            Self::Height => 2,
            Self::Normal => 3,
        }
    }
}

#[derive(Clone, Copy)]
pub enum BrushService {
    Load(usize),
    Dip(usize),
    DipCustom(usize),
    DipRack(usize),
    DipCustomLane(usize, BrushLane),
    DipRackLane(usize, BrushLane),
    Wipe(f32),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lighting {
    pub enabled: bool,
    pub impasto_depth: f32,
    pub canvas_weave: f32,
    pub specular_sheen: f32,
    pub light_angle_deg: f32,
    pub light_elevation_deg: f32,
}

impl Default for Lighting {
    fn default() -> Self {
        Self {
            enabled: true,
            impasto_depth: 1.0,
            canvas_weave: 0.35,
            specular_sheen: 1.0,
            light_angle_deg: 315.0,
            light_elevation_deg: 45.0,
        }
    }
}

impl Lighting {
    pub fn light_direction(self) -> [f32; 3] {
        let azimuth_rad = self.light_angle_deg.to_radians();
        let elevation_rad = self.light_elevation_deg.to_radians();
        let cos_el = elevation_rad.cos();
        let sin_el = elevation_rad.sin();
        // Clockwise compass angle in screen coordinates (+x right, +y down):
        // 0° = North/Top (0, -1)
        // 90° = East/Right (1, 0)
        // 180° = South/Bottom (0, 1)
        // 270° = West/Left (-1, 0)
        // 315° = Northwest/Top-Left (-0.7071, -0.7071)
        let lx = azimuth_rad.sin() * cos_el;
        let ly = -azimuth_rad.cos() * cos_el;
        let lz = sin_el;
        let len = (lx * lx + ly * ly + lz * lz).sqrt().max(0.0001);
        [lx / len, ly / len, lz / len]
    }
}

#[derive(Clone, Copy)]
struct WetInk {
    bounds: [u32; 4],
    remaining: u32,
}

pub struct PaintGpu {
    ink: Option<WetInk>,
    ink_history: [Option<WetInk>; HISTORY],
    ink_canvas: wgpu::Buffer,
    flow_ink: wgpu::ComputePipeline,
    canvas: wgpu::Buffer,
    dry_canvas: wgpu::Buffer,
    pub dry_substrate: wgpu::Buffer,
    reference_layer: wgpu::Buffer,
    sketch_canvas: wgpu::Buffer,
    state: wgpu::Buffer,
    device: wgpu::Device,
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
    display_atlas: Option<wgpu::Texture>,
    pub lighting: Lighting,
    pub layers: Layers,
    pub tile: bool,
    pub smooth_height: bool,
    pub gl_normals: bool,
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
        let ink_canvas = buffer("Ink canvas", INK_BYTES, copies | BufferUsage::STORAGE);
        let canvas = buffer("Pigment canvas", PAINT_BYTES, copies | BufferUsage::STORAGE);
        let dry_canvas = buffer("Dry canvas", DRY_BYTES, copies | BufferUsage::STORAGE);
        let dry_substrate = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Dry substrate"),
            contents: &vec![0xFFu8; SUBSTRATE_BYTES as usize],
            usage: copies | BufferUsage::STORAGE,
        });
        let reference_layer = buffer("Reference layer", REF_BYTES, copies | BufferUsage::STORAGE);
        let sketch_canvas = buffer("Sketch canvas", SKETCH_BYTES, copies | BufferUsage::STORAGE);
        let history = Vec::with_capacity(HISTORY);
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
            usage: TextureUsage::STORAGE_BINDING | TextureUsage::COPY_SRC,
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
                buffer_layout(6, BufferType::Storage { read_only: false }, false, None),
                buffer_layout(7, BufferType::Storage { read_only: false }, false, None),
                buffer_layout(8, BufferType::Storage { read_only: false }, false, None),
                buffer_layout(9, BufferType::Storage { read_only: false }, false, None),
                buffer_layout(10, BufferType::Storage { read_only: false }, false, None),
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
                wgpu::BindGroupEntry {
                    binding: 6,
                    resource: ink_canvas.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 7,
                    resource: dry_canvas.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 8,
                    resource: dry_substrate.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 9,
                    resource: reference_layer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 10,
                    resource: sketch_canvas.as_entire_binding(),
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
            dry_canvas,
            dry_substrate,
            reference_layer,
            sketch_canvas,
            ink: None,
            ink_history: [None; HISTORY],
            ink_canvas,
            flow_ink: pipeline("flow_ink"),
            state,
            device: device.clone(),
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
            display_atlas: None,
            lighting: Lighting {
                enabled: false,
                ..Default::default()
            },
            layers: Layers::default(),
            tile: false,
            smooth_height: false,
            gl_normals: false,
        }
    }

    pub fn display_view(&mut self) -> wgpu::TextureView {
        // An sRGB sampling texture decodes the stored bytes for egui. Some DX12
        // drivers reject reinterpreting the UAV as sRGB, so keep the UAV and SRV separate.
        self.display_atlas
            .get_or_insert_with(|| {
                self.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Paint atlas display"),
                    size: self.output.size(),
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                })
            })
            .create_view(&Default::default())
    }

    fn update_display_atlas(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        updates: impl IntoIterator<Item = [u32; 4]>,
    ) {
        let Some(display_atlas) = &self.display_atlas else {
            return;
        };
        for [x, y, width, height] in updates {
            let origin = wgpu::Origin3d { x, y, z: 0 };
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    origin,
                    ..self.output.as_image_copy()
                },
                wgpu::TexelCopyTextureInfo {
                    origin,
                    ..display_atlas.as_image_copy()
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    pub fn can_undo(&self) -> bool {
        self.snapshots > 0
    }

    pub fn ink_is_wet(&self) -> bool {
        self.ink.is_some()
    }

    fn snapshot(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.history.len() <= self.next_snapshot {
            self.history
                .push(self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("Undo snapshot"),
                    size: PAINT_BYTES
                        + BRUSH_BYTES
                        + INK_BYTES
                        + DRY_BYTES
                        + SUBSTRATE_BYTES
                        + SKETCH_BYTES,
                    usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }));
        }
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
        encoder.copy_buffer_to_buffer(
            &self.ink_canvas,
            0,
            &self.history[self.next_snapshot],
            PAINT_BYTES + BRUSH_BYTES,
            INK_BYTES,
        );
        encoder.copy_buffer_to_buffer(
            &self.dry_canvas,
            0,
            &self.history[self.next_snapshot],
            PAINT_BYTES + BRUSH_BYTES + INK_BYTES,
            DRY_BYTES,
        );
        encoder.copy_buffer_to_buffer(
            &self.dry_substrate,
            0,
            &self.history[self.next_snapshot],
            PAINT_BYTES + BRUSH_BYTES + INK_BYTES + DRY_BYTES,
            SUBSTRATE_BYTES,
        );
        encoder.copy_buffer_to_buffer(
            &self.sketch_canvas,
            0,
            &self.history[self.next_snapshot],
            PAINT_BYTES + BRUSH_BYTES + INK_BYTES + DRY_BYTES + SUBSTRATE_BYTES,
            SKETCH_BYTES,
        );
        self.next_snapshot = (self.next_snapshot + 1) % HISTORY;
        self.snapshots = (self.snapshots + 1).min(HISTORY);
    }

    pub fn encode(
        &mut self,
        device: &wgpu::Device,
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
                    encoder.clear_buffer(&self.dry_canvas, 0, None);
                    encoder.clear_buffer(&self.state, 0, None);
                    encoder.clear_buffer(&self.ink_canvas, 0, None);
                    encoder.clear_buffer(&self.sketch_canvas, 0, None);
                    preview_needed = true;
                    Dab::operation([0, 0, SIDE, ATLAS_HEIGHT], 1)
                }
                Command::Clear(surface) => {
                    if surface == Surface::Canvas {
                        self.ink = None;
                        encoder.clear_buffer(&self.ink_canvas, 0, None);
                    }
                    Dab::operation(surface.bounds(), 4)
                }
                Command::DryCanvas => {
                    preview_needed = true;
                    Dab::operation(Surface::Canvas.bounds(), 14)
                }
                Command::ImportReference(ref words) => {
                    let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Reference layer staging"),
                        contents: bytemuck::cast_slice(words),
                        usage: wgpu::BufferUsages::COPY_SRC,
                    });
                    encoder.copy_buffer_to_buffer(&staging, 0, &self.reference_layer, 0, REF_BYTES);
                    self.layers.has_reference = true;
                    dirty[0] = Some(Surface::Canvas.bounds());
                    continue;
                }
                Command::ClearSketch => {
                    encoder.clear_buffer(&self.sketch_canvas, 0, None);
                    dirty[0] = Some(Surface::Canvas.bounds());
                    continue;
                }
                Command::ClearReference => {
                    encoder.clear_buffer(&self.reference_layer, 0, None);
                    self.layers.has_reference = false;
                    dirty[0] = Some(Surface::Canvas.bounds());
                    continue;
                }
                Command::LoadProject(ref bytes) => {
                    self.ink = None;
                    let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Project load staging"),
                        contents: bytes,
                        usage: wgpu::BufferUsages::COPY_SRC,
                    });
                    let mut off = 0u64;
                    let mut copy = |dst: &wgpu::Buffer, size: u64| {
                        encoder.copy_buffer_to_buffer(&staging, off, dst, 0, size);
                        off += size;
                    };
                    copy(&self.canvas, PAINT_BYTES);
                    copy(&self.dry_canvas, DRY_BYTES);
                    copy(&self.dry_substrate, SUBSTRATE_BYTES);
                    copy(&self.ink_canvas, INK_BYTES);
                    copy(&self.sketch_canvas, SKETCH_BYTES);
                    copy(&self.reference_layer, REF_BYTES);
                    copy(&self.state, BRUSH_BYTES);
                    preview_needed = true;
                    dirty = [
                        Some(Surface::Canvas.bounds()),
                        Some(Surface::Palette.bounds()),
                        Some([544, SIDE, crate::workbench::CUSTOM_JAR_COUNT as u32 + 8, 1]),
                    ];
                    continue;
                }
                Command::ImportUnderlayer(ref words) => {
                    self.ink = None;
                    encoder.clear_buffer(
                        &self.canvas,
                        0,
                        Some(u64::from(SIDE) * u64::from(SIDE) * 32),
                    );
                    encoder.clear_buffer(&self.dry_canvas, 0, None);
                    encoder.clear_buffer(&self.ink_canvas, 0, None);
                    let staging = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("Underlayer staging"),
                        contents: bytemuck::cast_slice(words),
                        usage: wgpu::BufferUsages::COPY_SRC,
                    });
                    encoder.copy_buffer_to_buffer(
                        &staging,
                        0,
                        &self.dry_substrate,
                        0,
                        SUBSTRATE_BYTES,
                    );
                    dirty[0] = Some(Surface::Canvas.bounds());
                    continue;
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
                    encoder.copy_buffer_to_buffer(
                        history,
                        PAINT_BYTES + BRUSH_BYTES,
                        &self.ink_canvas,
                        0,
                        INK_BYTES,
                    );
                    encoder.copy_buffer_to_buffer(
                        history,
                        PAINT_BYTES + BRUSH_BYTES + INK_BYTES,
                        &self.dry_canvas,
                        0,
                        DRY_BYTES,
                    );
                    encoder.copy_buffer_to_buffer(
                        history,
                        PAINT_BYTES + BRUSH_BYTES + INK_BYTES + DRY_BYTES,
                        &self.dry_substrate,
                        0,
                        SUBSTRATE_BYTES,
                    );
                    encoder.copy_buffer_to_buffer(
                        history,
                        PAINT_BYTES + BRUSH_BYTES + INK_BYTES + DRY_BYTES + SUBSTRATE_BYTES,
                        &self.sketch_canvas,
                        0,
                        SKETCH_BYTES,
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
                            dab.brush[3] = 46000.;
                        }
                        BrushService::DipCustom(slot) => {
                            dab.mode[0] = 11;
                            dab.mode[2] = slot as u32;
                            dab.brush[3] = 46000.;
                        }
                        BrushService::DipRack(slot) => {
                            dab.mode[0] = 12;
                            dab.mode[2] = slot as u32;
                            dab.brush[3] = 46000.;
                        }
                        BrushService::DipRackLane(slot, lane)
                        | BrushService::DipCustomLane(slot, lane) => {
                            dab.mode[0] = if matches!(action, BrushService::DipRackLane(..)) {
                                12
                            } else {
                                11
                            };
                            dab.mode[2] = slot as u32;
                            dab.mode[3] = lane.selector();
                            dab.brush[3] = 46000.;
                        }
                        BrushService::Wipe(fraction) => {
                            dab.mode[0] = 6;
                            dab.brush[3] = fraction;
                        }
                    }
                    dab
                }
                Command::SaveSwatch(slot, surface, [x, y]) => {
                    let mut dab = Dab::operation(swatch_bounds(slot), 3);
                    dab.mode[2] = x.min(SIDE - 1);
                    dab.mode[3] = match surface {
                        Surface::Canvas => y.min(SIDE - 1),
                        Surface::Palette => SIDE + y.min(255),
                    };
                    dab
                }
                Command::RemoveSwatch(slot) => Dab::operation(swatch_bounds(slot), 4),
                Command::EquipRack(slot, recipe) => {
                    let mut dab = Dab::operation(rack_bounds(slot), 13);
                    dab.brush = [
                        recipe[0] * 45.0,
                        recipe[1] * 45.0,
                        recipe[2] * 45.0,
                        recipe[3] * 45.0,
                    ];
                    dab.axis = [
                        recipe[4] * 45.0,
                        recipe[5] * 45.0,
                        recipe[6] * 45.0,
                        recipe[7] * 45.0,
                    ];
                    dab
                }
            };
            let offset = self.upload(&mut upload, &dab);
            match dab.mode[0] {
                0 => {
                    self.dispatch(
                        encoder,
                        &self.gather,
                        offset,
                        [if matches!(dab.mode[2], 0 | 5) { 1 } else { 3 }, 1],
                    );
                }
                8 => self.dispatch(encoder, &self.choose_slots, offset, [1, 1]),
                10 => self.dispatch(
                    encoder,
                    &self.flow_ink,
                    offset,
                    [dab.bounds[2].div_ceil(8), dab.bounds[3].div_ceil(8)],
                ),
                5 | 6 | 11 | 12 => {
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
                        Some([544, SIDE, crate::workbench::CUSTOM_JAR_COUNT as u32 + 8, 1]),
                    ];
                } else {
                    let region = if dab.bounds[1] < SIDE {
                        0
                    } else if dab.bounds[0] < PALETTE_WIDTH {
                        1
                    } else {
                        2
                    };
                    let bounds = if region == 0
                        && dab.mode[3] != 0
                        && (dab.bounds[0] + dab.bounds[2] > SIDE
                            || dab.bounds[1] + dab.bounds[3] > SIDE)
                    {
                        Surface::Canvas.bounds()
                    } else {
                        dab.bounds
                    };
                    dirty[region] =
                        Some(dirty[region].map_or(bounds, |existing| union(existing, bounds)));
                }
            }
        }
        for bounds in dirty.iter_mut().flatten() {
            let is_canvas = bounds[1] < SIDE;
            let display_bounds = if is_canvas {
                let x0 = bounds[0].saturating_sub(1);
                let y0 = bounds[1].saturating_sub(1);
                let x1 = (bounds[0] + bounds[2] + 1).min(SIDE);
                let y1 = (bounds[1] + bounds[3] + 1).min(SIDE);
                [x0, y0, x1 - x0, y1 - y0]
            } else {
                *bounds
            };
            *bounds = display_bounds;
            let mut dab = Dab::operation(display_bounds, 1);
            self.configure_display_dab(&mut dab, is_canvas, 0);
            let offset = self.upload(&mut upload, &dab);
            self.dispatch(
                encoder,
                &self.display,
                offset,
                [display_bounds[2].div_ceil(8), display_bounds[3].div_ceil(8)],
            );
        }
        if preview_needed || placement {
            let dab = Dab::operation([0; 4], if placement { 2 } else { 1 });
            let offset = self.upload(&mut upload, &dab);
            self.dispatch(encoder, &self.preview, offset, [1, 1]);
        }
        self.update_display_atlas(
            encoder,
            dirty
                .into_iter()
                .flatten()
                .chain((preview_needed || placement).then_some([512, SIDE, BRUSH_LANES as u32, 1])),
        );
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

    fn configure_display_dab(&self, dab: &mut Dab, is_canvas: bool, export_mode: u32) {
        if !is_canvas {
            return;
        }
        let light_dir = self.lighting.light_direction();
        dab.mode[1] = u32::from(self.lighting.enabled);
        dab.mode[2] = export_mode;
        dab.mode[3] = u32::from(self.tile)
            | (u32::from(!self.layers.paint_visible) << 1)
            | (u32::from(export_mode != 0 && self.smooth_height) << 2)
            | (u32::from(export_mode == 3 && self.gl_normals) << 3);
        let sketch_alpha = if self.layers.sketch_visible {
            self.layers.sketch_opacity
        } else {
            0.0
        };
        let ref_alpha = if self.layers.reference_visible && self.layers.has_reference {
            self.layers.reference_opacity
        } else {
            0.0
        };
        dab.brush = [
            self.lighting.impasto_depth,
            self.lighting.canvas_weave,
            self.lighting.specular_sheen,
            sketch_alpha,
        ];
        dab.axis = [light_dir[0], light_dir[1], light_dir[2], ref_alpha];
    }

    pub fn readback_kind(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        kind: ExportKind,
    ) -> wgpu::Buffer {
        if kind == ExportKind::Color {
            return self.readback(device, encoder);
        }
        let make_dab = |export_mode| {
            let mut dab = Dab::operation(Surface::Canvas.bounds(), 1);
            self.configure_display_dab(&mut dab, true, export_mode);
            dab
        };
        let export_dab = make_dab(kind.shader_mode());
        let restore_dab = make_dab(0);
        let offset0 = (self.stride * MAX_COMMANDS) as u64;
        let offset1 = (self.stride * (MAX_COMMANDS + 1)) as u64;
        queue.write_buffer(&self.uniforms, offset0, bytemuck::bytes_of(&export_dab));
        queue.write_buffer(&self.uniforms, offset1, bytemuck::bytes_of(&restore_dab));

        let groups = [SIDE / 8, SIDE / 8];
        self.dispatch(encoder, &self.display, offset0 as u32, groups);
        let buffer = self.readback(device, encoder);
        self.dispatch(encoder, &self.display, offset1 as u32, groups);
        buffer
    }

    pub fn readback_custom_jars(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::Buffer {
        let size = (crate::workbench::CUSTOM_JAR_COUNT * 32) as u64;
        let offset = ((SIDE * SIDE + 544) as u64) * 32;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Custom jars readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&self.canvas, offset, &buffer, 0, size);
        buffer
    }

    pub fn readback_project(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::Buffer {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Project readback"),
            size: PROJECT_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut off = 0u64;
        let mut copy = |src: &wgpu::Buffer, size: u64| {
            encoder.copy_buffer_to_buffer(src, 0, &buffer, off, size);
            off += size;
        };
        copy(&self.canvas, PAINT_BYTES);
        copy(&self.dry_canvas, DRY_BYTES);
        copy(&self.dry_substrate, SUBSTRATE_BYTES);
        copy(&self.ink_canvas, INK_BYTES);
        copy(&self.sketch_canvas, SKETCH_BYTES);
        copy(&self.reference_layer, REF_BYTES);
        copy(&self.state, BRUSH_BYTES);
        buffer
    }

    pub fn readback_project_bytes(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Result<Vec<u8>, String> {
        let mut encoder = device.create_command_encoder(&Default::default());
        let buffer = self.readback_project(device, &mut encoder);
        queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Could not read project checkpoint: {e}"))?;
        let out = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        Ok(out)
    }

    pub fn readback_rgba_bytes(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        kind: ExportKind,
    ) -> Result<Vec<u8>, String> {
        let mut encoder = device.create_command_encoder(&Default::default());
        let buffer = self.readback_kind(device, queue, &mut encoder, kind);
        queue.submit([encoder.finish()]);
        let (tx, rx) = mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        let _ = device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| format!("Could not read image buffer: {e}"))?;
        let data = buffer.slice(..).get_mapped_range();
        let row_bytes = SIDE as usize * 4;
        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let padded = row_bytes.div_ceil(align) * align;
        let mut out = vec![0u8; row_bytes * SIDE as usize];
        for y in 0..SIDE as usize {
            out[y * row_bytes..(y + 1) * row_bytes]
                .copy_from_slice(&data[y * padded..y * padded + row_bytes]);
        }
        drop(data);
        buffer.unmap();
        Ok(out)
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
    pub fn inspect_dry(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
    ) -> wgpu::Buffer {
        let dry = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Test dry state"),
            size: DRY_BYTES,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&self.dry_canvas, 0, &dry, 0, DRY_BYTES);
        dry
    }

    #[cfg(test)]
    pub fn reference_swatches(&self, queue: &wgpu::Queue, colors: &[[f32; 8]]) {
        queue.write_buffer(&self.canvas, 0, bytemuck::cast_slice(colors));
    }

    #[cfg(test)]
    pub fn refresh(&self, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder) {
        let bounds = Surface::Canvas.bounds();
        let mut dab = Dab::operation(bounds, 1);
        self.configure_display_dab(&mut dab, true, 0);
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&dab));
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.display);
        pass.set_bind_group(0, &self.bindings, &[0]);
        pass.dispatch_workgroups(SIDE / 8, SIDE / 8, 1);
        drop(pass);
        self.update_display_atlas(encoder, [bounds]);
    }

    pub fn sample_diagnostics(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        x: u32,
        y: u32,
    ) -> crate::notes::GpuDiagnostics {
        let x = x.min(SIDE - 1);
        let y = y.min(SIDE - 1);
        let offset = (y * SIDE + x) as u64 * 32;
        let sub_offset = (y * SIDE + x) as u64 * 64;
        let ink_offset = (y * SIDE + x) as u64 * 8;

        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Diagnostic sample staging"),
            size: 1024,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Diagnostic sample encoder"),
        });
        encoder.copy_buffer_to_buffer(&self.canvas, offset, &staging, 0, 32);
        encoder.copy_buffer_to_buffer(&self.dry_canvas, offset, &staging, 32, 32);
        encoder.copy_buffer_to_buffer(&self.ink_canvas, ink_offset, &staging, 64, 8);
        const LOAD_END: usize = 72 + BRUSH_LOAD_BYTES;
        const SUBSTRATE_END: usize = LOAD_END + 64;
        const PIXEL_START: usize = SUBSTRATE_END.next_multiple_of(256);
        encoder.copy_buffer_to_buffer(&self.state, 0, &staging, 72, BRUSH_LOAD_BYTES as u64);
        encoder.copy_buffer_to_buffer(
            &self.dry_substrate,
            sub_offset,
            &staging,
            LOAD_END as u64,
            64,
        );
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.output,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &staging,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: PIXEL_START as u64,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));

        let (done, rx) = std::sync::mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |res| {
                let _ = done.send(res);
            });
        let _ = device.poll(wgpu::Maintain::Wait);
        let _ = rx.recv();

        let slice = staging.slice(..).get_mapped_range();
        let wet_channels: [f32; 8] = *bytemuck::from_bytes(&slice[0..32]);
        let dry_channels: [f32; 8] = *bytemuck::from_bytes(&slice[32..64]);
        let ink: [f32; 2] = *bytemuck::from_bytes(&slice[64..72]);
        let brush_loads = bytemuck::cast_slice::<u8, [f32; 8]>(&slice[72..LOAD_END]).to_vec();
        let substrate_words: [u32; 16] = *bytemuck::from_bytes(&slice[LOAD_END..SUBSTRATE_END]);
        let display_srgb = slice[PIXEL_START..PIXEL_START + 3].try_into().unwrap();
        drop(slice);
        staging.unmap();

        let mut substrate_bands = [1.0f32; 31];
        for w in 0..16 {
            let lo = (substrate_words[w] & 0xFFFF) as f32 / 65535.0;
            let hi = ((substrate_words[w] >> 16) & 0xFFFF) as f32 / 65535.0;
            if w * 2 < 31 {
                substrate_bands[w * 2] = lo;
            }
            if w * 2 + 1 < 31 {
                substrate_bands[w * 2 + 1] = hi;
            }
        }

        let wet_total: f32 = wet_channels.iter().sum();
        let dry_total: f32 = dry_channels.iter().sum();
        let optical =
            crate::notes::compute_optical_diagnostics(wet_channels, dry_channels, substrate_bands);

        crate::notes::GpuDiagnostics {
            wet_channels,
            wet_total,
            dry_channels,
            dry_total,
            ink,
            display_srgb,
            optical,
            brush_loads,
        }
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

pub fn rack_bounds(slot: usize) -> [u32; 4] {
    [608 + slot as u32, SIDE, 1, 1]
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

pub fn load_png_rgba1024(path: &std::path::Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut decoder = png::Decoder::new(std::io::BufReader::new(file));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| e.to_string())?;
    let bytes = &buf[..info.buffer_size()];
    let src_w = info.width as usize;
    let src_h = info.height as usize;
    if src_w == 0 || src_h == 0 {
        return Err("Image has zero dimensions.".into());
    }

    let mut src_rgba = vec![255u8; src_w * src_h * 4];
    match info.color_type {
        png::ColorType::Rgba => src_rgba.copy_from_slice(bytes),
        png::ColorType::Rgb => {
            for (dst, src) in src_rgba.chunks_exact_mut(4).zip(bytes.chunks_exact(3)) {
                dst[0] = src[0];
                dst[1] = src[1];
                dst[2] = src[2];
            }
        }
        png::ColorType::GrayscaleAlpha => {
            for (dst, src) in src_rgba.chunks_exact_mut(4).zip(bytes.chunks_exact(2)) {
                dst[0] = src[0];
                dst[1] = src[0];
                dst[2] = src[0];
                dst[3] = src[1];
            }
        }
        png::ColorType::Grayscale => {
            for (dst, &g) in src_rgba.chunks_exact_mut(4).zip(bytes.iter()) {
                dst[0] = g;
                dst[1] = g;
                dst[2] = g;
            }
        }
        _ => return Err("Unsupported PNG color format.".into()),
    }

    let canvas_rgba = if src_w == SIDE as usize && src_h == SIDE as usize {
        src_rgba
    } else {
        let side = SIDE as usize;
        let mut dst = vec![255u8; side * side * 4];
        let scale = (side as f32 / src_w as f32).min(side as f32 / src_h as f32);
        let fit_w = (src_w as f32 * scale).round().max(1.0) as usize;
        let fit_h = (src_h as f32 * scale).round().max(1.0) as usize;
        let off_x = (side - fit_w) / 2;
        let off_y = (side - fit_h) / 2;
        for y in 0..fit_h {
            let sy = ((y as f32 + 0.5) / fit_h as f32 * src_h as f32 - 0.5)
                .clamp(0.0, (src_h - 1) as f32);
            let y0 = sy.floor() as usize;
            let y1 = (y0 + 1).min(src_h - 1);
            let fy = sy - y0 as f32;
            for x in 0..fit_w {
                let sx = ((x as f32 + 0.5) / fit_w as f32 * src_w as f32 - 0.5)
                    .clamp(0.0, (src_w - 1) as f32);
                let x0 = sx.floor() as usize;
                let x1 = (x0 + 1).min(src_w - 1);
                let fx = sx - x0 as f32;
                let out_idx = ((off_y + y) * side + (off_x + x)) * 4;
                for c in 0..4 {
                    let p00 = src_rgba[(y0 * src_w + x0) * 4 + c] as f32;
                    let p10 = src_rgba[(y0 * src_w + x1) * 4 + c] as f32;
                    let p01 = src_rgba[(y1 * src_w + x0) * 4 + c] as f32;
                    let p11 = src_rgba[(y1 * src_w + x1) * 4 + c] as f32;
                    let top = p00 * (1.0 - fx) + p10 * fx;
                    let bot = p01 * (1.0 - fx) + p11 * fx;
                    dst[out_idx + c] = (top * (1.0 - fy) + bot * fy).round() as u8;
                }
            }
        }
        dst
    };

    Ok(canvas_rgba)
}

pub fn load_png_as_reference(path: &std::path::Path) -> Result<std::sync::Arc<Vec<u32>>, String> {
    let rgba = load_png_rgba1024(path)?;
    let words: Vec<u32> = rgba
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    Ok(std::sync::Arc::new(words))
}

pub fn load_png_as_substrate(path: &std::path::Path) -> Result<std::sync::Arc<Vec<u32>>, String> {
    let canvas_rgba = load_png_rgba1024(path)?;
    Ok(std::sync::Arc::new(
        crate::pigment::uplift_srgb_to_substrate(&canvas_rgba),
    ))
}

#[cfg(test)]
pub fn export(buffer: wgpu::Buffer, path: PathBuf) -> Receiver<Result<PathBuf, String>> {
    export_with_srgb(buffer, path, true)
}

pub fn save_project(
    buffer: wgpu::Buffer,
    meta: crate::project::ProjectMeta,
    path: PathBuf,
) -> Receiver<Result<PathBuf, String>> {
    let (done, result) = mpsc::channel();
    let mapped_buffer = buffer.clone();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |mapped| {
            if let Err(error) = mapped {
                let _ = done.send(Err(format!("Could not read project buffers: {error}")));
                return;
            }
            std::thread::spawn(move || {
                let bytes = mapped_buffer.slice(..).get_mapped_range();
                let result = crate::project::write_project(&path, meta, &bytes).map(|()| path);
                drop(bytes);
                mapped_buffer.unmap();
                let _ = done.send(result);
            });
        });
    result
}

pub fn export_with_srgb(
    buffer: wgpu::Buffer,
    path: PathBuf,
    is_srgb: bool,
) -> Receiver<Result<PathBuf, String>> {
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
                let result = write_png(&path, &pixels, is_srgb).map(|()| path);
                drop(pixels);
                mapped_buffer.unmap();
                let _ = done.send(result);
            });
        });
    result
}

pub fn export_palette(
    buffer: wgpu::Buffer,
    occupied: [bool; crate::workbench::CUSTOM_JAR_COUNT],
    path: PathBuf,
) -> Receiver<Result<PathBuf, String>> {
    let (done, result) = mpsc::channel();
    let mapped_buffer = buffer.clone();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |mapped| {
            if let Err(error) = mapped {
                let _ = done.send(Err(format!("Could not read custom jars: {error}")));
                return;
            }
            std::thread::spawn(move || {
                let bytes = mapped_buffer.slice(..).get_mapped_range();
                let channels: &[[f32; 8]] = bytemuck::cast_slice(&bytes);
                let result = write_palette(&path, channels, &occupied).map(|()| path);
                drop(bytes);
                mapped_buffer.unmap();
                let _ = done.send(result);
            });
        });
    result
}

const CHANNEL_NAMES: [&str; 8] = [
    "Hansa Yellow (PY74)",
    "Phthalo Blue (PB15)",
    "Quinacridone Magenta (PR122)",
    "Titanium White (PW6)",
    "Ultramarine Blue (PB29)",
    "Burnt Sienna (PBr7)",
    "Vermilion / Cadmium Red (PR106/PR108)",
    "Bone Black / Sepia (PBk9)",
];

#[derive(serde::Serialize)]
struct ExportedPalette {
    format: &'static str,
    jars: Vec<ExportedJar>,
}

#[derive(serde::Serialize)]
struct ExportedJar {
    slot: usize,
    name: String,
    hex: String,
    srgb: [u8; 3],
    channels: [f32; 8],
    ratios: Vec<(&'static str, f32)>,
}

fn write_palette(
    path: &std::path::Path,
    channels: &[[f32; 8]],
    occupied: &[bool; crate::workbench::CUSTOM_JAR_COUNT],
) -> Result<(), String> {
    let entries: Vec<(String, [f32; 8])> = occupied
        .iter()
        .enumerate()
        .filter(|&(_, &used)| used)
        .map(|(slot, _)| (format!("Jar {}", slot + 1), channels[slot]))
        .collect();
    export_named_palette(path, &entries)
}

pub fn export_named_palette(
    path: &std::path::Path,
    entries: &[(String, [f32; 8])],
) -> Result<(), String> {
    let mut jars = Vec::with_capacity(entries.len());
    for (slot, (name, ch)) in entries.iter().enumerate() {
        let srgb = crate::pigment::rgb(*ch);
        let hex = format!("#{:02X}{:02X}{:02X}", srgb[0], srgb[1], srgb[2]);
        let total: f32 = ch.iter().sum();
        let ratios = CHANNEL_NAMES
            .iter()
            .zip(ch.iter())
            .filter(|(_, &v)| v > 0.0001)
            .map(|(&cname, &v)| (cname, if total > 0.0 { v / total } else { 0.0 }))
            .collect();
        jars.push(ExportedJar {
            slot: slot + 1,
            name: name.clone(),
            hex,
            srgb,
            channels: *ch,
            ratios,
        });
    }

    let is_gpl = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("gpl"));

    let content = if is_gpl {
        let mut out = String::from("GIMP Palette\nName: Pigment Studio\nColumns: 8\n#\n");
        for jar in &jars {
            out.push_str(&format!(
                "{:3} {:3} {:3}\t{}\n",
                jar.srgb[0], jar.srgb[1], jar.srgb[2], jar.name
            ));
        }
        out
    } else {
        serde_json::to_string_pretty(&ExportedPalette {
            format: "pigment-studio-palette-v1",
            jars,
        })
        .map_err(|e| e.to_string())?
    };

    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    std::io::Write::write_all(file.as_file_mut(), content.as_bytes()).map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

fn write_png(path: &std::path::Path, pixels: &[u8], is_srgb: bool) -> Result<(), String> {
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    let mut png = png::Encoder::new(file.as_file_mut(), SIDE, SIDE);
    png.set_color(png::ColorType::Rgba);
    png.set_depth(png::BitDepth::Eight);
    if is_srgb {
        png.set_source_srgb(png::SrgbRenderingIntent::RelativeColorimetric);
    }
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
                required_limits: wgpu::Limits {
                    max_storage_buffers_per_shader_stage: 12
                        .min(adapter.limits().max_storage_buffers_per_shader_stage),
                    ..Default::default()
                },
                ..Default::default()
            },
            None,
        )
        .await
        .map_err(|e| format!("Could not start Direct3D 12. Check your GPU driver. {e}"))?;
    Ok((device, queue, adapter))
}
