use bytemuck::{Pod, Zeroable};
use std::num::NonZeroU64;
use std::sync::mpsc;
use wgpu::util::DeviceExt;

pub const SIDE: u32 = 1024;
const ATLAS_HEIGHT: u32 = SIDE + 264;
const PAINT_BYTES: u64 = (SIDE as u64) * (ATLAS_HEIGHT as u64) * 32;
const DRY_BYTES: u64 = (SIDE as u64) * (SIDE as u64) * 32;
const SUBSTRATE_BYTES: u64 = (SIDE as u64) * (SIDE as u64) * 64;
const INK_BYTES: u64 = (SIDE as u64) * (SIDE as u64) * 8;
const REF_BYTES: u64 = (SIDE as u64) * (SIDE as u64) * 4;
const SKETCH_BYTES: u64 = (SIDE as u64) * (SIDE as u64) * 4;
const BRUSH_BYTES: u64 = 17 * 32 + 48;
const MAX_DABS: usize = 256;

#[repr(C)]
#[derive(Clone, Copy, Default, Pod, Zeroable)]
struct Dab {
    bounds: [u32; 4],
    brush: [f32; 4],
    axis: [f32; 4],
    mode: [u32; 4],
    tubes: [u32; 4],
    sweep: [f32; 4],
}

impl Dab {
    fn operation(bounds: [u32; 4], op: u32) -> Self {
        Self {
            bounds,
            mode: [op, 0, 0, 0],
            ..Default::default()
        }
    }
}

fn brush_load_start(brush_id: u32) -> u32 {
    match brush_id {
        0 => 0,  // Round
        1 => 1,  // Flat
        2 => 4,  // Filbert
        3 => 7,  // Fan
        4 => 10, // PaletteKnife
        5 => 13, // SoftMop
        _ => 0,
    }
}

fn brush_profile_id(brush_id: u32) -> u32 {
    brush_id.min(5)
}

fn dab_bounds(x: f32, y: f32, diameter: f32) -> [u32; 4] {
    let radius = (diameter * 0.5).max(1.0);
    let min_x = ((x - radius - 2.0).floor().max(0.0) as u32).min(SIDE - 1);
    let min_y = ((y - radius - 2.0).floor().max(0.0) as u32).min(SIDE - 1);
    let max_x = ((x + radius + 2.0).ceil().min(SIDE as f32) as u32).max(min_x + 1);
    let max_y = ((y + radius + 2.0).ceil().min(SIDE as f32) as u32).max(min_y + 1);
    [min_x, min_y, max_x - min_x, max_y - min_y]
}

pub struct PigmentSession {
    device: wgpu::Device,
    queue: wgpu::Queue,
    canvas: wgpu::Buffer,
    dry_canvas: wgpu::Buffer,
    dry_substrate: wgpu::Buffer,
    state: wgpu::Buffer,
    uniforms: wgpu::Buffer,
    stride: usize,
    bindings: wgpu::BindGroup,
    gather: wgpu::ComputePipeline,
    apply: wgpu::ComputePipeline,
    display: wgpu::ComputePipeline,
    service: wgpu::ComputePipeline,
    output: wgpu::Texture,
    readback: wgpu::Buffer,
    readback_padded_row: usize,
}

