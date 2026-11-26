use crate::gpu::{
    self, Brush, BrushService, Command, ExportKind, Lighting, PaintGpu, Surface, MAX_COMMANDS,
};
use crate::stroke::{Sample, Source, Stroke};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrushKind {
    Round,
    Flat,
}

impl From<Brush> for BrushKind {
    fn from(b: Brush) -> Self {
        match b {
            Brush::Round => Self::Round,
            Brush::Flat => Self::Flat,
        }
    }
}

impl From<BrushKind> for Brush {
    fn from(b: BrushKind) -> Self {
        match b {
            BrushKind::Round => Self::Round,
            BrushKind::Flat => Self::Flat,
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
    Load { tube: usize },
    Dip { tube: usize },
    DipCustom { slot: usize },
    DipRack { slot: usize },
    Wipe { fraction: f32 },
}

impl From<BrushService> for ServiceAction {
    fn from(s: BrushService) -> Self {
        match s {
            BrushService::Load(tube) => Self::Load { tube },
            BrushService::Dip(tube) => Self::Dip { tube },
            BrushService::DipCustom(slot) => Self::DipCustom { slot },
            BrushService::DipRack(slot) => Self::DipRack { slot },
            BrushService::Wipe(fraction) => Self::Wipe { fraction },
        }
    }
}

impl From<ServiceAction> for BrushService {
    fn from(s: ServiceAction) -> Self {
        match s {
            ServiceAction::Load { tube } => Self::Load(tube),
            ServiceAction::Dip { tube } => Self::Dip(tube),
            ServiceAction::DipCustom { slot } => Self::DipCustom(slot),
            ServiceAction::DipRack { slot } => Self::DipRack(slot),
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RecordedEvent {
    Service {
        brush: BrushKind,
        action: ServiceAction,
    },
    EquipRack {
        slot: usize,
        recipe: [f32; 8],
    },
    Stroke {
        surface: SurfaceKind,
        brush: BrushKind,
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
        tile: bool,
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
    pub lighting: Option<LightingConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tile: Option<bool>,
    pub events: Vec<RecordedEvent>,
}

impl Default for Recording {
    fn default() -> Self {
        Self {
            format: "pigment-studio-recording-v1".into(),
            lighting: None,
            tile: None,
            events: Vec::new(),
        }
    }
}

pub fn compile_events(events: &[RecordedEvent], tile_state: &mut bool) -> Vec<Command> {
    let mut commands = Vec::new();
    for event in events {
        match event {
            RecordedEvent::Service { brush, action } => {
                commands.push(Command::Service((*brush).into(), (*action).into()));
            }
            RecordedEvent::EquipRack { slot, recipe } => {
                commands.push(Command::EquipRack(*slot, *recipe));
            }
            RecordedEvent::Stroke {
                surface,
                brush,
                ink,
                charcoal,
                erase,
                diameter,
                angle_deg,
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
                };
                let mut stroke = if *charcoal && *surface == SurfaceKind::Canvas {
                    Stroke::start_charcoal(
                        Source::Mouse,
                        to_sample(first),
                        *diameter,
                        *erase,
                        *tile,
                        &mut commands,
                    )
                } else if *ink && *surface == SurfaceKind::Canvas {
                    Stroke::start_ink_tiled(Source::Mouse, to_sample(first), *tile, &mut commands)
                } else {
                    Stroke::start_tiled(
                        Source::Mouse,
                        to_sample(first),
                        *diameter,
                        (*surface).into(),
                        (*brush).into(),
                        angle_deg.to_radians(),
                        *tile,
                        &mut commands,
                    )
                };
                for s in rest {
                    stroke.extend(to_sample(s), &mut commands);
                }
                stroke.finish(&mut commands);
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
            RecordedEvent::ImportReference { words } => {
                commands.push(Command::ImportReference(words.clone()));
            }
        }
    }
    commands
}

pub fn equip_cabinet(commands: &mut Vec<Command>) {
    for (slot, p) in crate::cabinet::PIGMENTS.iter().enumerate() {
        commands.push(Command::EquipRack(slot, p.recipe));
    }
}

pub fn render_headless(
    recording_path: &Path,
    out_path: &Path,
    mode: ExportKind,
) -> Result<usize, String> {
    let text = std::fs::read_to_string(recording_path).map_err(|e| e.to_string())?;
    let recording: Recording = serde_json::from_str(&text).map_err(|e| e.to_string())?;

    let inst = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    });
    let (device, queue, _) = pollster::block_on(gpu::device(&inst, None))?;
    let mut paint = PaintGpu::new(&device);
    if let Some(lighting) = recording.lighting.clone() {
        paint.lighting = lighting.into();
    }
    if let Some(tile) = recording.tile {
        paint.tile = tile;
    }

    let mut commands = vec![Command::Initialize];
    equip_cabinet(&mut commands);
    let mut tile_state = paint.tile;
    commands.extend(compile_events(&recording.events, &mut tile_state));
    paint.tile = tile_state;

    for chunk in commands.chunks(MAX_COMMANDS) {
        let mut encoder = device.create_command_encoder(&Default::default());
        paint.encode(&device, &queue, &mut encoder, chunk);
        queue.submit([encoder.finish()]);
    }
    while paint.ink_is_wet() {
        let mut encoder = device.create_command_encoder(&Default::default());
        paint.encode(&device, &queue, &mut encoder, &[Command::AdvanceInk]);
        queue.submit([encoder.finish()]);
    }

    let mut encoder = device.create_command_encoder(&Default::default());
    let buffer = paint.readback_kind(&device, &queue, &mut encoder, mode);
    queue.submit([encoder.finish()]);
    let rx = gpu::export_with_srgb(buffer, out_path.to_path_buf(), mode.is_srgb());
    let _ = device.poll(wgpu::Maintain::Wait);
    rx.recv().map_err(|e| e.to_string())??;
    Ok(recording.events.len())
}
