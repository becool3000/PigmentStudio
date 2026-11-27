use crate::gpu::{self, Command, ExportKind, Lighting, PaintGpu, MAX_COMMANDS, SIDE};
use crate::recording::{
    compile_events_dims, equip_cabinet, format_mode_out_path, parse_export_modes,
    render_modes_on_gpu, BrushKind, RecordedEvent, Recording, StrokeEdit,
};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const CHECKPOINT_STRIDE: usize = 64;
const MAX_CHECKPOINTS: usize = 8;

struct Checkpoint {
    event_index: usize,
    tile_state: bool,
    compressed: Vec<u8>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ServeRequest {
    Ping,
    LoadRecording {
        #[serde(default)]
        path: Option<String>,
        #[serde(default)]
        recording: Option<Recording>,
    },
    SaveRecording {
        path: String,
    },
    Apply {
        events: Vec<RecordedEvent>,
    },
    MoveStroke {
        id: String,
        dx: f32,
        dy: f32,
    },
    EditStroke {
        id: String,
        #[serde(flatten)]
        edit: StrokeEdit,
    },
    DeleteStroke {
        id: String,
    },
    ReorderStroke {
        id: String,
        #[serde(default)]
        before_id: Option<String>,
    },
    StrokesAt {
        x: f32,
        y: f32,
        #[serde(default = "default_radius")]
        radius: f32,
    },
    InspectPixel {
        x: u32,
        y: u32,
    },
    InspectRegion {
        x: u32,
        y: u32,
        w: u32,
        h: u32,
    },
    BrushState {
        #[serde(default)]
        brush: Option<BrushKind>,
    },
    Render {
        out: String,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        smooth_height: bool,
        #[serde(default)]
        normal_convention: Option<String>,
    },
    SetCanvasSize {
        width: u32,
        height: u32,
    },
}

fn default_radius() -> f32 {
    8.0
}

#[derive(Debug, Serialize)]
pub struct ServeResponse {
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl ServeResponse {
    fn ok(data: serde_json::Value) -> Self {
        Self {
            ok: true,
            error: None,
            data: Some(data),
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            data: None,
        }
    }
}

pub struct ServeSession {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub paint: PaintGpu,
    pub recording: Recording,
    base_dir: Option<PathBuf>,
    tile_state: bool,
    applied_len: usize,
    checkpoints: Vec<Checkpoint>,
}

impl ServeSession {
    pub fn new() -> Result<Self, String> {
        let inst = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::DX12,
            ..Default::default()
        });
        let (device, queue, _) = pollster::block_on(gpu::device(&inst, None))?;
        let paint = PaintGpu::new(&device);
        let recording = Recording::default();
        let mut session = Self {
            device,
            queue,
            paint,
            recording,
            base_dir: None,
            tile_state: false,
            applied_len: 0,
            checkpoints: Vec::new(),
        };
        session.reset_gpu_to_start()?;
        Ok(session)
    }

    fn reset_gpu_to_start(&mut self) -> Result<(), String> {
        if let (Some(w), Some(h)) = (self.recording.canvas_width, self.recording.canvas_height) {
            self.paint.resize_canvas(gpu::CanvasDims::new(w, h));
        }
        self.paint.lighting = self
            .recording
            .lighting
            .clone()
            .map(Into::into)
            .unwrap_or(Lighting {
                enabled: false,
                ..Default::default()
            });
        self.tile_state = self.recording.tile.unwrap_or(false);
        self.paint.tile = self.tile_state;

        let mut commands = vec![Command::Initialize];
        equip_cabinet(&mut commands);
        if let Some(ref raw) = self.recording.underlayer_path {
            let resolved = crate::recording::resolve_path(self.base_dir.as_deref(), raw);
            let words = gpu::load_png_as_substrate(&resolved)?;
            commands.push(Command::ImportUnderlayer(words));
        }
        self.submit_commands(&commands);
        self.applied_len = 0;
        self.checkpoints.clear();
        self.save_checkpoint(0)?;
        Ok(())
    }

    fn submit_commands(&mut self, commands: &[Command]) {
        for chunk in commands.chunks(MAX_COMMANDS) {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            self.paint
                .encode(&self.device, &self.queue, &mut encoder, chunk);
            self.queue.submit([encoder.finish()]);
        }
        while self.paint.ink_is_wet() {
            let mut encoder = self.device.create_command_encoder(&Default::default());
            self.paint.encode(
                &self.device,
                &self.queue,
                &mut encoder,
                &[Command::AdvanceInk],
            );
            self.queue.submit([encoder.finish()]);
        }
    }