impl PigmentSession {
    pub fn new() -> Result<Self, String> {
        let inst = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12 | wgpu::Backends::VULKAN | wgpu::Backends::METAL,
            ..Default::default()
        });
        let adapter = pollster::block_on(inst.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .ok_or_else(|| "No GPU adapter found for pigment-blender.".to_string())?;
        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("pigment-blender GPU"),
                required_features: wgpu::Features::empty(),
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::Performance,
            },
            None,
        ))
        .map_err(|e| format!("Could not open GPU device: {e}"))?;

        use wgpu::{
            BufferBindingType as BufferType, BufferUsages as BufferUsage,
            TextureUsages as TextureUsage,
        };
        let mk_buf = |label, size, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let copies = BufferUsage::COPY_SRC | BufferUsage::COPY_DST;
        let canvas = mk_buf("canvas", PAINT_BYTES, copies | BufferUsage::STORAGE);
        let dry_canvas = mk_buf("dry_canvas", DRY_BYTES, copies | BufferUsage::STORAGE);
        let dry_substrate = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dry_substrate"),
            contents: &vec![0xFFu8; SUBSTRATE_BYTES as usize],
            usage: copies | BufferUsage::STORAGE,
        });
        let ink_step = mk_buf(
            "ink_step",
            u64::from(SIDE) * u64::from(SIDE) * 8,
            BufferUsage::STORAGE,
        );
        let ink_canvas = mk_buf("ink_canvas", INK_BYTES, copies | BufferUsage::STORAGE);
        let reference_layer = mk_buf("ref_layer", REF_BYTES, copies | BufferUsage::STORAGE);
        let sketch_canvas = mk_buf("sketch_canvas", SKETCH_BYTES, copies | BufferUsage::STORAGE);
        let paint_step = mk_buf("paint_step", DRY_BYTES, BufferUsage::STORAGE);
        let state = mk_buf("brush_state", BRUSH_BYTES, copies | BufferUsage::STORAGE);

        let band_data = pigment_engine::spectrum();
        let bands = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bands"),
            contents: bytemuck::cast_slice(&band_data),
            usage: BufferUsage::STORAGE,
        });

        let stride = size_of::<Dab>()
            .next_multiple_of(device.limits().min_uniform_buffer_offset_alignment as usize);
        let uniforms = mk_buf(
            "uniforms",
            (stride * (MAX_DABS + 8)) as u64,
            BufferUsage::UNIFORM | BufferUsage::COPY_DST,
        );

        let output = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("output"),
            size: wgpu::Extent3d {
                width: SIDE,
                height: ATLAS_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: TextureUsage::STORAGE_BINDING | TextureUsage::COPY_SRC,
            view_formats: &[],
        });

        let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT as usize;
        let readback_padded_row = (SIDE as usize * 4).div_ceil(align) * align;
        let readback = mk_buf(
            "readback",
            (readback_padded_row * SIDE as usize) as u64,
            BufferUsage::COPY_DST | BufferUsage::MAP_READ,
        );

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
            label: Some("pigment_blender_layout"),
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
                buffer_layout(11, BufferType::Storage { read_only: false }, false, None),
            ],
        });
        let view = output.create_view(&Default::default());
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("pigment_blender_bindings"),
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
                    resource: wgpu::BindingResource::TextureView(&view),
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
                wgpu::BindGroupEntry {
                    binding: 11,
                    resource: paint_step.as_entire_binding(),
                },
            ],
        });

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("paint.wgsl"),
            source: wgpu::ShaderSource::Wgsl(pigment_engine::PAINT_WGSL.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pigment_blender_pipeline_layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });
        let mk_pipe = |entry| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: Some(&pipeline_layout),
                module: &shader,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };

        let gather = mk_pipe("gather");
        let apply = mk_pipe("apply");
        let display = mk_pipe("display");
        let service = mk_pipe("service");

        let session = Self {
            device,
            queue,
            canvas,
            dry_canvas,
            dry_substrate,
            state,
            uniforms,
            stride,
            bindings,
            gather,
            apply,
            display,
            service,
            output,
            readback,
            readback_padded_row,
        };
        session.clear();
        Ok(session)
    }

    fn dispatch_dab(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        pipeline: &wgpu::ComputePipeline,
        slot: usize,
        dab: &Dab,
        groups: [u32; 2],
    ) {
        let offset = (self.stride * slot) as u64;
        self.queue
            .write_buffer(&self.uniforms, offset, bytemuck::bytes_of(dab));
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bindings, &[offset as u32]);
        pass.dispatch_workgroups(groups[0], groups[1], 1);
    }

    pub fn clear(&self) {
        self.queue.write_buffer(
            &self.dry_substrate,
            0,
            &vec![0xFFu8; SUBSTRATE_BYTES as usize],
        );
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.clear_buffer(&self.canvas, 0, None);
        encoder.clear_buffer(&self.dry_canvas, 0, None);
        encoder.clear_buffer(&self.state, 0, None);
        let init_dab = Dab::operation([0, 0, SIDE, SIDE], 1);
        self.dispatch_dab(
            &mut encoder,
            &self.display,
            0,
            &init_dab,
            [SIDE / 8, SIDE / 8],
        );
        self.queue.submit([encoder.finish()]);
    }

    pub fn dry_canvas(&self) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let dry = Dab::operation([0, 0, SIDE, SIDE], 14);
        self.dispatch_dab(&mut encoder, &self.apply, 0, &dry, [SIDE / 8, SIDE / 8]);
        let disp = Dab::operation([0, 0, SIDE, SIDE], 1);
        self.dispatch_dab(&mut encoder, &self.display, 1, &disp, [SIDE / 8, SIDE / 8]);
        self.queue.submit([encoder.finish()]);
    }

    pub fn wipe(&self, brush_id: u32, fraction: f32) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut dab = Dab::operation([0, 0, 1, 1], 6);
        dab.mode[1] = brush_load_start(brush_id);
        dab.brush[3] = fraction.clamp(0.0, 1.0);
        self.dispatch_dab(&mut encoder, &self.service, 0, &dab, [1, 1]);
        self.queue.submit([encoder.finish()]);
    }

    pub fn set_mix(&self, brush_id: u32, recipe: [f32; 8], clean: bool) {
        if clean {
            self.wipe(brush_id, 0.0);
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let equip = Dab {
            bounds: [608, SIDE, 1, 1],
            brush: [
                recipe[0] * 45.0,
                recipe[1] * 45.0,
                recipe[2] * 45.0,
                recipe[3] * 45.0,
            ],
            axis: [
                recipe[4] * 45.0,
                recipe[5] * 45.0,
                recipe[6] * 45.0,
                recipe[7] * 45.0,
            ],
            mode: [13, 0, 0, 0],
            ..Default::default()
        };
        self.dispatch_dab(&mut encoder, &self.apply, 0, &equip, [1, 1]);

        let mut dip = Dab::operation([0, 0, 1, 1], 12);
        dip.mode[1] = brush_load_start(brush_id);
        dip.mode[2] = 0;
        dip.brush[3] = 46000.0;
        self.dispatch_dab(&mut encoder, &self.service, 1, &dip, [1, 1]);
        self.queue.submit([encoder.finish()]);
    }

    pub fn stroke(
        &self,
        brush_id: u32,
        diameter: f32,
        angle_rad: f32,
        auto_follow: bool,
        points: &[[f32; 3]],
    ) {
        if points.is_empty() {
            return;
        }
        let load_start = brush_load_start(brush_id);
        let profile = brush_profile_id(brush_id);
        let step = (diameter * 0.22).max(1.5);
        let mut dabs = Vec::new();
        for w in 0..points.len() {
            let [x0, y0, p0] = points[w];
            if w == 0 {
                let (ax, ay) = (angle_rad.cos(), angle_rad.sin());
                dabs.push(Dab {
                    bounds: dab_bounds(x0, y0, diameter),
                    brush: [x0, y0, diameter * 0.5, p0.clamp(0.05, 1.0)],
                    axis: [ax, ay, 1.0, 0.0],
                    mode: [0, load_start, profile, 0],
                    ..Default::default()
                });
                continue;
            }
            let [px, py, pp] = points[w - 1];
            let dx = x0 - px;
            let dy = y0 - py;
            let dist = (dx * dx + dy * dy).sqrt();
            let dir_angle = if auto_follow && dist > 0.5 {
                dy.atan2(dx)
            } else {
                angle_rad
            };
            let (ax, ay) = (dir_angle.cos(), dir_angle.sin());
            let n = (dist / step).ceil().max(1.0) as usize;
            for i in 1..=n {
                let t = i as f32 / n as f32;
                let ix = px + dx * t;
                let iy = py + dy * t;
                let ip = (pp + (p0 - pp) * t).clamp(0.05, 1.0);
                dabs.push(Dab {
                    bounds: dab_bounds(ix, iy, diameter),
                    brush: [ix, iy, diameter * 0.5, ip],
                    axis: [ax, ay, 1.0, 0.0],
                    mode: [0, load_start, profile, 0],
                    ..Default::default()
                });
            }
        }

        for chunk in dabs.chunks(MAX_DABS) {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            for (i, dab) in chunk.iter().enumerate() {
                let groups = [dab.bounds[2].div_ceil(8), dab.bounds[3].div_ceil(8)];
                self.dispatch_dab(&mut encoder, &self.gather, i, dab, [1, 1]);
                self.dispatch_dab(&mut encoder, &self.apply, i, dab, groups);
            }
            self.queue.submit([encoder.finish()]);
        }
    }

    fn readback_mode_f32(
        &self,
        export_mode: u32,
        gl_normals: bool,
        smooth_height: bool,
        out: &mut [f32],
    ) -> Result<(), String> {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut dab = Dab::operation([0, 0, SIDE, SIDE], 1);
        dab.mode[1] = 0; // unlit flat albedo for Blender PBR base color
        dab.mode[2] = export_mode;
        dab.mode[3] =
            (u32::from(smooth_height) << 2) | (u32::from(export_mode == 3 && gl_normals) << 3);
        dab.brush = [1.0, 0.0, 0.0, 0.0]; // impasto_scale = 1.0, weave_scale = 0.0
        self.dispatch_dab(&mut encoder, &self.display, 0, &dab, [SIDE / 8, SIDE / 8]);
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.output,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &self.readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(self.readback_padded_row as u32),
                    rows_per_image: Some(SIDE),
                },
            },
            wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);

        let (tx, rx) = mpsc::channel();
        self.readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
        let _ = self.device.poll(wgpu::Maintain::Wait);
        rx.recv()
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;

        let data = self.readback.slice(..).get_mapped_range();
        let row_bytes = SIDE as usize * 4;
        // Blender Image.pixels is bottom-to-top; flip Y during copy so UVs align!
        for y in 0..SIDE as usize {
            let src_y = (SIDE as usize - 1) - y;
            let src_row = &data
                [src_y * self.readback_padded_row..src_y * self.readback_padded_row + row_bytes];
            let dst_base = y * row_bytes;
            for (i, &b) in src_row.iter().enumerate() {
                out[dst_base + i] = b as f32 / 255.0;
            }
        }
        drop(data);
        self.readback.unmap();
        Ok(())
    }
}

