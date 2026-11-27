use crate::gpu::{
    self, Brush, BrushLane, BrushService, Command, ExportKind, Lighting, PaintGpu, Surface,
    MAX_COMMANDS,
};
use crate::stroke::{Sample, Source, Stroke};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushKind {
    Round,
    Flat,
    Filbert,
    Fan,
    PaletteKnife,
    SoftMop,
}

impl From<Brush> for BrushKind {
    fn from(b: Brush) -> Self {
        match b {
            Brush::Round => Self::Round,
            Brush::Flat => Self::Flat,
            Brush::Filbert => Self::Filbert,
            Brush::Fan => Self::Fan,
            Brush::PaletteKnife => Self::PaletteKnife,
            Brush::SoftMop => Self::SoftMop,
        }
    }
}

impl From<BrushKind> for Brush {
    fn from(b: BrushKind) -> Self {
        match b {
            BrushKind::Round => Self::Round,
            BrushKind::Flat => Self::Flat,
            BrushKind::Filbert => Self::Filbert,
            BrushKind::Fan => Self::Fan,
            BrushKind::PaletteKnife => Self::PaletteKnife,
            BrushKind::SoftMop => Self::SoftMop,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SurfaceKind {
    Canvas,
    Palette,
}

impl From<Surface> for SurfaceKind {
    fn from(s: Surface) -> Self {
        match s {
            Surface::Canvas => Self::Canvas,
            Surface::Palette => Self::Palette,
        }
    }
}

impl From<SurfaceKind> for Surface {
    fn from(s: SurfaceKind) -> Self {
        match s {
            SurfaceKind::Canvas => Self::Canvas,
            SurfaceKind::Palette => Self::Palette,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ServiceAction {
    Load {
        tube: usize,
    },
    Dip {
        tube: usize,
    },
    DipCustom {
        slot: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lane: Option<BrushLane>,
    },
    DipRack {
        slot: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lane: Option<BrushLane>,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        clean: bool,
    },
    Wipe {
        fraction: f32,
    },
}

impl From<BrushService> for ServiceAction {
    fn from(s: BrushService) -> Self {
        match s {
            BrushService::Load(tube) => Self::Load { tube },
            BrushService::Dip(tube) => Self::Dip { tube },
            BrushService::DipCustom(slot) => Self::DipCustom { slot, lane: None },
            BrushService::DipRack(slot) => Self::DipRack {
                slot,
                lane: None,
                clean: false,
            },
            BrushService::DipCustomLane(slot, lane) => Self::DipCustom {
                slot,
                lane: Some(lane),
            },
            BrushService::DipRackLane(slot, lane) => Self::DipRack {
                slot,
                lane: Some(lane),
                clean: false,
            },
            BrushService::Wipe(fraction) => Self::Wipe { fraction },
        }
    }
}

impl From<ServiceAction> for BrushService {
    fn from(s: ServiceAction) -> Self {
        match s {
            ServiceAction::Load { tube } => Self::Load(tube),
            ServiceAction::Dip { tube } => Self::Dip(tube),
            ServiceAction::DipCustom { slot, lane: None } => Self::DipCustom(slot),
            ServiceAction::DipRack {
                slot, lane: None, ..
            } => Self::DipRack(slot),
            ServiceAction::DipCustom {
                slot,
                lane: Some(lane),
            } => Self::DipCustomLane(slot, lane),
            ServiceAction::DipRack {
                slot,
                lane: Some(lane),
                ..
            } => Self::DipRackLane(slot, lane),
            ServiceAction::Wipe { fraction } => Self::Wipe(fraction),
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct RecordedSample {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
    #[serde(default)]
    pub timestamp_ms: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordedEvent {
    Service {
        brush: BrushKind,
        action: ServiceAction,
        #[serde(default)]
        legacy_reservoir: bool,
    },
    EquipRack {
        slot: usize,
        recipe: [f32; 8],
    },
    SetMix {
        brush: BrushKind,
        recipe: [f32; 8],
        #[serde(default = "default_true")]
        clean: bool,
        #[serde(default)]
        legacy_reservoir: bool,
    },
    Stroke {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        surface: SurfaceKind,
        brush: BrushKind,
        #[serde(default)]
        legacy_reservoir: bool,
        #[serde(default)]
        ink: bool,
        #[serde(default)]
        charcoal: bool,
        #[serde(default)]
        erase: bool,
        #[serde(default = "default_diameter")]
        diameter: f32,
        #[serde(default)]
        angle_deg: f32,
        #[serde(default)]
        auto_follow: bool,
        #[serde(default)]
        knife_sweep: bool,
        #[serde(default)]
        bristle_sweep: bool,
        #[serde(default)]
        filbert_sweep: bool,
        #[serde(default)]
        fan_sweep: bool,
        #[serde(default)]
        mop_sweep: bool,
        #[serde(default)]
        round_pressure: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tile: Option<bool>,
        samples: Vec<RecordedSample>,
    },
    Clear {
        surface: SurfaceKind,
    },
    ClearSketch,
    ClearReference,
    DryCanvas,
    SaveSwatch {
        slot: usize,
        surface: SurfaceKind,
        x: u32,
        y: u32,
    },
    RemoveSwatch {
        slot: usize,
    },
    SetTile {
        enabled: bool,
    },
    ImportUnderlayer {
        path: String,
    },
    #[serde(skip)]
    ImportReference {
        words: std::sync::Arc<Vec<u32>>,
    },
    Splatter {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        group: Option<String>,
        brush: BrushKind,
        #[serde(default)]
        settings: crate::splatter::SplatterSettings,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tile: Option<bool>,
        samples: Vec<RecordedSample>,
    },
    GravityStep {
        strength: f32,
        viscosity: f32,
        angle_deg: f32,
        #[serde(default = "default_one")]
        steps: u32,
    },
}

fn default_one() -> u32 {
    1
}

fn default_true() -> bool {
    true
}

fn default_diameter() -> f32 {
    64.0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LightingConfig {
    pub enabled: bool,
    pub impasto_depth: f32,
    pub canvas_weave: f32,
    pub specular_sheen: f32,
    pub light_angle_deg: f32,
    pub light_elevation_deg: f32,
}

impl From<Lighting> for LightingConfig {
    fn from(l: Lighting) -> Self {
        Self {
            enabled: l.enabled,
            impasto_depth: l.impasto_depth,
            canvas_weave: l.canvas_weave,
            specular_sheen: l.specular_sheen,
            light_angle_deg: l.light_angle_deg,
            light_elevation_deg: l.light_elevation_deg,
        }
    }
}

impl From<LightingConfig> for Lighting {
    fn from(l: LightingConfig) -> Self {
        Self {
            enabled: l.enabled,
            impasto_depth: l.impasto_depth,
            canvas_weave: l.canvas_weave,
            specular_sheen: l.specular_sheen,
            light_angle_deg: l.light_angle_deg,
            light_elevation_deg: l.light_elevation_deg,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Recording {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas_width: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub canvas_height: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underlayer_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lighting: Option<LightingConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile: Option<bool>,
    pub events: Vec<RecordedEvent>,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            format: "pigment-studio-recording-v2".into(),
            canvas_width: None,
            canvas_height: None,
            underlayer_path: None,
            lighting: None,
            tile: None,
            events: Vec::new(),
        }
    }
}

impl Recording {
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut recording: Self = serde_json::from_str(text).map_err(|e| e.to_string())?;
        recording.upgrade_legacy_reservoirs();
        Ok(recording)
    }

    pub fn upgrade_legacy_reservoirs(&mut self) {
        if self.format != "pigment-studio-recording-v1" {
            return;
        }
        self.format = "pigment-studio-recording-v2".into();
        for event in &mut self.events {
            match event {
                RecordedEvent::Stroke {
                    legacy_reservoir, ..
                }
                | RecordedEvent::Service {
                    legacy_reservoir, ..
                } => *legacy_reservoir = true,
                _ => {}
            }
        }
    }
}

pub fn compile_events(events: &[RecordedEvent], tile_state: &mut bool) -> Vec<Command> {
    compile_events_in(events, tile_state, None).unwrap_or_default()
}

pub fn resolve_path(base_dir: Option<&Path>, raw: &str) -> std::path::PathBuf {
    let p = Path::new(raw);
    if p.is_absolute() {
        p.to_path_buf()
    } else if let Some(dir) = base_dir {
        dir.join(p)
    } else {
        p.to_path_buf()
    }
}

pub fn compile_events_in(
    events: &[RecordedEvent],
    tile_state: &mut bool,
    base_dir: Option<&Path>,
) -> Result<Vec<Command>, String> {
    compile_events_dims(events, tile_state, base_dir, gpu::CanvasDims::default())
}

pub fn compile_events_dims(
    events: &[RecordedEvent],
    tile_state: &mut bool,
    base_dir: Option<&Path>,
    dims: gpu::CanvasDims,
) -> Result<Vec<Command>, String> {
    let mut commands = Vec::new();
    for event in events {
        match event {
            RecordedEvent::Service {
                brush,
                action,
                legacy_reservoir,
            } => {
                let service = (*action).into();
                if *brush != BrushKind::Flat
                    && matches!(
                        service,
                        BrushService::DipRackLane(..) | BrushService::DipCustomLane(..)
                    )
                {
                    return Err("Split jar dips require the flat brush.".into());
                }
                let brush = Brush::from(*brush);
                let reservoir = if *legacy_reservoir {
                    brush.legacy_reservoir()
                } else {
                    brush
                };
                if matches!(action, ServiceAction::DipRack { clean: true, .. }) {
                    commands.push(Command::Service(reservoir, BrushService::Wipe(0.0)));
                }
                commands.push(Command::Service(reservoir, service));
            }
            RecordedEvent::EquipRack { slot, recipe } => {
                commands.push(Command::EquipRack(*slot, *recipe));
            }
            RecordedEvent::SetMix {
                brush,
                recipe,
                clean,
                legacy_reservoir,
            } => {
                let brush = Brush::from(*brush);
                let reservoir = if *legacy_reservoir {
                    brush.legacy_reservoir()
                } else {
                    brush
                };
                if *clean {
                    commands.push(Command::Service(reservoir, BrushService::Wipe(0.0)));
                }
                commands.push(Command::EquipRack(19, *recipe));
                commands.push(Command::Service(reservoir, BrushService::DipRack(19)));
            }
            RecordedEvent::Stroke {
                id: _,
                group: _,
                surface,
                brush,
                legacy_reservoir,
                ink,
                charcoal,
                erase,
                diameter,
                angle_deg,
                auto_follow,
                knife_sweep,
                bristle_sweep,
                filbert_sweep,
                fan_sweep,
                mop_sweep,
                round_pressure,
                tile,
                samples,
            } => {
                let Some((first, rest)) = samples.split_first() else {
                    continue;
                };
                let to_sample = |s: &RecordedSample| Sample {
                    point: egui::pos2(s.x, s.y),
                    pressure: s.pressure,
                    timestamp_ms: s.timestamp_ms,
                    angle: s.angle_deg.map(f32::to_radians),
                };
                let stroke_tile = tile.unwrap_or(*tile_state);
                let first_command = commands.len();
                let mut stroke = if *charcoal && *surface == SurfaceKind::Canvas {
                    Stroke::start_charcoal_dims(
                        Source::Mouse,
                        to_sample(first),
                        *diameter,
                        *erase,
                        stroke_tile,
                        dims,
                        &mut commands,
                    )
                } else if *ink && *surface == SurfaceKind::Canvas {
                    Stroke::start_ink_dims(
                        Source::Mouse,
                        to_sample(first),
                        stroke_tile,
                        dims,
                        &mut commands,
                    )
                } else {
                    Stroke::start_full_dims(
                        Source::Mouse,
                        to_sample(first),
                        *diameter,
                        (*surface).into(),
                        (*brush).into(),
                        angle_deg.to_radians(),
                        *auto_follow,
                        1.0,
                        stroke_tile,
                        (*brush == BrushKind::PaletteKnife && *knife_sweep)
                            || (*brush == BrushKind::Flat && *bristle_sweep)
                            || (*brush == BrushKind::Filbert && *filbert_sweep)
                            || (*brush == BrushKind::Fan && *fan_sweep)
                            || (*brush == BrushKind::SoftMop && *mop_sweep)
                            || (*brush == BrushKind::Round && *round_pressure),
                        dims,
                        &mut commands,
                    )
                };
                for s in rest {
                    stroke.extend(to_sample(s), &mut commands);
                }
                stroke.finish(&mut commands);
                if *legacy_reservoir {
                    for command in &mut commands[first_command..] {
                        if let Command::Dab(dab) = command {
                            dab.use_legacy_reservoir((*brush).into());
                        }
                    }
                }
            }
            RecordedEvent::Clear { surface } => {
                commands.push(Command::Clear((*surface).into()));
            }
            RecordedEvent::ClearSketch => {
                commands.push(Command::ClearSketch);
            }
            RecordedEvent::ClearReference => {
                commands.push(Command::ClearReference);
            }
            RecordedEvent::DryCanvas => {
                commands.push(Command::DryCanvas);
            }
            RecordedEvent::SaveSwatch {
                slot,
                surface,
                x,
                y,
            } => {
                commands.push(Command::SaveSwatch(*slot, (*surface).into(), [*x, *y]));
            }
            RecordedEvent::RemoveSwatch { slot } => {
                commands.push(Command::RemoveSwatch(*slot));
            }
            RecordedEvent::SetTile { enabled } => {
                *tile_state = *enabled;
            }
            RecordedEvent::ImportUnderlayer { path } => {
                let resolved = resolve_path(base_dir, path);
                let words = gpu::load_png_as_substrate(&resolved)?;
                commands.push(Command::ImportUnderlayer(words));
            }
            RecordedEvent::ImportReference { words } => {
                commands.push(Command::ImportReference(words.clone()));
            }
            RecordedEvent::Splatter {
                brush,
                settings,
                tile,
                samples,
                ..
            } => {
                let Some(first) = samples.first() else {
                    continue;
                };
                let to_sample = |s: &RecordedSample| Sample {
                    point: egui::pos2(s.x, s.y),
                    pressure: s.pressure,
                    timestamp_ms: s.timestamp_ms,
                    angle: s.angle_deg.map(f32::to_radians),
                };
                let stroke_tile = tile.unwrap_or(*tile_state);
                let mut stroke = Stroke::start_splatter_dims(
                    Source::Mouse,
                    to_sample(first),
                    (*brush).into(),
                    stroke_tile,
                    *settings,
                    dims,
                    &mut commands,
                );
                for s in &samples[1..] {
                    stroke.extend(to_sample(s), &mut commands);
                }
                stroke.finish(&mut commands);
            }
            RecordedEvent::GravityStep {
                strength,
                viscosity,
                angle_deg,
                steps,
            } => {
                for _ in 0..(*steps).min(600) {
                    commands.push(Command::AdvanceGravity {
                        strength: *strength,
                        viscosity: *viscosity,
                        angle_deg: *angle_deg,
                    });
                }
            }
        }
    }
    Ok(commands)
}

pub fn equip_cabinet(commands: &mut Vec<Command>) {
    for (slot, p) in crate::cabinet::PIGMENTS.iter().enumerate() {
        commands.push(Command::EquipRack(slot, p.recipe));
    }
}

fn export_mode_name(mode: ExportKind) -> &'static str {
    match mode {
        ExportKind::Color => "color",
        ExportKind::Transparent => "transparent",
        ExportKind::Height => "height",
        ExportKind::Normal => "normal",
    }
}

#[cfg(test)]
pub fn render_headless(
    recording_path: &Path,
    out_path: &Path,
    mode: ExportKind,
    underlayer_override: Option<&Path>,
) -> Result<usize, String> {
    let modes = vec![(export_mode_name(mode).to_string(), mode)];
    let (events, _) = render_modes_headless(
        recording_path,
        out_path,
        &modes,
        underlayer_override,
        false,
        false,
    )?;
    Ok(events)
}

pub fn render_modes_headless(
    recording_path: &Path,
    out_path: &Path,
    modes: &[(String, ExportKind)],
    underlayer_override: Option<&Path>,
    smooth_height: bool,
    gl_normals: bool,
) -> Result<(usize, Vec<(String, std::path::PathBuf)>), String> {
    let text = std::fs::read_to_string(recording_path).map_err(|e| e.to_string())?;
    let recording = Recording::parse(&text)?;
    let base_dir = recording_path
        .parent()
        .filter(|p| !p.as_os_str().is_empty());

    let inst = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    let (device, queue, _) = pollster::block_on(gpu::device(&inst, None))?;
    let mut paint = PaintGpu::new(&device);
    paint.smooth_height = smooth_height;
    paint.gl_normals = gl_normals;
    render_modes_on_gpu(
        &device,
        &queue,
        &mut paint,
        &recording,
        base_dir,
        underlayer_override,
        out_path,
        modes,
    )
}

#[allow(clippy::too_many_arguments)]
fn render_one(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    paint: &mut PaintGpu,
    recording: &Recording,
    base_dir: Option<&Path>,
    underlayer_override: Option<&Path>,
    out_path: &Path,
    mode: ExportKind,
) -> Result<usize, String> {
    let modes = vec![(export_mode_name(mode).to_string(), mode)];
    let (events, _) = render_modes_on_gpu(
        device,
        queue,
        paint,
        recording,
        base_dir,
        underlayer_override,
        out_path,
        &modes,
    )?;
    Ok(events)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn render_modes_on_gpu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    paint: &mut PaintGpu,
    recording: &Recording,
    base_dir: Option<&Path>,
    underlayer_override: Option<&Path>,
    out_path: &Path,
    modes: &[(String, ExportKind)],
) -> Result<(usize, Vec<(String, std::path::PathBuf)>), String> {
    if let (Some(w), Some(h)) = (recording.canvas_width, recording.canvas_height) {
        paint.resize_canvas(gpu::CanvasDims::new(w, h));
    }
    paint.lighting = recording
        .lighting
        .clone()
        .map(Into::into)
        .unwrap_or(Lighting {
            enabled: false,
            ..Default::default()
        });
    paint.tile = recording.tile.unwrap_or(false);

    let mut commands = vec![Command::Initialize];
    equip_cabinet(&mut commands);

    if let Some(ul_path) = underlayer_override {
        let words = gpu::load_png_as_substrate_sized(ul_path, paint.dims)?;
        commands.push(Command::ImportUnderlayer(words));
    } else if let Some(ref raw) = recording.underlayer_path {
        let resolved = resolve_path(base_dir, raw);
        let words = gpu::load_png_as_substrate_sized(&resolved, paint.dims)?;
        commands.push(Command::ImportUnderlayer(words));
    }

    let mut tile_state = paint.tile;
    commands.extend(compile_events_dims(
        &recording.events,
        &mut tile_state,
        base_dir,
        paint.dims,
    )?);
    paint.tile = tile_state;

    for chunk in commands.chunks(MAX_COMMANDS) {
        let mut encoder = device.create_command_encoder(&Default::default());
        paint.encode(device, queue, &mut encoder, chunk);
        queue.submit([encoder.finish()]);
    }
    while paint.ink_is_wet() {
        let mut encoder = device.create_command_encoder(&Default::default());
        paint.encode(device, queue, &mut encoder, &[Command::AdvanceInk]);
        queue.submit([encoder.finish()]);
    }

    let mut outputs = Vec::with_capacity(modes.len());
    let multi = modes.len() > 1;
    if out_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pigment"))
    {
        let mut encoder = device.create_command_encoder(&Default::default());
        let buffer = paint.readback_project(device, &mut encoder);
        queue.submit([encoder.finish()]);
        let meta = crate::project::ProjectMeta {
            calibration_version: crate::project::CURRENT_CALIBRATION_VERSION,
            canvas_width: paint.dims.width,
            canvas_height: paint.dims.height,
            loads: [0.0; crate::gpu::BRUSH_COUNT],
            jars: vec![false; crate::workbench::CUSTOM_JAR_COUNT],
            sizes: [48.0, 64.0],
            angle: 0.0,
            brush: BrushKind::Round,
            ink_selected: false,
            charcoal_erase: false,
            charcoal_size: 14.0,
            lighting: paint.lighting.into(),
            layers: paint.layers.into(),
            tile: paint.tile,
            recording: recording.clone(),
        };
        let rx = gpu::save_project(buffer, meta, out_path.to_path_buf());
        let _ = device.poll(wgpu::Maintain::Wait);
        rx.recv().map_err(|e| e.to_string())??;
        outputs.push(("project".to_string(), out_path.to_path_buf()));
    } else {
        for (mode_name, mode) in modes {
            let target = format_mode_out_path(out_path, mode_name, multi);
            if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty()) {
                std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let mut encoder = device.create_command_encoder(&Default::default());
            let buffer = paint.readback_kind(device, queue, &mut encoder, *mode);
            queue.submit([encoder.finish()]);
            let rx = gpu::export_sized(buffer, paint.dims, target.clone(), mode.is_srgb());
            let _ = device.poll(wgpu::Maintain::Wait);
            rx.recv().map_err(|e| e.to_string())??;
            outputs.push((mode_name.clone(), target));
        }
    }
    Ok((recording.events.len(), outputs))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchManifest {
    pub format: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_recording: Option<String>,
    pub jobs: Vec<BatchJob>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BatchJob {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub underlayer: Option<String>,
    pub out: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default)]
    pub smooth_height: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normal_convention: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct BatchJobOutput {
    pub index: usize,
    pub out: String,
    pub mode: String,
    pub events: usize,
}

pub fn parse_export_mode(mode_str: &str) -> Result<ExportKind, String> {
    match mode_str.trim() {
        "color" => Ok(ExportKind::Color),
        "transparent" => Ok(ExportKind::Transparent),
        "height" => Ok(ExportKind::Height),
        "normal" => Ok(ExportKind::Normal),
        other => Err(format!(
            "Unknown mode '{other}'. Expected color, transparent, height, or normal."
        )),
    }
}

pub fn parse_export_modes(spec: &str) -> Result<Vec<(String, ExportKind)>, String> {
    let mut out = Vec::new();
    for raw in spec.split(',') {
        let name = raw.trim();
        if name.is_empty() {
            continue;
        }
        out.push((name.to_string(), parse_export_mode(name)?));
    }
    if out.is_empty() {
        return Err("No export mode specified.".into());
    }
    Ok(out)
}

pub fn format_mode_out_path(out_path: &Path, mode_name: &str, multi: bool) -> std::path::PathBuf {
    let raw = out_path.to_string_lossy();
    if raw.contains("{mode}") {
        return std::path::PathBuf::from(raw.replace("{mode}", mode_name));
    }
    if !multi {
        return out_path.to_path_buf();
    }
    let stem = out_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("render");
    let ext = out_path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png");
    let file_name = format!("{stem}_{mode_name}.{ext}");
    match out_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(file_name),
        _ => std::path::PathBuf::from(file_name),
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StrokeEdit {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diameter: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub angle_deg: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure_scale: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brush: Option<BrushKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct StrokeHit {
    pub index: usize,
    pub id: Option<String>,
    pub group: Option<String>,
    pub brush: BrushKind,
    pub bounds: [f32; 4],
    pub sample_count: usize,
}

impl Recording {
    pub fn ensure_stroke_ids(&mut self) -> Vec<String> {
        let mut assigned = Vec::new();
        let mut next_num = self.events.len();
        let mut used: std::collections::HashSet<String> = self
            .events
            .iter()
            .filter_map(|e| match e {
                RecordedEvent::Stroke { id: Some(id), .. } => Some(id.clone()),
                _ => None,
            })
            .collect();
        for (idx, ev) in self.events.iter_mut().enumerate() {
            if let RecordedEvent::Stroke { id, .. } = ev {
                if id.is_none() {
                    let mut cand = format!("s{idx}");
                    while used.contains(&cand) {
                        next_num += 1;
                        cand = format!("s{next_num}");
                    }
                    used.insert(cand.clone());
                    *id = Some(cand.clone());
                    assigned.push(cand);
                }
            }
        }
        assigned
    }

    pub fn stroke_bounds(event: &RecordedEvent) -> Option<[f32; 4]> {
        let RecordedEvent::Stroke {
            diameter, samples, ..
        } = event
        else {
            return None;
        };
        let r = diameter * 0.5;
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for s in samples {
            min_x = min_x.min(s.x - r);
            min_y = min_y.min(s.y - r);
            max_x = max_x.max(s.x + r);
            max_y = max_y.max(s.y + r);
        }
        if !min_x.is_finite() {
            return None;
        }
        Some([min_x, min_y, max_x, max_y])
    }

    pub fn strokes_at(&self, x: f32, y: f32, radius: f32) -> Vec<StrokeHit> {
        let mut hits = Vec::new();
        for (index, ev) in self.events.iter().enumerate() {
            let RecordedEvent::Stroke {
                id,
                group,
                brush,
                diameter,
                samples,
                ..
            } = ev
            else {
                continue;
            };
            let Some(bounds) = Self::stroke_bounds(ev) else {
                continue;
            };
            let reach = diameter * 0.5 + radius;
            let reach2 = reach * reach;
            let touched = samples.iter().any(|s| {
                let dx = s.x - x;
                let dy = s.y - y;
                dx * dx + dy * dy <= reach2
            });
            if touched {
                hits.push(StrokeHit {
                    index,
                    id: id.clone(),
                    group: group.clone(),
                    brush: *brush,
                    bounds,
                    sample_count: samples.len(),
                });
            }
        }
        hits
    }

    fn find_stroke_index(&self, target_id: &str) -> Result<usize, String> {
        self.events
            .iter()
            .position(
                |ev| matches!(ev, RecordedEvent::Stroke { id: Some(id), .. } if id == target_id),
            )
            .ok_or_else(|| format!("Stroke id '{target_id}' not found."))
    }

    pub fn move_stroke(&mut self, target_id: &str, dx: f32, dy: f32) -> Result<usize, String> {
        let idx = self.find_stroke_index(target_id)?;
        if let RecordedEvent::Stroke { samples, .. } = &mut self.events[idx] {
            for s in samples {
                s.x += dx;
                s.y += dy;
            }
        }
        Ok(idx)
    }

    pub fn edit_stroke(&mut self, target_id: &str, edit: &StrokeEdit) -> Result<usize, String> {
        let idx = self.find_stroke_index(target_id)?;
        if let RecordedEvent::Stroke {
            diameter,
            angle_deg,
            brush,
            group,
            samples,
            ..
        } = &mut self.events[idx]
        {
            if let Some(d) = edit.diameter {
                *diameter = d.max(1.0);
            }
            if let Some(a) = edit.angle_deg {
                *angle_deg = a;
            }
            if let Some(b) = edit.brush {
                *brush = b;
            }
            if let Some(ref g) = edit.group {
                *group = Some(g.clone());
            }
            if let Some(ps) = edit.pressure_scale {
                for s in samples {
                    s.pressure = (s.pressure * ps).clamp(0.0, 1.0);
                }
            }
        }
        Ok(idx)
    }

    pub fn delete_stroke(&mut self, target_id: &str) -> Result<usize, String> {
        let idx = self.find_stroke_index(target_id)?;
        self.events.remove(idx);
        Ok(idx)
    }

    pub fn reorder_stroke(
        &mut self,
        target_id: &str,
        before_id: Option<&str>,
    ) -> Result<usize, String> {
        let old_idx = self.find_stroke_index(target_id)?;
        let ev = self.events.remove(old_idx);
        let new_idx = match before_id {
            Some(bid) => self.find_stroke_index(bid).unwrap_or(self.events.len()),
            None => self.events.len(),
        };
        self.events.insert(new_idx, ev);
        Ok(old_idx.min(new_idx))
    }
}

pub fn render_batch(manifest_path: &Path) -> Result<Vec<BatchJobOutput>, String> {
    let text = std::fs::read_to_string(manifest_path).map_err(|e| e.to_string())?;
    let manifest: BatchManifest = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let manifest_dir = manifest_path.parent().filter(|p| !p.as_os_str().is_empty());

    let inst = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    let (device, queue, _) = pollster::block_on(gpu::device(&inst, None))?;
    let mut paint = PaintGpu::new(&device);
    let mut cache: std::collections::HashMap<std::path::PathBuf, Recording> =
        std::collections::HashMap::new();
    let mut results = Vec::with_capacity(manifest.jobs.len());

    for (index, job) in manifest.jobs.iter().enumerate() {
        let rec_raw = job
            .recording
            .as_deref()
            .or(manifest.shared_recording.as_deref())
            .ok_or_else(|| {
                format!(
                    "Batch job {index} is missing 'recording' and no 'shared_recording' was set."
                )
            })?;
        let rec_path = resolve_path(manifest_dir, rec_raw);
        let recording = if let Some(cached) = cache.get(&rec_path) {
            cached.clone()
        } else {
            let rec_text = std::fs::read_to_string(&rec_path)
                .map_err(|e| format!("Job {index} recording {}: {e}", rec_path.display()))?;
            let parsed = Recording::parse(&rec_text)
                .map_err(|e| format!("Job {index} recording {}: {e}", rec_path.display()))?;
            cache.insert(rec_path.clone(), parsed.clone());
            parsed
        };
        let rec_dir = rec_path.parent().filter(|p| !p.as_os_str().is_empty());
        let underlayer_path = job
            .underlayer
            .as_deref()
            .map(|p| resolve_path(manifest_dir, p));
        let out_path = resolve_path(manifest_dir, &job.out);
        if let Some(parent) = out_path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mode_str = job.mode.as_deref().unwrap_or("color");
        let mode = parse_export_mode(mode_str)?;
        paint.smooth_height = job.smooth_height;
        paint.gl_normals = matches!(job.normal_convention.as_deref(), Some("gl"));

        let events = render_one(
            &device,
            &queue,
            &mut paint,
            &recording,
            rec_dir,
            underlayer_path.as_deref(),
            &out_path,
            mode,
        )?;
        results.push(BatchJobOutput {
            index,
            out: out_path.display().to_string(),
            mode: mode_str.to_string(),
            events,
        });
    }
    Ok(results)
}
