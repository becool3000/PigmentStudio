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
            BrushService::DipRack(slot) => Self::DipRack { slot, lane: None },
            BrushService::DipCustomLane(slot, lane) => Self::DipCustom {
                slot,
                lane: Some(lane),
            },
            BrushService::DipRackLane(slot, lane) => Self::DipRack {
                slot,
                lane: Some(lane),
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
            ServiceAction::DipRack { slot, lane: None } => Self::DipRack(slot),
            ServiceAction::DipCustom {
                slot,
                lane: Some(lane),
            } => Self::DipCustomLane(slot, lane),
            ServiceAction::DipRack {
                slot,
                lane: Some(lane),
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
    Stroke {
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
                commands.push(Command::Service(reservoir, service));
            }
            RecordedEvent::EquipRack { slot, recipe } => {
                commands.push(Command::EquipRack(*slot, *recipe));
            }
            RecordedEvent::Stroke {
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
                    Stroke::start_charcoal(
                        Source::Mouse,
                        to_sample(first),
                        *diameter,
                        *erase,
                        stroke_tile,
                        &mut commands,
                    )
                } else if *ink && *surface == SurfaceKind::Canvas {
                    Stroke::start_ink_tiled(
                        Source::Mouse,
                        to_sample(first),
                        stroke_tile,
                        &mut commands,
                    )
                } else {
                    Stroke::start_full(
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
                            || (*brush == BrushKind::Round && *round_pressure),
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
        }
    }
    Ok(commands)
}

pub fn equip_cabinet(commands: &mut Vec<Command>) {
    for (slot, p) in crate::cabinet::PIGMENTS.iter().enumerate() {
        commands.push(Command::EquipRack(slot, p.recipe));
    }
}

#[cfg(test)]
pub fn render_headless(
    recording_path: &Path,
    out_path: &Path,
    mode: ExportKind,
    underlayer_override: Option<&Path>,
) -> Result<usize, String> {
    render_headless_with_options(recording_path, out_path, mode, underlayer_override, false)
}

pub fn render_headless_with_options(
    recording_path: &Path,
    out_path: &Path,
    mode: ExportKind,
    underlayer_override: Option<&Path>,
    smooth_height: bool,
) -> Result<usize, String> {
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
    render_one(
        &device,
        &queue,
        &mut paint,
        &recording,
        base_dir,
        underlayer_override,
        out_path,
        mode,
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
        let words = gpu::load_png_as_substrate(ul_path)?;
        commands.push(Command::ImportUnderlayer(words));
    } else if let Some(ref raw) = recording.underlayer_path {
        let resolved = resolve_path(base_dir, raw);
        let words = gpu::load_png_as_substrate(&resolved)?;
        commands.push(Command::ImportUnderlayer(words));
    }

    let mut tile_state = paint.tile;
    commands.extend(compile_events_in(
        &recording.events,
        &mut tile_state,
        base_dir,
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

    let mut encoder = device.create_command_encoder(&Default::default());
    if out_path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("pigment"))
    {
        let buffer = paint.readback_project(device, &mut encoder);
        queue.submit([encoder.finish()]);
        let meta = crate::project::ProjectMeta {
            calibration_version: crate::project::CURRENT_CALIBRATION_VERSION,
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
    } else {
        let buffer = paint.readback_kind(device, queue, &mut encoder, mode);
        queue.submit([encoder.finish()]);
        let rx = gpu::export_with_srgb(buffer, out_path.to_path_buf(), mode.is_srgb());
        let _ = device.poll(wgpu::Maintain::Wait);
        rx.recv().map_err(|e| e.to_string())??;
    }
    Ok(recording.events.len())
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
}

#[derive(Clone, Debug, Serialize)]
pub struct BatchJobOutput {
    pub index: usize,
    pub out: String,
    pub mode: String,
    pub events: usize,
}

pub fn parse_export_mode(mode_str: &str) -> Result<ExportKind, String> {
    match mode_str {
        "color" => Ok(ExportKind::Color),
        "transparent" => Ok(ExportKind::Transparent),
        "height" => Ok(ExportKind::Height),
        "normal" => Ok(ExportKind::Normal),
        other => Err(format!(
            "Unknown mode '{other}'. Expected color, transparent, height, or normal."
        )),
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