#[no_mangle]
pub extern "C" fn pigment_session_create() -> *mut PigmentSession {
    match PigmentSession::new() {
        Ok(s) => Box::into_raw(Box::new(s)),
        Err(_) => std::ptr::null_mut(),
    }
}

/// # Safety
/// `session` must be null or a pointer returned by `pigment_session_create`.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_destroy(session: *mut PigmentSession) {
    if !session.is_null() {
        drop(Box::from_raw(session));
    }
}

/// # Safety
/// `session` must be a valid pointer from `pigment_session_create`.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_clear(session: *mut PigmentSession) {
    if let Some(s) = session.as_ref() {
        s.clear();
    }
}

/// # Safety
/// `session` must be a valid pointer from `pigment_session_create`.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_dry_canvas(session: *mut PigmentSession) {
    if let Some(s) = session.as_ref() {
        s.dry_canvas();
    }
}

/// # Safety
/// `session` must be valid and `recipe_ptr` must point to 8 `f32` values.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_set_mix(
    session: *mut PigmentSession,
    brush_id: u32,
    recipe_ptr: *const f32,
    clean: bool,
) {
    if let Some(s) = session.as_ref() {
        if !recipe_ptr.is_null() {
            let slice = std::slice::from_raw_parts(recipe_ptr, 8);
            let mut rec = [0.0f32; 8];
            rec.copy_from_slice(slice);
            s.set_mix(brush_id, rec, clean);
        }
    }
}