    fn save_checkpoint(&mut self, event_index: usize) -> Result<(), String> {
        let bytes = self
            .paint
            .readback_project_bytes(&self.device, &self.queue)?;
        let compressed = crate::project::rle_encode_u32(&bytes);
        if self.checkpoints.len() >= MAX_CHECKPOINTS && self.checkpoints.len() > 2 {
            // Keep index 0 (initial state) and drop the oldest middle checkpoint.
            self.checkpoints.remove(1);
        }
        self.checkpoints.push(Checkpoint {
            event_index,
            tile_state: self.tile_state,
            compressed,
        });
        Ok(())
    }

    fn replay_from(&mut self, dirty_from: usize) -> Result<(), String> {
        let ckpt_pos = self
            .checkpoints
            .iter()
            .rposition(|c| c.event_index <= dirty_from);
        if let Some(pos) = ckpt_pos {
            let ckpt_idx = self.checkpoints[pos].event_index;
            let tile = self.checkpoints[pos].tile_state;
            let raw = crate::project::rle_decode_u32(
                &self.checkpoints[pos].compressed,
                crate::gpu::PROJECT_BYTES as usize,
            )?;
            self.checkpoints.truncate(pos + 1);
            self.submit_commands(&[Command::LoadProject(Arc::new(raw))]);
            self.tile_state = tile;
            self.paint.tile = tile;
            self.applied_len = ckpt_idx;
        } else {
            self.reset_gpu_to_start()?;
        }
        self.advance_to_end()
    }

    fn advance_to_end(&mut self) -> Result<(), String> {
        let total = self.recording.events.len();
        while self.applied_len < total {
            let end = (self.applied_len + CHECKPOINT_STRIDE).min(total);
            let slice = &self.recording.events[self.applied_len..end];
            let cmds = compile_events_dims(
                slice,
                &mut self.tile_state,
                self.base_dir.as_deref(),
                self.paint.dims,
            )?;
            self.paint.tile = self.tile_state;
            self.submit_commands(&cmds);
            self.applied_len = end;
            if self.applied_len < total {
                self.save_checkpoint(self.applied_len)?;
            }
        }
        Ok(())
    }

    pub fn handle_request(&mut self, req: ServeRequest) -> ServeResponse {
        match self.handle_inner(req) {
            Ok(data) => ServeResponse::ok(data),
            Err(err) => ServeResponse::err(err),
        }
    }