/// # Safety
/// `session` must be a valid pointer from `pigment_session_create`.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_wipe(
    session: *mut PigmentSession,
    brush_id: u32,
    fraction: f32,
) {
    if let Some(s) = session.as_ref() {
        s.wipe(brush_id, fraction);
    }
}

/// # Safety
/// `session` must be valid and `pts_xy_p` must point to `count * 3` `f32` values `(x, y, pressure)`.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_stroke(
    session: *mut PigmentSession,
    brush_id: u32,
    diameter: f32,
    angle_rad: f32,
    auto_follow: bool,
    pts_xy_p: *const f32,
    count: u32,
) {
    if let Some(s) = session.as_ref() {
        if !pts_xy_p.is_null() && count > 0 {
            let raw = std::slice::from_raw_parts(pts_xy_p, count as usize * 3);
            let pts: Vec<[f32; 3]> = raw.chunks_exact(3).map(|c| [c[0], c[1], c[2]]).collect();
            s.stroke(brush_id, diameter, angle_rad, auto_follow, &pts);
        }
    }
}

/// # Safety
/// Non-null output pointers must each have capacity for `1024 * 1024 * 4` `f32` elements.
#[no_mangle]
pub unsafe extern "C" fn pigment_session_readback_pbr(
    session: *mut PigmentSession,
    out_color_f32: *mut f32,
    out_normal_f32: *mut f32,
    out_height_f32: *mut f32,
    gl_normals: bool,
    smooth_height: bool,
) -> bool {
    let Some(s) = session.as_ref() else {
        return false;
    };
    let len = SIDE as usize * SIDE as usize * 4;
    if !out_color_f32.is_null()
        && s.readback_mode_f32(
            0,
            gl_normals,
            smooth_height,
            std::slice::from_raw_parts_mut(out_color_f32, len),
        )
        .is_err()
    {
        return false;
    }
    if !out_normal_f32.is_null()
        && s.readback_mode_f32(
            3,
            gl_normals,
            smooth_height,
            std::slice::from_raw_parts_mut(out_normal_f32, len),
        )
        .is_err()
    {
        return false;
    }
    if !out_height_f32.is_null()
        && s.readback_mode_f32(
            2,
            gl_normals,
            smooth_height,
            std::slice::from_raw_parts_mut(out_height_f32, len),
        )
        .is_err()
    {
        return false;
    }
    true
}