    fn handle_inner(&mut self, req: ServeRequest) -> Result<serde_json::Value, String> {
        match req {
            ServeRequest::Ping => Ok(serde_json::json!({
                "version": env!("CARGO_PKG_VERSION"),
                "events": self.recording.events.len(),
                "checkpoints": self.checkpoints.len(),
            })),
            ServeRequest::LoadRecording { path, recording } => {
                if let Some(p) = path {
                    let pb = PathBuf::from(&p);
                    let text = std::fs::read_to_string(&pb).map_err(|e| e.to_string())?;
                    self.recording = Recording::parse(&text)?;
                    self.base_dir = pb
                        .parent()
                        .filter(|d| !d.as_os_str().is_empty())
                        .map(Path::to_path_buf);
                } else if let Some(rec) = recording {
                    self.recording = rec;
                    self.base_dir = None;
                } else {
                    return Err("load_recording requires 'path' or 'recording'.".into());
                }
                let assigned = self.recording.ensure_stroke_ids();
                self.reset_gpu_to_start()?;
                self.advance_to_end()?;
                Ok(serde_json::json!({
                    "events": self.recording.events.len(),
                    "assigned_ids": assigned,
                }))
            }
            ServeRequest::SaveRecording { path } => {
                let text =
                    serde_json::to_string_pretty(&self.recording).map_err(|e| e.to_string())?;
                std::fs::write(&path, text).map_err(|e| e.to_string())?;
                Ok(serde_json::json!({ "saved": path, "events": self.recording.events.len() }))
            }
            ServeRequest::Apply { events } => {
                let start = self.recording.events.len();
                self.recording.events.extend(events);
                let assigned = self.recording.ensure_stroke_ids();
                self.advance_to_end()?;
                Ok(serde_json::json!({
                    "applied_from": start,
                    "events": self.recording.events.len(),
                    "assigned_ids": assigned,
                }))
            }
            ServeRequest::MoveStroke { id, dx, dy } => {
                let idx = self.recording.move_stroke(&id, dx, dy)?;
                self.replay_from(idx)?;
                Ok(serde_json::json!({ "id": id, "replayed_from": idx }))
            }
            ServeRequest::EditStroke { id, edit } => {
                let idx = self.recording.edit_stroke(&id, &edit)?;
                self.replay_from(idx)?;
                Ok(serde_json::json!({ "id": id, "replayed_from": idx }))
            }
            ServeRequest::DeleteStroke { id } => {
                let idx = self.recording.delete_stroke(&id)?;
                self.replay_from(idx)?;
                Ok(
                    serde_json::json!({ "id": id, "replayed_from": idx, "events": self.recording.events.len() }),
                )
            }
            ServeRequest::ReorderStroke { id, before_id } => {
                let idx = self.recording.reorder_stroke(&id, before_id.as_deref())?;
                self.replay_from(idx)?;
                Ok(serde_json::json!({ "id": id, "replayed_from": idx }))
            }
            ServeRequest::StrokesAt { x, y, radius } => {
                let hits = self.recording.strokes_at(x, y, radius);
                Ok(serde_json::json!({ "hits": hits }))
            }
            ServeRequest::InspectPixel { x, y } => {
                let d = self
                    .paint
                    .sample_diagnostics(&self.device, &self.queue, x, y);
                Ok(serde_json::json!({
                    "x": x.min(SIDE - 1),
                    "y": y.min(SIDE - 1),
                    "srgb": d.display_srgb,
                    "wet_channels": d.wet_channels,
                    "dry_channels": d.dry_channels,
                    "wet_total": d.wet_total,
                    "dry_total": d.dry_total,
                    "hiding_ratio_pct": d.optical.wet_layer_hiding_ratio_pct,
                }))
            }
            ServeRequest::InspectRegion { x, y, w, h } => {
                let x0 = x.min(self.paint.dims.width - 1);
                let y0 = y.min(self.paint.dims.height - 1);
                let x1 = (x0 + w.max(1)).min(self.paint.dims.width);
                let y1 = (y0 + h.max(1)).min(self.paint.dims.height);
                let color_bytes =
                    self.paint
                        .readback_rgba_bytes(&self.device, &self.queue, ExportKind::Color)?;
                let transp_bytes = self.paint.readback_rgba_bytes(
                    &self.device,
                    &self.queue,
                    ExportKind::Transparent,
                )?;
                let height_bytes = self.paint.readback_rgba_bytes(
                    &self.device,
                    &self.queue,
                    ExportKind::Height,
                )?;
                let mut count = 0u64;
                let mut sum_rgb = [0u64; 3];
                let mut covered = 0u64;
                let mut sum_h = 0u64;
                let mut max_h = 0u8;
                for py in y0..y1 {
                    for px in x0..x1 {
                        let i = ((py * self.paint.dims.width + px) * 4) as usize;
                        sum_rgb[0] += u64::from(color_bytes[i]);
                        sum_rgb[1] += u64::from(color_bytes[i + 1]);
                        sum_rgb[2] += u64::from(color_bytes[i + 2]);
                        if transp_bytes[i + 3] > 0 {
                            covered += 1;
                        }
                        let hv = height_bytes[i];
                        sum_h += u64::from(hv);
                        max_h = max_h.max(hv);
                        count += 1;
                    }
                }
                let n = count.max(1) as f64;
                Ok(serde_json::json!({
                    "bounds": [x0, y0, x1 - x0, y1 - y0],
                    "mean_srgb": [sum_rgb[0] as f64 / n, sum_rgb[1] as f64 / n, sum_rgb[2] as f64 / n],
                    "coverage_pct": (covered as f64 / n) * 100.0,
                    "mean_height": sum_h as f64 / n,
                    "max_height": max_h,
                }))
            }
            ServeRequest::BrushState { brush } => {
                let d = self
                    .paint
                    .sample_diagnostics(&self.device, &self.queue, 0, 0);
                let target_idx = brush.map(|b| match b {
                    BrushKind::Round => 0,
                    BrushKind::Flat => 1,
                    BrushKind::Filbert => 2,
                    BrushKind::Fan => 3,
                    BrushKind::PaletteKnife => 4,
                    BrushKind::SoftMop => 5,
                });
                const NAMES: [&str; 6] = [
                    "Round",
                    "Flat",
                    "Filbert",
                    "Fan",
                    "Palette knife",
                    "Soft mop",
                ];
                let brushes: Vec<serde_json::Value> = d
                    .brush_loads
                    .iter()
                    .enumerate()
                    .filter(|&(idx, _)| target_idx.is_none_or(|t| idx == t))
                    .map(|(idx, ch)| {
                        let load: f32 = ch.iter().sum();
                        serde_json::json!({
                            "brush": NAMES.get(idx).copied().unwrap_or("Unknown"),
                            "load": load,
                            "channels": ch,
                            "srgb": crate::pigment::rgb(*ch),
                        })
                    })
                    .collect();
                Ok(serde_json::json!({ "brushes": brushes }))
            }
            ServeRequest::Render {
                out,
                mode,
                smooth_height,
                normal_convention,
            } => {
                let mode_str = mode.as_deref().unwrap_or("color");
                let modes = parse_export_modes(mode_str)?;
                let gl_normals = match normal_convention.as_deref().unwrap_or("dx") {
                    "dx" => false,
                    "gl" => true,
                    other => return Err(format!("Unknown normal_convention '{other}'.")),
                };
                self.paint.smooth_height = smooth_height;
                self.paint.gl_normals = gl_normals;
                let out_path = PathBuf::from(&out);
                let multi = modes.len() > 1;
                let mut files = Vec::with_capacity(modes.len());
                if out_path
                    .extension()
                    .and_then(|e| e.to_str())
                    .is_some_and(|e| e.eq_ignore_ascii_case("pigment"))
                {
                    let (_, out_list) = render_modes_on_gpu(
                        &self.device,
                        &self.queue,
                        &mut self.paint,
                        &self.recording,
                        self.base_dir.as_deref(),
                        None,
                        &out_path,
                        &modes,
                    )?;
                    for (m, p) in out_list {
                        files
                            .push(serde_json::json!({ "mode": m, "out": p.display().to_string() }));
                    }
                } else {
                    for (mode_name, kind) in modes {
                        let target = format_mode_out_path(&out_path, &mode_name, multi);
                        if let Some(parent) = target.parent().filter(|p| !p.as_os_str().is_empty())
                        {
                            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                        }
                        let mut encoder = self.device.create_command_encoder(&Default::default());
                        let buffer =
                            self.paint
                                .readback_kind(&self.device, &self.queue, &mut encoder, kind);
                        self.queue.submit([encoder.finish()]);
                        let rx = gpu::export_sized(
                            buffer,
                            self.paint.dims,
                            target.clone(),
                            kind.is_srgb(),
                        );
                        let _ = self.device.poll(wgpu::Maintain::Wait);
                        rx.recv().map_err(|e| e.to_string())??;
                        files.push(serde_json::json!({ "mode": mode_name, "out": target.display().to_string() }));
                    }
                }
                Ok(serde_json::json!({ "files": files }))
            }
            ServeRequest::SetCanvasSize { width, height } => {
                let dims = gpu::CanvasDims::new(width, height);
                self.recording.canvas_width = Some(dims.width);
                self.recording.canvas_height = Some(dims.height);
                self.paint.resize_canvas(dims);
                self.checkpoints.clear();
                self.applied_len = 0;
                self.reset_gpu_to_start()?;
                Ok(serde_json::json!({ "width": dims.width, "height": dims.height }))
            }
        }
    }
}

pub fn run_stdio() -> Result<(), String> {
    let mut session = ServeSession::new()?;
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = std::io::BufWriter::new(stdout.lock());
    for line in stdin.lock().lines() {
        let line = line.map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<ServeRequest>(trimmed) {
            Ok(req) => session.handle_request(req),
            Err(err) => ServeResponse::err(format!("Invalid request JSON: {err}")),
        };
        let json = serde_json::to_string(&resp).map_err(|e| e.to_string())?;
        writeln!(out, "{json}").map_err(|e| e.to_string())?;
        out.flush().map_err(|e| e.to_string())?;
    }
    Ok(())
}