/// # Safety
/// `recipe_ptr` must point to 8 `f32`s and `out_rgb` to 3 `u8`s.
#[no_mangle]
pub unsafe extern "C" fn pigment_mix_preview_srgb(recipe_ptr: *const f32, out_rgb: *mut u8) {
    if recipe_ptr.is_null() || out_rgb.is_null() {
        return;
    }
    let slice = std::slice::from_raw_parts(recipe_ptr, 8);
    let mut rec = [0.0f32; 8];
    rec.copy_from_slice(slice);
    let rgb = pigment_engine::rgb(rec);
    std::slice::from_raw_parts_mut(out_rgb, 3).copy_from_slice(&rgb);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_session_paints_and_reads_pbr_maps() {
        let s = PigmentSession::new().expect("GPU session");
        // Mix Hansa Yellow + Phthalo Blue -> green
        s.set_mix(0, [0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0], true);
        s.stroke(
            0,
            64.0,
            0.0,
            true,
            &[[400.0, 512.0, 1.0], [620.0, 512.0, 1.0]],
        );

        let len = SIDE as usize * SIDE as usize * 4;
        let mut color = vec![0.0f32; len];
        let mut normal = vec![0.0f32; len];
        let mut height = vec![0.0f32; len];
        s.readback_mode_f32(0, true, true, &mut color).unwrap();
        s.readback_mode_f32(3, true, true, &mut normal).unwrap();
        s.readback_mode_f32(2, true, true, &mut height).unwrap();

        // Center pixel (512, 512) -> flipped Y is still 511
        let idx = (511 * SIDE as usize + 512) * 4;
        assert!(
            color[idx + 1] > color[idx] && color[idx + 1] > color[idx + 2],
            "expected green paint, got {:?}",
            &color[idx..idx + 4]
        );
        assert!(height[idx] > 0.13, "expected raised paint height");
        assert!(
            (normal[idx + 2] - 1.0).abs() < 0.2,
            "expected normal Z near +1"
        );
    }
}
