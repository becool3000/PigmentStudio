#![cfg_attr(not(test), windows_subsystem = "windows")]

mod audio;
mod benchmark;
mod cabinet;
mod gpu;
mod notes;
mod pen;
mod pigment;
mod stroke;
mod workbench;

#[cfg(test)]
mod tests;

use egui::{Color32, Pos2, Rect};
use gpu::{Brush, BrushService, Command, Dab, PaintGpu, Surface, MAX_COMMANDS, SIDE};
use pen::{PenInput, Phase};
use std::{
    path::PathBuf,
    sync::{mpsc::Receiver, Arc},
    time::{Duration, Instant},
};
use stroke::{Sample, Source, Stroke};
use winit::{
    application::ApplicationHandler,
    dpi::{LogicalSize, PhysicalPosition},
    event::{ElementState, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

type ExportResult = Receiver<Result<PathBuf, String>>;

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let is_cli = args.iter().any(|arg| {
        arg == "--benchmark"
            || arg == "--notes"
            || arg == "--note-done"
            || arg == "--inspect-pixel"
            || arg == "--clear-done-notes"
    });
    #[cfg(windows)]
    if is_cli {
        unsafe {
            windows_sys::Win32::System::Console::AttachConsole(
                windows_sys::Win32::System::Console::ATTACH_PARENT_PROCESS,
            );
        }
    }

    if args.iter().any(|arg| arg == "--notes") {
        let notes_list = notes::read(&notes::default_path());
        println!(
            "{}",
            serde_json::to_string_pretty(&notes_list).unwrap_or_default()
        );
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(pos) = args.iter().position(|arg| arg == "--note-done") {
        if let Some(id_str) = args.get(pos + 1) {
            if let Ok(id) = id_str.parse::<u32>() {
                if notes::set_done(&notes::default_path(), id, true) {
                    println!("Marked note #{} as done.", id);
                    return std::process::ExitCode::SUCCESS;
                } else {
                    eprintln!("Note #{} not found.", id);
                    return std::process::ExitCode::FAILURE;
                }
            }
        }
    }

    if args.iter().any(|arg| arg == "--clear-done-notes") {
        let count = notes::clear_done(&notes::default_path());
        println!("Cleared {} done note(s).", count);
        return std::process::ExitCode::SUCCESS;
    }

    if let Some(pos) = args.iter().position(|arg| arg == "--inspect-pixel") {
        if let (Some(xs), Some(ys)) = (args.get(pos + 1), args.get(pos + 2)) {
            if let (Ok(x), Ok(y)) = (xs.parse::<u32>(), ys.parse::<u32>()) {
                let inst = instance();
                if let Ok((device, queue, _)) = pollster::block_on(gpu::device(&inst, None)) {
                    let paint = PaintGpu::new(&device);
                    let diag = paint.sample_diagnostics(&device, &queue, x, y);
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&diag).unwrap_or_default()
                    );
                    return std::process::ExitCode::SUCCESS;
                }
            }
        }
    }

    let benchmark = args.iter().any(|argument| argument == "--benchmark");
    if let Err(error) = run(benchmark) {
        eprintln!("{error}");
        if !benchmark {
            rfd::MessageDialog::new()
                .set_title("Pigment Studio")
                .set_description(error)
                .set_level(rfd::MessageLevel::Error)
                .show();
        }
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

fn run(benchmark: bool) -> Result<(), String> {
    let event_loop = EventLoop::new().map_err(|e| e.to_string())?;
    let mut app = App {
        benchmark: benchmark.then(benchmark::Benchmark::default),
        ..Default::default()
    };
    event_loop.run_app(&mut app).map_err(|e| e.to_string())?;
    app.error.map_or(Ok(()), Err)
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::DX12,
        ..Default::default()
    })
}

#[derive(Default)]
struct App {
    studio: Option<Studio>,
    error: Option<String>,
    benchmark: Option<benchmark::Benchmark>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.studio.is_some() {
            return;
        }
        let result = event_loop
            .create_window(
                Window::default_attributes()
                    .with_title("Pigment Studio")
                    .with_inner_size(LogicalSize::new(1160, 840))
                    .with_min_inner_size(LogicalSize::new(520, 400))
                    .with_visible(false),
            )
            .map_err(|e| e.to_string())
            .and_then(|window| pollster::block_on(Studio::new(Arc::new(window))));
        match result {
            Ok(studio) => {
                studio.window.set_visible(true);
                studio.window.request_redraw();
                self.studio = Some(studio);
            }
            Err(error) => {
                self.error = Some(error);
                event_loop.exit();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let Some(studio) = &mut self.studio else {
            return;
        };
        if studio.window.id() != id {
            return;
        }
        let mut redraw = studio.drain_pen();
        let response = studio.egui_state.on_window_event(&studio.window, &event);
        redraw |= response.repaint;
        if matches!(
            &event,
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Released,
                ..
            } | WindowEvent::Touch(Touch {
                phase: TouchPhase::Ended,
                ..
            })
        ) {
            // Egui activates the widget on its next layout. A pen down that
            // follows the click must see the new brush and its queued paint.
            studio.pen_waits_for_ui = true;
        }
        match event {
            WindowEvent::CloseRequested => {
                if studio.exporting.is_none() && studio.export_path.is_none() {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                // The old physical hit region is invalid until layout runs again.
                studio.canvas = None;
                studio.viewport = None;
                studio.pan_drag = None;
                studio.palette = None;
                studio.rag = None;
                studio.pen.surfaces([None; 3], None);
                studio.reset_pen();
                studio.end_stroke();
                if size.width != 0 && size.height != 0 {
                    studio.surface_config.width = size.width;
                    studio.surface_config.height = size.height;
                    studio
                        .surface
                        .configure(&studio.device, &studio.surface_config);
                    studio.drawable = true;
                } else {
                    studio.drawable = false;
                }
                redraw = true;
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                studio.canvas = None;
                studio.viewport = None;
                studio.pan_drag = None;
                studio.palette = None;
                studio.rag = None;
                studio.pen.surfaces([None; 3], None);
                studio.reset_pen();
                studio.end_stroke();
                redraw = true;
            }
            WindowEvent::Focused(false) => {
                studio.pan_drag = None;
                studio.reset_pen();
                studio.end_stroke();
                studio.cursor = None;
            }
            WindowEvent::CursorMoved { position, .. } => {
                studio.cursor = Some(egui::pos2(position.x as f32, position.y as f32));
                if let Some(previous) = studio.pan_drag {
                    let current = egui::pos2(position.x as f32, position.y as f32);
                    studio.pan_drag = Some(current);
                    studio.pan_by(current - previous);
                    redraw = true;
                } else {
                    studio.mouse_move(position);
                }
            }
            WindowEvent::CursorLeft { .. } => {
                studio.pan_drag = None;
                if studio
                    .rag_gesture
                    .is_some_and(|(source, _)| source == Source::Mouse)
                {
                    studio.rag_gesture = None;
                }
                studio.cursor = None;
                if studio
                    .stroke
                    .as_ref()
                    .is_some_and(|s| s.source == Source::Mouse)
                {
                    studio.end_stroke();
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let steps = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y,
                    MouseScrollDelta::PixelDelta(position) => (position.y / 60.) as f32,
                };
                if let Some(cursor) = studio.cursor.filter(|point| studio.over_viewport(*point)) {
                    studio.zoom_at(cursor, steps);
                    redraw = true;
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Right,
                ..
            } => {
                if state == ElementState::Pressed {
                    if let Some(cursor) = studio.cursor.filter(|point| studio.over_viewport(*point))
                    {
                        studio.end_stroke();
                        studio.pan_drag = Some(cursor);
                    }
                } else {
                    studio.pan_drag = None;
                }
                redraw = true;
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                if state == ElementState::Released {
                    if let Some(position) = studio.cursor {
                        studio.wipe_gesture(Source::Mouse, position);
                    }
                    if studio
                        .rag_gesture
                        .is_some_and(|(source, _)| source == Source::Mouse)
                    {
                        studio.rag_gesture = None;
                    }
                }
                if state == ElementState::Pressed {
                    if let Some(pos) = studio.cursor {
                        studio.start_stroke(
                            Source::Mouse,
                            pos,
                            studio.mouse_pressure(),
                            studio.elapsed(),
                        );
                    }
                } else if studio
                    .stroke
                    .as_ref()
                    .is_some_and(|s| s.source == Source::Mouse)
                {
                    studio.end_stroke();
                }
            }
            WindowEvent::RedrawRequested => {
                let painting = studio
                    .commands
                    .iter()
                    .any(|command| matches!(command, Command::Dab(_)));
                let measure = painting && self.benchmark.is_some();
                match studio.redraw(measure) {
                    Ok(Some(timing)) => {
                        if let Some(benchmark) = self.benchmark.as_mut() {
                            benchmark.frame(timing);
                        }
                    }
                    Err(error) => {
                        self.error = Some(error);
                        event_loop.exit();
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        if redraw || !studio.commands.is_empty() {
            studio.window.request_redraw();
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let Some(studio) = &mut self.studio else {
            return;
        };
        if studio.drain_pen() {
            studio.window.request_redraw();
        }
        if studio.exporting.is_some() {
            let _ = studio.device.poll(wgpu::Maintain::Poll);
            if let Some(result) =
                studio
                    .exporting
                    .as_ref()
                    .and_then(|pending| match pending.try_recv() {
                        Ok(result) => Some(result),
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                            Some(Err("The export worker stopped.".into()))
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => None,
                    })
            {
                studio.exporting = None;
                studio.status = match result {
                    Ok(path) => format!("Exported {}", path.display()),
                    Err(error) => {
                        format!("Export failed. Check the destination and try again. {error}")
                    }
                };
                studio.window.request_redraw();
            }
        }
        if studio.placement_result.is_some() {
            let _ = studio.device.poll(wgpu::Maintain::Poll);
            if let Some(result) = studio
                .placement_result
                .as_ref()
                .and_then(|pending| match pending.try_recv() {
                    Ok(placed) => Some(placed),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(None),
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                })
            {
                studio.placement_result = None;
                studio.placement_pending = false;
                match result {
                    Some(true) => {
                        studio.end_stroke();
                        studio.snapshot();
                        studio.commands.push(Command::Tubes);
                    }
                    Some(false) => {
                        studio.workbench_status =
                            "There are not enough adjacent spots. Move paint away or clear the palette.".into()
                    }
                    None => {
                        studio.workbench_status = "Could not check palette space. Try again.".into()
                    }
                }
                studio.window.request_redraw();
            }
        }
        if let (Some(benchmark), Some(rect)) = (self.benchmark.as_mut(), studio.canvas) {
            if !studio.benchmark_seeded {
                studio.commands.extend([
                    Command::SeedBenchmark,
                    Command::Service(Brush::Round, BrushService::Load(0)),
                ]);
                studio.benchmark_seeded = true;
                studio.window.request_redraw();
                return;
            }
            let now = Instant::now();
            studio.workbench.sizes[0] = 64.;
            for (i, point, pressure, time) in benchmark.ready_samples(now) {
                let physical = rect.min + point.to_vec2() * rect.width();
                if i == 0 {
                    studio.start_stroke(Source::Pen, physical, pressure, time);
                } else {
                    studio.move_stroke(Source::Pen, physical, pressure, time);
                }
            }
            if benchmark.finished(now) {
                studio.end_stroke();
                if studio.commands.is_empty() {
                    self.error = benchmark.report().err();
                    event_loop.exit();
                    return;
                }
            }
            if !studio.commands.is_empty() {
                studio.window.request_redraw();
            }
            let deadline = benchmark
                .next_deadline()
                .max(now + Duration::from_millis(1));
            event_loop.set_control_flow(ControlFlow::WaitUntil(deadline));
            return;
        }
        let ink_held = studio.stroke.as_ref().is_some_and(Stroke::ink_is_held);
        let ink_wet = studio.paint.ink_is_wet();
        let ink_animating = studio.drawable && (ink_held || ink_wet);
        let now = Instant::now();
        if ink_animating && now >= studio.next_ink_step {
            studio.next_ink_step = now + Duration::from_millis(16);
            if let Some(stroke) = studio.stroke.as_mut().filter(|_| ink_held) {
                stroke.dwell(now, &mut studio.commands);
            }
            if ink_wet {
                studio.commands.push(Command::AdvanceInk);
            }
            if !studio.commands.is_empty() {
                studio.window.request_redraw();
            }
        }
        let polling = studio.exporting.is_some()
            || studio.placement_result.is_some()
            || ink_animating
            || studio.stroke.is_some()
            || !studio.commands.is_empty();
        event_loop.set_control_flow(if polling {
            ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(8))
        } else {
            ControlFlow::Wait
        });
    }
}

struct Studio {
    // The subclass is removed before this type releases its window handles.
    pen: PenInput,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    surface_config: wgpu::SurfaceConfiguration,
    device: wgpu::Device,
    queue: wgpu::Queue,
    paint: PaintGpu,
    renderer: egui_wgpu::Renderer,
    egui_state: egui_winit::State,
    texture_id: egui::TextureId,
    drawable: bool,
    canvas: Option<Rect>,
    viewport: Option<Rect>,
    view: CanvasView,
    pan_drag: Option<Pos2>,
    cursor: Option<Pos2>,
    stroke: Option<Stroke>,
    palette: Option<Rect>,
    palette_clip: Option<Rect>,
    overlay: Option<Rect>,
    rag: Option<Rect>,
    rag_gesture: Option<(Source, Pos2)>,
    pen_waits_for_ui: bool,
    selected_dossier: Option<&'static cabinet::PigmentDef>,
    brush: Brush,
    workbench: workbench::Workbench,
    placement_pending: bool,
    placement_result: Option<Receiver<Option<bool>>>,
    workbench_status: String,
    benchmark_seeded: bool,
    ink_selected: bool,
    next_ink_step: Instant,

    notes: Vec<notes::Note>,
    note_tool: bool,
    notes_manager_open: bool,
    active_note_at: Option<Pos2>,
    note_input_text: String,
    selected_note: Option<u32>,

    pressure: f32,
    commands: Vec<Command>,
    export_path: Option<PathBuf>,
    exporting: Option<ExportResult>,
    status: String,
    started: Instant,
}

impl Studio {
    async fn new(window: Arc<Window>) -> Result<Self, String> {
        let instance = instance();
        let surface = instance.create_surface(window.clone()).map_err(|e| {
            format!("Could not open the GPU canvas. Check your graphics driver. {e}")
        })?;
        let (device, queue, adapter) = gpu::device(&instance, Some(&surface)).await?;
        let size = window.inner_size();
        let mut surface_config = surface
            .get_default_config(&adapter, size.width.max(1), size.height.max(1))
            .ok_or("This Direct3D 12 adapter cannot present to the app window.")?;
        let capabilities = surface.get_capabilities(&adapter);
        if let Some(format) = capabilities.formats.iter().find(|format| !format.is_srgb()) {
            surface_config.format = *format;
        }
        // Mailbox presents the latest completed stroke without building a
        // FIFO backlog when pen samples arrive faster than screen refresh.
        if capabilities
            .present_modes
            .contains(&wgpu::PresentMode::Mailbox)
        {
            surface_config.present_mode = wgpu::PresentMode::Mailbox;
        }
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        surface.configure(&device, &surface_config);
        let paint = PaintGpu::new(&device);
        let mut renderer = egui_wgpu::Renderer::new(&device, surface_config.format, None, 1, false);
        let texture_id =
            renderer.register_native_texture(&device, &paint.view(), wgpu::FilterMode::Linear);
        if let Some(error) = device.pop_error_scope().await {
            return Err(format!("The GPU could not initialize the paint shaders. Update your graphics driver and restart.\n{error}"));
        }
        let ctx = egui::Context::default();
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = Color32::from_rgb(30, 32, 35);
        visuals.selection.bg_fill = Color32::from_rgb(46, 86, 81);
        ctx.set_visuals(visuals);
        let egui_state = egui_winit::State::new(
            ctx,
            egui::ViewportId::ROOT,
            &*window,
            Some(window.scale_factor() as f32),
            window.theme(),
            Some(device.limits().max_texture_dimension_2d as usize),
        );
        let pen = PenInput::attach(&window)?;
        let mut commands = vec![Command::Initialize];
        for (slot, p) in cabinet::PIGMENTS.iter().enumerate() {
            commands.push(Command::EquipRack(slot, p.recipe));
        }
        let mut paint = paint;
        let workbench = workbench::Workbench::default();
        paint.lighting = workbench.lighting;
        Ok(Self {
            pen,
            window,
            surface,
            surface_config,
            device,
            queue,
            paint,
            renderer,
            egui_state,
            texture_id,
            selected_dossier: None,
            drawable: true,
            canvas: None,
            viewport: None,
            view: CanvasView::default(),
            pan_drag: None,
            cursor: None,
            stroke: None,
            palette: None,
            palette_clip: None,
            overlay: None,
            rag: None,
            rag_gesture: None,
            pen_waits_for_ui: false,
            brush: Brush::Round,
            workbench,
            placement_pending: false,
            placement_result: None,
            workbench_status: String::new(),
            benchmark_seeded: false,
            ink_selected: false,
            next_ink_step: Instant::now(),
            notes: notes::read(&notes::default_path()),
            note_tool: false,
            notes_manager_open: false,
            active_note_at: None,
            note_input_text: String::new(),
            selected_note: None,
            pressure: 0.,
            commands,
            export_path: None,
            exporting: None,
            status: String::new(),
            started: Instant::now(),
        })
    }

    fn elapsed(&self) -> u32 {
        self.started.elapsed().as_millis() as u32
    }

    fn over_viewport(&self, point: Pos2) -> bool {
        self.viewport.is_some_and(|rect| rect.contains(point))
            && !self.overlay.is_some_and(|rect| rect.contains(point))
    }

    fn update_canvas(&mut self) {
        self.canvas = self.viewport.map(|viewport| self.view.rect(viewport));
        self.update_pen_regions();
    }

    fn update_pen_regions(&self) {
        let visible_canvas = self
            .canvas
            .zip(self.viewport)
            .map(|(canvas, viewport)| canvas.intersect(viewport))
            .filter(|rect| rect.is_positive());
        self.pen
            .surfaces([visible_canvas, self.palette_clip, self.rag], self.overlay);
    }

    fn pan_by(&mut self, delta: egui::Vec2) {
        if let Some(viewport) = self.viewport {
            self.view.pan_by(viewport, delta);
            self.update_canvas();
        }
    }

    fn zoom_at(&mut self, cursor: Pos2, steps: f32) {
        if steps == 0. || self.pan_drag.is_some() {
            return;
        }
        if let Some(viewport) = self.viewport {
            self.end_stroke();
            self.view.zoom_at(viewport, cursor, steps);
            self.update_canvas();
        }
    }

    fn to_surface(&self, surface: Surface, physical: Pos2) -> Option<Pos2> {
        if self.overlay.is_some_and(|rect| rect.contains(physical)) {
            return None;
        }
        let rect = match surface {
            Surface::Canvas => self.canvas.filter(|_| self.over_viewport(physical))?,
            Surface::Palette => self.palette.filter(|_| {
                self.palette_clip
                    .is_some_and(|clip| clip.contains(physical))
            })?,
        };
        if !rect.contains(physical) {
            return None;
        }
        let [_, _, w, h] = surface.bounds();
        let delta = physical - rect.min;
        Some(egui::pos2(
            (delta.x / rect.width() * w as f32).clamp(0., w as f32 - 0.001),
            (delta.y / rect.height() * h as f32).clamp(0., h as f32 - 0.001),
        ))
    }

    fn snapshot(&mut self) {
        self.workbench.snapshot();
        self.commands.push(Command::Snapshot);
    }

    fn place_tubes(&mut self, tubes: [u32; 4]) {
        if self.placement_pending {
            return;
        }
        self.end_stroke();
        self.placement_pending = true;
        self.workbench_status.clear();
        self.commands.push(Command::CheckTubes(tubes));
    }

    #[allow(dead_code)]
    fn dip_brush(&mut self, tube: usize) {
        self.end_stroke();
        self.snapshot();
        let load = &mut self.workbench.session.loads[self.brush.index()];
        *load = (*load + 0.25).min(1.0);
        self.commands
            .push(Command::Service(self.brush, BrushService::Dip(tube)));
    }

    fn dip_from_jar(&mut self, service: BrushService) {
        self.end_stroke();
        self.snapshot();
        let load = &mut self.workbench.session.loads[self.brush.index()];
        *load = (*load + 0.25).min(1.0);
        self.commands.push(Command::Service(self.brush, service));
        self.ink_selected = false;
        self.workbench.sampling = false;
    }

    fn wipe(&mut self) {
        let current = self.workbench.session.loads[self.brush.index()];
        if current <= 0.0001 {
            return;
        }
        let target = (current - 0.10).max(0.0);
        let scale = target / current;
        self.end_stroke();
        self.snapshot();
        self.workbench.session.loads[self.brush.index()] = target;
        self.commands
            .push(Command::Service(self.brush, BrushService::Wipe(scale)));
    }

    fn clean_brush(&mut self) {
        if self.workbench.session.loads[self.brush.index()] <= 0.0001 {
            return;
        }
        self.end_stroke();
        self.snapshot();
        self.workbench.session.loads[self.brush.index()] = 0.0;
        self.commands
            .push(Command::Service(self.brush, BrushService::Wipe(0.0)));
    }

    fn start_stroke(&mut self, source: Source, position: Pos2, pressure: f32, timestamp_ms: u32) {
        if pressure <= 0.0 {
            return;
        }
        if self.pan_drag.is_some()
            || self.placement_pending
            || self.stroke.is_some()
            || self.rag_gesture.is_some()
        {
            return;
        }
        if self.rag.is_some_and(|rect| rect.contains(position)) {
            self.rag_gesture = Some((source, position));
            return;
        }
        let (surface, point) = if let Some(point) = self.to_surface(Surface::Palette, position) {
            if self.placement_pending {
                return;
            }
            if self.workbench.sampling {
                self.workbench.sampling = false;
                if let Some(slot) = self
                    .workbench
                    .session
                    .jars
                    .iter()
                    .position(|occupied| !*occupied)
                {
                    self.snapshot();
                    self.workbench.session.jars[slot] = true;
                    self.commands.push(Command::SaveSwatch(
                        slot,
                        Surface::Palette,
                        [point.x as u32, point.y as u32],
                    ));
                    self.workbench_status =
                        format!("Filled Jar {} with palette mixture.", slot + 1);
                    audio::play(audio::Sound::JarClack);
                } else {
                    self.workbench_status =
                        "Custom jar shelf full (64 jars). Empty a jar to make room.".into();
                }
                return;
            }
            if self.ink_selected {
                return;
            }
            (Surface::Palette, point)
        } else if let Some(point) = self.to_surface(Surface::Canvas, position) {
            if self.active_note_at.is_some() {
                if self.note_input_text.trim().is_empty() {
                    self.active_note_at = Some(point);
                }
                return;
            }
            if self.note_tool {
                self.active_note_at = Some(point);
                self.selected_note = None;
                self.note_input_text.clear();
                return;
            }
            if self.workbench.sampling {
                self.workbench.sampling = false;
                if let Some(slot) = self
                    .workbench
                    .session
                    .jars
                    .iter()
                    .position(|occupied| !*occupied)
                {
                    self.snapshot();
                    self.workbench.session.jars[slot] = true;
                    self.commands.push(Command::SaveSwatch(
                        slot,
                        Surface::Canvas,
                        [point.x as u32, point.y as u32],
                    ));
                    self.workbench_status = format!("Filled Jar {} with canvas color.", slot + 1);
                    audio::play(audio::Sound::JarClack);
                } else {
                    self.workbench_status =
                        "Custom jar shelf full (64 jars). Empty a jar to make room.".into();
                }
                return;
            }
            (Surface::Canvas, point)
        } else {
            return;
        };
        self.pressure = pressure;
        self.snapshot();
        let sample = Sample {
            point,
            pressure,
            timestamp_ms,
        };
        self.stroke = Some(if self.ink_selected && surface == Surface::Canvas {
            Stroke::start_ink(source, sample, &mut self.commands)
        } else {
            if surface == Surface::Canvas {
                let dep = (0.0008
                    * (self.workbench.sizes[self.brush.index()] / 64.0).clamp(0.4, 2.0)
                    * pressure)
                    .min(0.002);
                let load = &mut self.workbench.session.loads[self.brush.index()];
                *load = (*load - dep).max(0.0);
            }
            Stroke::start(
                source,
                sample,
                self.workbench.sizes[self.brush.index()],
                surface,
                self.brush,
                self.workbench.angle.to_radians(),
                &mut self.commands,
            )
        });
    }

    fn wipe_gesture(&mut self, source: Source, position: Pos2) {
        if let Some((owner, start)) = self.rag_gesture {
            if source == owner
                && position.distance(start) >= 12. * self.window.scale_factor() as f32
            {
                self.wipe();
            }
        }
    }

    fn move_stroke(&mut self, source: Source, position: Pos2, pressure: f32, timestamp_ms: u32) {
        if pressure <= 0.0 {
            self.end_stroke();
            return;
        }
        self.wipe_gesture(source, position);
        let Some(surface) = self
            .stroke
            .as_ref()
            .filter(|stroke| stroke.source == source)
            .map(|stroke| stroke.surface)
        else {
            return;
        };
        let Some(point) = self.to_surface(surface, position) else {
            self.end_stroke();
            return;
        };
        self.pressure = pressure;
        if let Some(stroke) = &mut self.stroke {
            let before = self.commands.len();
            let surface = stroke.surface;
            stroke.extend(
                Sample {
                    point,
                    pressure,
                    timestamp_ms,
                },
                &mut self.commands,
            );
            if !self.ink_selected && surface == Surface::Canvas {
                let dabs = self.commands.len() - before;
                if dabs > 0 {
                    let dep = (0.0008
                        * (self.workbench.sizes[self.brush.index()] / 64.0).clamp(0.4, 2.0)
                        * pressure)
                        .min(0.002);
                    let load = &mut self.workbench.session.loads[self.brush.index()];
                    *load = (*load - dabs as f32 * dep).max(0.0);
                }
            }
        }
    }

    fn end_stroke(&mut self) {
        self.rag_gesture = None;
        if let Some(stroke) = self.stroke.take() {
            let before = self.commands.len();
            let surface = stroke.surface;
            stroke.finish(&mut self.commands);
            if !self.ink_selected && surface == Surface::Canvas {
                let dabs = self.commands.len() - before;
                if dabs > 0 {
                    let dep =
                        0.0008 * (self.workbench.sizes[self.brush.index()] / 64.0).clamp(0.4, 2.0);
                    let load = &mut self.workbench.session.loads[self.brush.index()];
                    *load = (*load - dabs as f32 * dep).max(0.0);
                }
            }
        }
        self.pressure = 0.;
    }
    fn mouse_pressure(&self) -> f32 {
        if self.ink_selected {
            0.87
        } else {
            0.65
        }
    }

    fn mouse_move(&mut self, position: PhysicalPosition<f64>) {
        self.move_stroke(
            Source::Mouse,
            egui::pos2(position.x as f32, position.y as f32),
            self.mouse_pressure(),
            self.elapsed(),
        );
    }

    fn drain_pen(&mut self) -> bool {
        if self.pen_waits_for_ui {
            return false;
        }
        let samples = self.pen.drain();
        for sample in &samples {
            let [x, y] = sample.position;
            let pos = egui::pos2(x, y);
            match sample.phase {
                Phase::Down => {
                    self.start_stroke(Source::Pen, pos, sample.pressure, sample.timestamp_ms)
                }
                Phase::Move => {
                    self.move_stroke(Source::Pen, pos, sample.pressure, sample.timestamp_ms)
                }
                Phase::Up => {
                    self.wipe_gesture(Source::Pen, pos);
                    self.end_pen_stroke();
                }
                Phase::Cancel => self.end_pen_stroke(),
                Phase::MouseCaptureLost => {
                    self.pan_drag = None;
                    if self
                        .rag_gesture
                        .is_some_and(|(source, _)| source == Source::Mouse)
                    {
                        self.rag_gesture = None;
                    }
                    if self
                        .stroke
                        .as_ref()
                        .is_some_and(|s| s.source == Source::Mouse)
                    {
                        self.end_stroke();
                    }
                }
            }
        }
        !samples.is_empty()
    }

    fn reset_pen(&mut self) {
        self.pen_waits_for_ui = false;
        self.pen.reset_contact();
    }

    fn end_pen_stroke(&mut self) {
        if self
            .rag_gesture
            .is_some_and(|(source, _)| source == Source::Pen)
        {
            self.rag_gesture = None;
        }
        if self
            .stroke
            .as_ref()
            .is_some_and(|stroke| stroke.source == Source::Pen)
        {
            self.end_stroke();
        }
    }

    fn layout(&mut self, ctx: &egui::Context) {
        let prev_lighting = self.workbench.lighting;
        egui::TopBottomPanel::top("tools").show(ctx, |ui| {
            ui.add_space(6.);
            ui.horizontal(|ui| {
                ui.strong("Pigment Studio");
                ui.separator();
                let can_undo = !self.placement_pending && !self.workbench.history.is_empty();
                let undo_shortcut =
                    ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .on_hover_text("Undo last stroke or action (Ctrl+Z)")
                    .clicked()
                    || (can_undo && undo_shortcut)
                {
                    self.end_stroke();
                    if self.workbench.undo() {
                        self.commands.push(Command::Undo);
                    }
                }
                let typing = ctx.wants_keyboard_input();
                let undo_shortcut =
                    !typing && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Z));
                if ui
                    .add_enabled(can_undo, egui::Button::new("Undo"))
                    .on_hover_text("Undo last stroke or action (Ctrl+Z)")
                    .clicked()
                    || (can_undo && undo_shortcut)
                {
                    self.end_stroke();
                    if self.workbench.undo() {
                        self.commands.push(Command::Undo);
                    }
                }
                let dry_shortcut =
                    !typing && ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::D));
                let note_shortcut =
                    !typing && ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::N));
                let f12_pressed =
                    ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::F12));

                if note_shortcut && self.active_note_at.is_none() {
                    self.note_tool = !self.note_tool;
                    if self.note_tool {
                        self.workbench_status = "📌 Note tool active: Click on canvas to pin a note for the agent (N to toggle).".into();
                    } else {
                        self.workbench_status = "Note tool deactivated.".into();
                    }
                }

                if f12_pressed {
                    if let Some(cursor_pos) = self.cursor {
                        if let Some(pt) = self.to_surface(Surface::Canvas, cursor_pos) {
                            let diag = self.paint.sample_diagnostics(&self.device, &self.queue, pt.x as u32, pt.y as u32);
                            let note = notes::add(&notes::default_path(), pt.x, pt.y, "Quick F12 diagnostic dump", "artist", Some(diag));
                            self.notes = notes::read(&notes::default_path());
                            self.workbench_status = format!("F12: Dumped diagnostic note #{} at ({:.0}, {:.0}).", note.id, pt.x, pt.y);
                            audio::play(audio::Sound::BrassClick);
                        }
                    }
                }

                let note_btn = ui.selectable_label(self.note_tool, "📌 Note (N)")
                    .on_hover_text("Toggle Note tool (N): Click anywhere on the canvas to drop a pinned diagnostic note for the agent. Hover and press F12 for quick dump.");
                if note_btn.clicked() {
                    self.note_tool = !self.note_tool;
                    if self.note_tool {
                        self.workbench_status = "📌 Note tool active: Click on canvas to pin a note for the agent.".into();
                    }
                }

                let total_notes = self.notes.len();
                let open_notes = self.notes.iter().filter(|n| !n.done).count();
                let notes_list_btn = ui.selectable_label(
                    self.notes_manager_open,
                    format!("📋 Notes ({open_notes}/{total_notes}) ▾"),
                ).on_hover_text("Open Notes Manager: view, manage, inspect, or pan to pinned notes.");
                if notes_list_btn.clicked() {
                    self.notes_manager_open = !self.notes_manager_open;
                }

                if ui
                    .button("☀️ Dry canvas")
                    .on_hover_text("Lock and cure wet paint into a permanent dry underlayer (Ctrl+D). Allows glazing and painting over background without bleed-through or brush contamination.")
                    .clicked()
                    || dry_shortcut
                {
                    self.end_stroke();
                    self.snapshot();
                    self.commands.push(Command::DryCanvas);
                    audio::play(audio::Sound::BrassClick);
                }
                if ui.button("Fit canvas").clicked() {
                    self.end_stroke();
                    self.view = CanvasView::default();
                    self.update_canvas();
                }
                if ui.button("Clear canvas").clicked() {
                    self.end_stroke();
                    self.snapshot();
                    self.commands.push(Command::Clear(Surface::Canvas));
                }
                if ui
                    .add_enabled(
                        self.exporting.is_none() && self.export_path.is_none(),
                        egui::Button::new("Export PNG"),
                    )
                    .clicked()
                {
                    self.end_stroke();
                    self.export_path = rfd::FileDialog::new()
                        .set_parent(&*self.window)
                        .set_file_name("painting.png")
                        .add_filter("PNG image", &["png"])
                        .save_file();
                }
            });
            ui.add_space(6.);
        });

        self.palette = None;
        self.palette_clip = None;
        self.rag = None;
        self.overlay = None;
        egui::SidePanel::right("workbench")
            .resizable(false)
            .exact_width(workbench_width(ctx))
            .show(ctx, |ui| self.workbench_ui(ui));

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(Color32::from_rgb(20, 21, 24)))
            .show(ctx, |ui| {
                let room = ui.available_rect_before_wrap();
                let viewport = physical_rect(ctx, room);
                self.viewport = Some(viewport);
                let physical = self.view.rect(viewport);
                let scale = ctx.pixels_per_point();
                let canvas = Rect::from_min_max(
                    (physical.min.to_vec2() / scale).to_pos2(),
                    (physical.max.to_vec2() / scale).to_pos2(),
                );
                ui.painter().with_clip_rect(room).image(
                    self.texture_id,
                    canvas,
                    gpu::atlas_uv(Surface::Canvas.bounds()),
                    Color32::WHITE,
                );
                if self.pan_drag.is_some() {
                    ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
                } else if self.note_tool || self.workbench.sampling {
                    ctx.set_cursor_icon(egui::CursorIcon::PointingHand);
                } else if ui.rect_contains_pointer(canvas.intersect(room)) {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                let note_popup_rect = self.draw_note_pins(ui, room, canvas);
                if let Some(nr) = note_popup_rect {
                    self.overlay = Some(physical_rect(ctx, nr));
                }
                self.canvas = Some(physical);
            });
        self.pigment_info_ui(ctx);
        self.guide_ui(ctx);
        self.notes_manager_ui(ctx);

        if self.workbench.lighting != prev_lighting {
            self.paint.lighting = self.workbench.lighting;
            self.commands
                .push(Command::Dab(Dab::operation(Surface::Canvas.bounds(), 1)));
            self.window.request_redraw();
        }
        self.update_pen_regions();
    }

    fn workbench_ui(&mut self, ui: &mut egui::Ui) {
        let height = ui.available_height();
        // Keep palette and brush controls in view while leaving room for jars in short windows.
        egui::ScrollArea::vertical()
            .id_salt("paint_tools")
            .max_height((height - 110.).max(height * 0.65))
            .min_scrolled_height(0.)
            .auto_shrink([false, true])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                self.mixing_palette_ui(ui);
                self.brush_controls_ui(ui);
            });

        ui.separator();
        ui.heading("Atelier Rack");
        egui::ScrollArea::vertical()
            .id_salt("atelier_rack")
            .min_scrolled_height(0.)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.atelier_rack_ui(ui);
                self.custom_jars_ui(ui);
                self.studio_settings_ui(ui);
            });
    }

    fn mixing_palette_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.);
        ui.heading("Mixing palette");
        ui.add_space(4.);
        let width = ui.available_width().min(288.);
        let (palette, _) =
            ui.allocate_exact_size(egui::vec2(width, width * 0.5), egui::Sense::hover());
        ui.painter().image(
            self.texture_id,
            palette,
            gpu::atlas_uv(Surface::Palette.bounds()),
            Color32::WHITE,
        );
        let edge_y = palette.top() + palette.height() * 0.25;
        let line = egui::Stroke::new(1., Color32::from_gray(215));
        ui.painter().hline(palette.x_range(), edge_y, line);
        for slot in 1..8 {
            let x = palette.left() + palette.width() * slot as f32 / 8.;
            ui.painter().vline(x, palette.top()..=edge_y, line);
        }
        let visible = palette.intersect(ui.clip_rect());
        // Hit-test the visible portion but retain the uncut rectangle for mapping.
        if visible.is_positive() {
            self.palette = Some(physical_rect(ui.ctx(), palette));
            self.palette_clip = Some(physical_rect(ui.ctx(), visible));
        }
        if ui.rect_contains_pointer(palette) {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        }

        ui.horizontal(|ui| {
            if ui
                .selectable_label(self.workbench.sampling, "Fill Jar")
                .on_hover_text(
                    "Then tap a mixed color on the canvas or palette to fill a custom jar",
                )
                .clicked()
            {
                self.workbench.sampling = !self.workbench.sampling;
                self.workbench_status.clear();
            }
            if ui
                .add_enabled(!self.placement_pending, egui::Button::new("Clear palette"))
                .clicked()
            {
                self.end_stroke();
                self.snapshot();
                self.commands.push(Command::Clear(Surface::Palette));
            }
        });
        if self.workbench.sampling {
            ui.label("Tap a color on the canvas or palette to fill a jar.");
        }
        if !self.workbench_status.is_empty() {
            ui.label(&self.workbench_status);
        }
    }

    fn atelier_rack_ui(&mut self, ui: &mut egui::Ui) {
        let gap = 6.;
        let width = ui.available_width();
        let columns = if width >= 366. { 2 } else { 1 };
        let tile_width = (width - gap * (columns - 1) as f32) / columns as f32;
        for group in cabinet::PigmentGroup::ALL {
            ui.add_space(4.);
            ui.strong(egui::RichText::new(group.title()).size(15.));
            egui::Grid::new(group.title())
                .num_columns(columns)
                .spacing([gap, 2.])
                .show(ui, |ui| {
                    for (row_index, (slot, pigment)) in cabinet::PIGMENTS
                        .iter()
                        .enumerate()
                        .filter(|(_, pigment)| pigment.group == group)
                        .enumerate()
                    {
                        ui.push_id(pigment.id, |ui| {
                            self.pigment_tile(ui, tile_width, slot, pigment);
                        });
                        if (row_index + 1) % columns == 0 {
                            ui.end_row();
                        }
                    }
                });
        }
    }

    fn pigment_tile(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        slot: usize,
        pigment: &'static cabinet::PigmentDef,
    ) {
        let jar_size = egui::vec2(40., 36.);
        let info_width = 35.;
        let padding = 5.;
        let name_x = jar_size.x + 2. * padding;
        let name_width = (width - info_width - name_x - padding).max(1.);
        let name = ui.painter().layout(
            pigment.name.to_owned(),
            egui::FontId::proportional(16.),
            ui.visuals().text_color(),
            name_width,
        );
        let height = (name.size().y + 6.).max(46.);
        let (tile, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        let dip_rect = Rect::from_min_max(tile.min, tile.max - egui::vec2(info_width, 0.));
        let dip = ui.interact(dip_rect, ui.id().with("dip"), egui::Sense::click());
        dip.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, pigment.name));
        let visuals = ui.style().interact(&dip);
        ui.painter()
            .rect_filled(dip_rect.shrink(1.), 4., visuals.bg_fill);
        let jar = Rect::from_min_size(
            egui::pos2(tile.left() + padding, tile.center().y - jar_size.y * 0.5),
            jar_size,
        );
        let lid = Rect::from_min_size(jar.min + egui::vec2(6., 2.), egui::vec2(28., 8.));
        let body = Rect::from_min_max(jar.min + egui::vec2(2., 10.), jar.max - egui::vec2(2., 2.));
        let [r, g, b] = pigment.rgb;
        ui.painter().rect_filled(lid, 2., Color32::from_gray(140));
        ui.painter()
            .rect_filled(body, 4., Color32::from_rgb(r, g, b));
        ui.painter().rect_stroke(
            body,
            4.,
            egui::Stroke::new(1., Color32::from_gray(60)),
            egui::StrokeKind::Inside,
        );
        let name_pos = egui::pos2(tile.left() + name_x, tile.center().y - name.size().y * 0.5);
        ui.painter()
            .galley(name_pos, name, ui.visuals().text_color());
        if dip.on_hover_text("Dip brush (+25% load)").clicked() {
            self.dip_from_jar(BrushService::DipRack(slot));
        }

        let info_rect = Rect::from_min_max(
            egui::pos2(dip_rect.right() + 1., tile.top() + 2.),
            tile.max - egui::Vec2::splat(2.),
        );
        let mut info = ui.new_child(egui::UiBuilder::new().max_rect(info_rect).layout(
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
        ));
        if info
            .add(
                egui::Button::new("Info")
                    .small()
                    .wrap_mode(egui::TextWrapMode::Extend),
            )
            .on_hover_text(format!("About {}", pigment.name))
            .clicked()
        {
            self.selected_dossier = Some(pigment);
            audio::play(audio::Sound::BrassClick);
        }
    }

    fn custom_jars_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.strong("Custom jars");
        let custom_count = self.workbench.session.jars.iter().filter(|&&o| o).count();
        if custom_count == 0 {
            ui.label(
                egui::RichText::new(
                    "No custom jars. Click \"Fill Jar\" above and tap a color on the canvas or palette to save it.",
                )
                .weak()
                .italics(),
            );
        } else {
            for (slot, occupied) in self.workbench.session.jars.into_iter().enumerate() {
                if !occupied {
                    continue;
                }
                ui.horizontal(|ui| {
                    let (jar, icon) =
                        ui.allocate_exact_size(egui::vec2(20., 18.), egui::Sense::click());
                    let lid =
                        Rect::from_min_size(jar.min + egui::vec2(3., 1.), egui::vec2(14., 4.));
                    let body = Rect::from_min_max(
                        jar.min + egui::vec2(1., 5.),
                        jar.max - egui::vec2(1., 1.),
                    );
                    ui.painter().rect_filled(lid, 1., Color32::from_gray(140));
                    ui.painter().image(
                        self.texture_id,
                        body,
                        gpu::atlas_uv(gpu::swatch_bounds(slot)),
                        Color32::WHITE,
                    );
                    let name = format!("Jar {}", slot + 1);
                    let dip = ui
                        .small_button(&name)
                        .on_hover_text("Dip active brush (+25% load)");
                    if icon
                        .on_hover_text(format!("Dip into {name} (+25% load)"))
                        .clicked()
                        || dip.clicked()
                    {
                        self.dip_from_jar(BrushService::DipCustom(slot));
                    }
                    if ui
                        .small_button("×")
                        .on_hover_text(format!("Empty {name}"))
                        .clicked()
                    {
                        self.end_stroke();
                        self.snapshot();
                        self.workbench.session.jars[slot] = false;
                        self.commands.push(Command::RemoveSwatch(slot));
                        self.workbench_status = format!("Emptied {name}.");
                    }
                });
            }
        }
    }

    fn brush_controls_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.strong("Brush rack");
        ui.horizontal(|ui| {
            self.brush_card(ui, Brush::Round, "Round");
            self.brush_card(ui, Brush::Flat, "Flat");
            if ui
                .add_sized(
                    egui::vec2(58., 42.),
                    egui::SelectableLabel::new(self.ink_selected, "Ink Pen"),
                )
                .on_hover_text("Draw lightly for a fine line. Press and pause to make ink drip.")
                .clicked()
                && !self.ink_selected
            {
                self.end_stroke();
                self.ink_selected = true;
            }
        });
        if !self.ink_selected {
            ui.add(
                egui::Slider::new(&mut self.workbench.sizes[self.brush.index()], 4.0..=96.0)
                    .text("Size")
                    .suffix(" px")
                    .integer(),
            );
        }
        if !self.ink_selected && self.brush == Brush::Flat {
            ui.add(
                egui::Slider::new(&mut self.workbench.angle, 0.0..=180.0)
                    .text("Angle")
                    .suffix("°")
                    .integer(),
            )
            .on_hover_text("Sets the direction of the flat brush’s wide edge.");
        }
        ui.add(egui::ProgressBar::new(self.pressure).text(format!(
            "Pressure {}%",
            (self.pressure * 100.).round() as u32
        )));
        if self.ink_selected {
            ui.label("Press and pause to make ink drip.");
        } else {
            ui.horizontal(|ui| {
                let (rag, _) = ui.allocate_exact_size(egui::vec2(72., 25.), egui::Sense::hover());
                ui.painter()
                    .rect_filled(rag, 4., Color32::from_rgb(89, 75, 67));
                ui.painter().text(
                    rag.center(),
                    egui::Align2::CENTER_CENTER,
                    "Wipe rag",
                    egui::FontId::proportional(12.),
                    Color32::WHITE,
                );
                let visible = rag.intersect(ui.clip_rect());
                if visible.is_positive() {
                    self.rag = Some(physical_rect(ui.ctx(), visible));
                }
                if ui
                    .button("Clean brush")
                    .on_hover_text("Completely remove all paint from active brush")
                    .clicked()
                {
                    self.clean_brush();
                }
            });
            ui.label("Swipe rag to wipe off 10% load.");
        }
    }

    fn studio_settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.separator();
        ui.collapsing("💡 3D Impasto & Studio Light", |ui| {
            ui.checkbox(&mut self.workbench.lighting.enabled, "Enable 3D Relief")
                .on_hover_text("Toggle 3D impasto thickness, canvas weave relief, and raking studio light on the canvas");
            if self.workbench.lighting.enabled {
                ui.add(
                    egui::Slider::new(&mut self.workbench.lighting.impasto_depth, 0.0..=2.0)
                        .text("Impasto Depth"),
                ).on_hover_text("Scale of physical paint ridge height and bristle furrows");
                ui.add(
                    egui::Slider::new(&mut self.workbench.lighting.canvas_weave, 0.0..=1.0)
                        .text("Canvas Weave"),
                ).on_hover_text("Tactile primed linen weave texture strength on canvas");
                ui.add(
                    egui::Slider::new(&mut self.workbench.lighting.specular_sheen, 0.0..=2.0)
                        .text("Bristle Sheen"),
                ).on_hover_text("Wet oil gloss and bristle crest specular reflection intensity");
                ui.add(
                    egui::Slider::new(&mut self.workbench.lighting.light_angle_deg, 0.0..=360.0)
                        .text("Light Azimuth")
                        .suffix("°"),
                ).on_hover_text("Azimuth direction of the studio raking light (315° is classical top-left)");
                ui.add(
                    egui::Slider::new(&mut self.workbench.lighting.light_elevation_deg, 15.0..=75.0)
                        .text("Elevation")
                        .suffix("°"),
                ).on_hover_text("Raking angle of light above the canvas plane (lower angles cast longer relief shadows)");
                if ui
                    .button("↺ Reset to North Light")
                    .on_hover_text("Reset to standard studio raking light (315° NW, 45° elevation)")
                    .clicked()
                {
                    self.workbench.lighting.light_angle_deg = 315.0;
                    self.workbench.lighting.light_elevation_deg = 45.0;
                    self.workbench.lighting.impasto_depth = 1.0;
                    self.workbench.lighting.canvas_weave = 0.35;
                    self.workbench.lighting.specular_sheen = 1.0;
                }
            }
        });

        ui.separator();
        if ui.button("Get to know your blue").clicked() {
            self.workbench.guide = None;
            self.workbench.exercise = true;
        }

        if self.exporting.is_some() {
            ui.label("Exporting PNG…");
        } else if !self.status.is_empty() {
            ui.label(&self.status);
        }
    }

    fn brush_card(&mut self, ui: &mut egui::Ui, brush: Brush, name: &str) {
        let (rect, response) = ui.allocate_exact_size(egui::vec2(104., 42.), egui::Sense::click());
        let color = if !self.ink_selected && self.brush == brush {
            Color32::from_rgb(49, 73, 70)
        } else {
            Color32::from_gray(43)
        };
        ui.painter().rect_filled(rect, 5., color);
        let index = brush.index();
        let load = self.workbench.session.loads[index];

        let y = rect.center().y;
        let x = rect.left() + 6.;
        let handle = Rect::from_min_size(egui::pos2(x, y - 2.5), egui::vec2(13., 5.));
        ui.painter()
            .rect_filled(handle, 2., Color32::from_rgb(163, 110, 72));
        let ferrule = Rect::from_min_size(egui::pos2(x + 11., y - 4.), egui::vec2(6., 8.));
        ui.painter()
            .rect_filled(ferrule, 1., Color32::from_gray(158));
        let tip_height = if brush == Brush::Round { 10. } else { 16. };
        let tip = Rect::from_min_size(
            egui::pos2(x + 17., y - tip_height / 2.),
            egui::vec2(15., tip_height),
        );
        // Base dry bristle background
        ui.painter().rect_filled(
            tip,
            if index == 0 { 4. } else { 1. },
            Color32::from_rgb(125, 105, 90),
        );

        // Paint reservoir on bristles (fills from tip inward: right to left)
        if load > 0.005 {
            let painted_width = ((tip.width() - 1.) * load.clamp(0.0, 1.0)).max(1.5);
            let painted_left = tip.right() - 0.5 - painted_width;
            let count = if index == 0 { 1 } else { 3 };
            for group in 0..count {
                let lane = if index == 0 { 0 } else { 1 + group };
                let height = tip.height() / count as f32;
                let area = Rect::from_min_max(
                    egui::pos2(painted_left, tip.top() + group as f32 * height + 0.5),
                    egui::pos2(
                        tip.right() - 0.5,
                        tip.top() + (group + 1) as f32 * height - 0.5,
                    ),
                );
                ui.painter().image(
                    self.texture_id,
                    area,
                    gpu::atlas_uv([512 + lane, SIDE, 1, 1]),
                    Color32::WHITE,
                );
            }
        }

        // Right side: Name, mini progress bar, and load percentage text
        let content_x = rect.left() + 40.;
        ui.painter().text(
            egui::pos2(content_x, rect.top() + 4.),
            egui::Align2::LEFT_TOP,
            name,
            egui::FontId::proportional(12.),
            Color32::WHITE,
        );

        let track =
            Rect::from_min_size(egui::pos2(content_x, rect.top() + 19.), egui::vec2(54., 4.));
        ui.painter().rect_filled(track, 2., Color32::from_gray(28));
        if load > 0.005 {
            let fill_width = (track.width() * load.clamp(0.0, 1.0)).max(2.0);
            let fill_rect = Rect::from_min_size(track.min, egui::vec2(fill_width, track.height()));
            let lane = if index == 0 { 0 } else { 2 };
            ui.painter().image(
                self.texture_id,
                fill_rect,
                gpu::atlas_uv([512 + lane, SIDE, 1, 1]),
                Color32::WHITE,
            );
        }

        let load_pct = (load * 100.0).round() as u32;
        let status_text = if load < 0.005 {
            "Empty".to_string()
        } else {
            format!("{load_pct}%")
        };
        ui.painter().text(
            egui::pos2(content_x, rect.top() + 26.),
            egui::Align2::LEFT_TOP,
            status_text,
            egui::FontId::proportional(10.),
            if load < 0.005 {
                Color32::from_gray(140)
            } else {
                Color32::from_gray(210)
            },
        );

        if response
            .on_hover_text(format!(
                "{name} brush: {load_pct}% loaded. Bristle tip fills from edge inward."
            ))
            .clicked()
            && (self.ink_selected || self.brush != brush)
        {
            self.end_stroke();
            self.brush = brush;
            self.ink_selected = false;
        }
    }

    fn draw_note_pins(&mut self, ui: &mut egui::Ui, room: Rect, canvas: Rect) -> Option<Rect> {
        let mut clicked_pin = None;
        let scale = canvas.width() / 1024.0;
        let mut hovered_tip = None;

        for note in &self.notes {
            let screen_pos = canvas.min + egui::vec2(note.x * scale, note.y * scale);
            if !room.contains(screen_pos) {
                continue;
            }
            let pin_rect = Rect::from_center_size(screen_pos, egui::vec2(22.0, 22.0));
            let resp = ui.allocate_rect(pin_rect, egui::Sense::click());
            if resp.clicked() {
                clicked_pin = Some(note.id);
            }
            if resp.hovered() {
                hovered_tip = Some((screen_pos, note.clone()));
            }

            let painter = ui.painter().with_clip_rect(room);
            let fill = if note.done {
                Color32::from_rgb(46, 160, 67)
            } else {
                Color32::from_rgb(235, 165, 30)
            };
            if self.selected_note == Some(note.id) {
                painter.circle_stroke(
                    screen_pos,
                    14.0,
                    egui::Stroke::new(2.5, Color32::from_rgb(100, 210, 255)),
                );
            }
            painter.circle_filled(screen_pos, 10.0, fill);
            painter.circle_stroke(
                screen_pos,
                10.0,
                egui::Stroke::new(1.5, Color32::from_black_alpha(200)),
            );
            let label = if note.done {
                "✓".to_string()
            } else {
                note.id.to_string()
            };
            painter.text(
                screen_pos,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(11.0),
                Color32::WHITE,
            );
        }

        if let Some(id) = clicked_pin {
            self.selected_note = Some(id);
            self.active_note_at = None;
        }

        // Show hover tooltip
        if let Some((pos, note)) = hovered_tip {
            egui::Area::new(egui::Id::new("note_hover_tooltip"))
                .fixed_pos(pos + egui::vec2(12.0, 12.0))
                .order(egui::Order::Tooltip)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_max_width(280.0);
                        ui.horizontal(|ui| {
                            let status = if note.done { "✓ Done" } else { "● Open" };
                            let col = if note.done {
                                Color32::from_rgb(100, 220, 100)
                            } else {
                                Color32::from_rgb(240, 180, 50)
                            };
                            ui.colored_label(col, format!("Note #{} [{status}]", note.id));
                            ui.label(format!("({:.0}, {:.0})", note.x, note.y));
                        });
                        ui.add_space(2.0);
                        ui.label(&note.text);
                        if let Some(diag) = &note.diagnostics {
                            ui.separator();
                            ui.label(format!(
                                "Wet: {:.3} | Dry: {:.3}",
                                diag.wet_total, diag.dry_total
                            ));
                            ui.label(format!("Display: RGB{:?}", diag.display_srgb));
                            ui.label(format!(
                                "Intimate blend: RGB{:?}",
                                diag.optical.intimate_blend_srgb
                            ));
                        }
                    });
                });
        }

        self.note_popover(ui.ctx(), canvas)
    }

    fn note_popover(&mut self, ctx: &egui::Context, canvas: Rect) -> Option<Rect> {
        let scale = canvas.width() / 1024.0;
        let mut overlay_rect = None;

        // 1. Window for creating a new note at active_note_at
        if let Some(pt) = self.active_note_at {
            let screen_pos = canvas.min + egui::vec2(pt.x * scale, pt.y * scale);
            let popup_id = egui::Id::new("create_note_popup");
            let mut pin_action = false;
            let mut cancel_action = false;

            let window_resp = egui::Window::new("📌 Pin Note for Agent")
                .id(popup_id)
                .fixed_pos(screen_pos + egui::vec2(14.0, 14.0))
                .collapsible(false)
                .resizable(false)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    ui.set_max_width(280.0);
                    ui.label(format!("Canvas coordinate: ({:.0}, {:.0})", pt.x, pt.y));
                    ui.add_space(4.0);
                    let text_resp = ui.add(
                        egui::TextEdit::multiline(&mut self.note_input_text)
                            .hint_text("What looks weird or what should the agent know/do here?")
                            .desired_rows(3),
                    );
                    if self.note_input_text.is_empty() && !text_resp.has_focus() {
                        text_resp.request_focus();
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        let can_pin = !self.note_input_text.trim().is_empty();
                        if ui
                            .add_enabled(can_pin, egui::Button::new("📌 Pin it"))
                            .clicked()
                        {
                            pin_action = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_action = true;
                        }
                    });
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel_action = true;
                    }
                    if ui.input(|i| {
                        i.key_pressed(egui::Key::Enter) && (i.modifiers.ctrl || i.modifiers.command)
                    }) && !self.note_input_text.trim().is_empty()
                    {
                        pin_action = true;
                    }
                });

            if let Some(w) = window_resp {
                overlay_rect = Some(w.response.rect);
            }

            if pin_action {
                let diag = self.paint.sample_diagnostics(
                    &self.device,
                    &self.queue,
                    pt.x as u32,
                    pt.y as u32,
                );
                let note = notes::add(
                    &notes::default_path(),
                    pt.x,
                    pt.y,
                    &self.note_input_text,
                    "artist",
                    Some(diag),
                );
                self.notes = notes::read(&notes::default_path());
                self.active_note_at = None;
                self.note_input_text.clear();
                self.note_tool = false;
                self.workbench_status = format!("Pinned note #{} with GPU diagnostics.", note.id);
                audio::play(audio::Sound::BrassClick);
            }
            if cancel_action {
                self.active_note_at = None;
                self.note_input_text.clear();
            }
        }

        // 2. Window for viewing/editing selected_note
        if let Some(id) = self.selected_note {
            if let Some(note) = self.notes.iter().find(|n| n.id == id).cloned() {
                let screen_pos = canvas.min + egui::vec2(note.x * scale, note.y * scale);
                let popup_id = egui::Id::new("view_note_popup");
                let mut toggle_done = false;
                let mut delete_note = false;
                let mut close_popup = false;

                let window_resp = egui::Window::new(format!("📌 Note #{} Details", note.id))
                    .id(popup_id)
                    .fixed_pos(screen_pos + egui::vec2(14.0, 14.0))
                    .collapsible(false)
                    .resizable(false)
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        ui.set_max_width(340.0);
                        ui.horizontal(|ui| {
                            let status_str = if note.done { "✓ DONE" } else { "● OPEN" };
                            let col = if note.done {
                                Color32::from_rgb(100, 220, 100)
                            } else {
                                Color32::from_rgb(240, 180, 50)
                            };
                            ui.colored_label(col, format!("[{}]", status_str));
                            ui.label(format!(
                                "by {} at ({:.0}, {:.0})",
                                note.author, note.x, note.y
                            ));
                        });
                        ui.add_space(4.0);
                        ui.label(&note.text);

                        if let Some(diag) = &note.diagnostics {
                            ui.add_space(6.0);
                            ui.separator();
                            ui.heading("🔬 GPU Layer Diagnostics");
                            ui.label(format!("Wet pigment mass: {:.4}", diag.wet_total));
                            ui.monospace(format!(
                                "  Channels: [{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}]",
                                diag.wet_channels[0],
                                diag.wet_channels[1],
                                diag.wet_channels[2],
                                diag.wet_channels[3],
                                diag.wet_channels[4],
                                diag.wet_channels[5],
                                diag.wet_channels[6],
                                diag.wet_channels[7],
                            ));
                            ui.label(format!("Dry underlayer mass: {:.4}", diag.dry_total));
                            ui.monospace(format!(
                                "  Channels: [{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2}]",
                                diag.dry_channels[0],
                                diag.dry_channels[1],
                                diag.dry_channels[2],
                                diag.dry_channels[3],
                                diag.dry_channels[4],
                                diag.dry_channels[5],
                                diag.dry_channels[6],
                                diag.dry_channels[7],
                            ));
                            ui.add_space(2.0);
                            ui.label(format!("Rendered sRGB: RGB{:?}", diag.display_srgb));
                            ui.label(format!(
                                "Intimate blend sRGB: RGB{:?}",
                                diag.optical.intimate_blend_srgb
                            ));
                            ui.label(format!(
                                "Top layer estimated hiding: {:.1}%",
                                diag.optical.top_layer_opacity_pct
                            ));
                            ui.label(format!(
                                "R_sub mean: {:.3} | R_int mean: {:.3}",
                                diag.optical.r_sub_mean, diag.optical.r_int_mean
                            ));
                        }

                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let done_label = if note.done {
                                "Reopen"
                            } else {
                                "✓ Mark as Done"
                            };
                            if ui.button(done_label).clicked() {
                                toggle_done = true;
                            }
                            if ui.button("🗑 Delete").clicked() {
                                delete_note = true;
                            }
                            if ui.button("Close").clicked() {
                                close_popup = true;
                            }
                        });
                    });

                if let Some(w) = window_resp {
                    overlay_rect = Some(w.response.rect);
                }

                if toggle_done {
                    notes::set_done(&notes::default_path(), note.id, !note.done);
                    self.notes = notes::read(&notes::default_path());
                }
                if delete_note {
                    notes::remove(&notes::default_path(), note.id);
                    self.notes = notes::read(&notes::default_path());
                    self.selected_note = None;
                }
                if close_popup {
                    self.selected_note = None;
                }
            } else {
                self.selected_note = None;
            }
        }

        overlay_rect
    }

    fn notes_manager_ui(&mut self, ctx: &egui::Context) {
        if !self.notes_manager_open {
            return;
        }
        let mut open = self.notes_manager_open;
        let mut pan_to_coord = None;
        let mut inspect_id = None;
        let mut clear_all_done = false;

        let window_resp = egui::Window::new("📋 Notes Manager")
            .id(egui::Id::new("notes_manager_window"))
            .open(&mut open)
            .default_width(380.0)
            .default_height(400.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let total = self.notes.len();
                    let open_n = self.notes.iter().filter(|n| !n.done).count();
                    ui.label(format!("{open_n} open / {total} total"));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .button("🧹 Clear Done")
                            .on_hover_text("Remove all completed notes")
                            .clicked()
                        {
                            clear_all_done = true;
                        }
                    });
                });
                ui.separator();

                if self.notes.is_empty() {
                    ui.add_space(20.0);
                    ui.vertical_centered(|ui| {
                        ui.label("No pinned notes yet.");
                        ui.label(
                            "Press N or click '📌 Note (N)' and click on the canvas to add one.",
                        );
                    });
                } else {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let mut to_toggle = None;
                        let mut to_delete = None;

                        for note in &self.notes {
                            ui.group(|ui| {
                                ui.horizontal(|ui| {
                                    let mut done = note.done;
                                    if ui.checkbox(&mut done, "").clicked() {
                                        to_toggle = Some((note.id, done));
                                    }
                                    let badge_col = if note.done {
                                        Color32::from_rgb(46, 160, 67)
                                    } else {
                                        Color32::from_rgb(235, 165, 30)
                                    };
                                    ui.colored_label(badge_col, format!("#{}", note.id));
                                    ui.label(format!("({:.0}, {:.0})", note.x, note.y));

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            if ui
                                                .small_button("🗑")
                                                .on_hover_text("Delete note")
                                                .clicked()
                                            {
                                                to_delete = Some(note.id);
                                            }
                                            if ui
                                                .small_button("🔬 Inspect")
                                                .on_hover_text("View GPU layer diagnostics")
                                                .clicked()
                                            {
                                                inspect_id = Some(note.id);
                                            }
                                            if ui
                                                .small_button("🎯 Pan to")
                                                .on_hover_text("Center canvas on this note")
                                                .clicked()
                                            {
                                                pan_to_coord = Some((note.x, note.y));
                                            }
                                        },
                                    );
                                });

                                ui.label(&note.text);

                                if let Some(diag) = &note.diagnostics {
                                    ui.horizontal(|ui| {
                                        ui.monospace(format!(
                                            "Wet: {:.2} | Dry: {:.2} | RGB{:?}",
                                            diag.wet_total, diag.dry_total, diag.display_srgb
                                        ));
                                    });
                                }
                            });
                            ui.add_space(4.0);
                        }

                        if let Some((id, done)) = to_toggle {
                            notes::set_done(&notes::default_path(), id, done);
                            self.notes = notes::read(&notes::default_path());
                        }
                        if let Some(id) = to_delete {
                            notes::remove(&notes::default_path(), id);
                            self.notes = notes::read(&notes::default_path());
                            if self.selected_note == Some(id) {
                                self.selected_note = None;
                            }
                        }
                    });
                }
            });

        self.notes_manager_open = open;
        if let Some(w) = window_resp {
            let win_rect = physical_rect(ctx, w.response.rect);
            self.overlay = Some(self.overlay.map_or(win_rect, |o| o.union(win_rect)));
        }

        if clear_all_done {
            notes::clear_done(&notes::default_path());
            self.notes = notes::read(&notes::default_path());
        }
        if let Some((x, y)) = pan_to_coord {
            if let Some(viewport) = self.viewport {
                let rect = self.view.rect(viewport);
                let current_point = rect.min + egui::vec2(x / 1024.0, y / 1024.0) * rect.size();
                let delta = viewport.center() - current_point;
                self.pan_by(delta);
            }
        }
        if let Some(id) = inspect_id {
            self.selected_note = Some(id);
            self.active_note_at = None;
        }
    }

    fn pigment_info_ui(&mut self, ctx: &egui::Context) {
        let Some(pigment) = self.selected_dossier else {
            return;
        };

        let mut open = true;
        let window = egui::Window::new(pigment.name)
            .id(egui::Id::new("pigment_info"))
            .open(&mut open)
            .anchor(egui::Align2::RIGHT_TOP, [-workbench_width(ctx) - 12., 48.])
            .default_width(300.)
            .max_width(320.)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.label(format!("Pigment code: {}", pigment.code));
                ui.label(format!("Opacity: {}", pigment.opacity));
                ui.colored_label(Color32::from_rgb(230, 190, 100), pigment.price_tier);
                ui.separator();
                ui.label(pigment.lore);
            });

        if let Some(window) = window {
            let rect = physical_rect(ctx, window.response.rect);
            self.overlay = Some(self.overlay.map_or(rect, |overlay| overlay.union(rect)));
        }
        if !open {
            self.selected_dossier = None;
        }
    }

    fn guide_ui(&mut self, ctx: &egui::Context) {
        if let Some(index) = self.workbench.guide {
            let entry = &workbench::GUIDE[index];
            let mut open = true;
            let area = egui::Window::new(pigment::NAMES[index])
                .id(egui::Id::new("pigment guide"))
                .open(&mut open)
                .anchor(egui::Align2::LEFT_TOP, [20., 48.])
                .resizable(false)
                .default_width(280.)
                .show(ctx, |ui| {
                    ui.set_max_width(300.);
                    ui.strong(entry.code);
                    ui.label(entry.character);
                    ui.label(entry.use_case);
                    ui.add_space(6.);
                    ui.label(entry.prompt);
                    ui.label("Pigment Studio uses approximate digital paint curves.");
                    ui.hyperlink_to("Manufacturer reference", entry.source);
                    if ui
                        .add_enabled(!self.placement_pending, egui::Button::new("Try this mix"))
                        .clicked()
                    {
                        self.place_tubes(entry.mix);
                        self.workbench.guide = None;
                    }
                });
            if let Some(area) = area {
                let rect = physical_rect(ctx, area.response.rect);
                self.overlay = Some(self.overlay.map_or(rect, |o| o.union(rect)));
            }
            if !open {
                self.workbench.guide = None;
            }
        }
        if self.workbench.exercise {
            let mut open = true;
            let area = egui::Window::new("Get to know your blue").open(&mut open)
                .anchor(egui::Align2::LEFT_TOP, [20., 48.]).resizable(false).default_width(310.)
                .show(ctx, |ui| {
                    ui.set_max_width(320.);
                    ui.label("1. Predict which blue will change the white more.");
                    ui.label("2. Use a clean brush to add equal small amounts of blue to separate dabs of white. Fill a jar with each.");
                    ui.label("3. Adjust the mixes until they look equally light. Compare how much white each needed.");
                    ui.label("4. Repeat one mix with a wiped brush, then with a cleaned brush. Compare the jars.");
                    if ui.add_enabled(!self.placement_pending, egui::Button::new("Set out the four dabs")).clicked() {
                        self.place_tubes([5, 4, 2, 4]);
                    }
                });
            if let Some(area) = area {
                let rect = physical_rect(ctx, area.response.rect);
                self.overlay = Some(self.overlay.map_or(rect, |o| o.union(rect)));
            }
            self.workbench.exercise = open;
        }
    }
    fn redraw(&mut self, measure: bool) -> Result<Option<benchmark::FrameTiming>, String> {
        if !self.drawable {
            return Ok(None);
        }
        let started = Instant::now();
        let frame = match self.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.surface.configure(&self.device, &self.surface_config);
                self.window.request_redraw();
                return Ok(None);
            }
            Err(wgpu::SurfaceError::Timeout) => {
                self.window.request_redraw();
                return Ok(None);
            }
            Err(error) => return Err(format!("The GPU stopped presenting the canvas. Reopen the app and check your graphics driver. {error}")),
        };
        let acquired = Instant::now();
        let ctx = self.egui_state.egui_ctx().clone();
        let input = self.egui_state.take_egui_input(&self.window);
        let output = ctx.run(input, |ctx| self.layout(ctx));
        if std::mem::take(&mut self.pen_waits_for_ui) && self.drain_pen() {
            self.window.request_redraw();
        }
        self.egui_state
            .handle_platform_output(&self.window, output.platform_output);
        let jobs = ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.surface_config.width, self.surface_config.height],
            pixels_per_point: output.pixels_per_point,
        };
        for (id, delta) in &output.textures_delta.set {
            self.renderer
                .update_texture(&self.device, &self.queue, *id, delta);
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let count = self.commands.len().min(MAX_COMMANDS);
        let checked_palette = self.commands[..count]
            .iter()
            .any(|command| matches!(command, Command::CheckTubes(_)));
        self.paint.encode(
            &self.queue,
            &mut encoder,
            &self.commands[..self.commands.len().min(MAX_COMMANDS)],
        );
        self.commands.drain(..count);
        let placement =
            checked_palette.then(|| self.paint.placement_readback(&self.device, &mut encoder));
        let export = if self.commands.is_empty() {
            self.export_path
                .take()
                .map(|path| (path, self.paint.readback(&self.device, &mut encoder)))
        } else {
            self.window.request_redraw();
            None
        };
        let extra =
            self.renderer
                .update_buffers(&self.device, &self.queue, &mut encoder, &jobs, &screen);
        {
            let view = frame.texture.create_view(&Default::default());
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            self.renderer
                .render(&mut pass.forget_lifetime(), &jobs, &screen);
        }
        let encoded = Instant::now();
        let submission = self
            .queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        if let Some(buffer) = placement {
            self.placement_result = Some(gpu::placement_result(buffer));
        }
        if let Some((path, buffer)) = export {
            self.exporting = Some(gpu::export(buffer, path));
            self.window.request_redraw();
        }
        self.window.pre_present_notify();
        let present_started = Instant::now();
        frame.present();
        let presented = Instant::now();
        if measure {
            let _ = self
                .device
                .poll(wgpu::Maintain::WaitForSubmissionIndex(submission));
        }
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        Ok(measure.then(|| benchmark::FrameTiming {
            total: started.elapsed(),
            acquire: acquired - started,
            encode: encoded - acquired,
            present: presented - present_started,
            gpu_and_submit: (present_started - encoded) + presented.elapsed(),
        }))
    }
}

fn workbench_width(ctx: &egui::Context) -> f32 {
    (ctx.screen_rect().width() * 0.38).clamp(304., 452.)
}

fn physical_rect(ctx: &egui::Context, rect: Rect) -> Rect {
    let scale = ctx.pixels_per_point();
    Rect::from_min_max(
        (rect.min.to_vec2() * scale).to_pos2(),
        (rect.max.to_vec2() * scale).to_pos2(),
    )
}

struct CanvasView {
    zoom: f32,
    pan: egui::Vec2,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: egui::Vec2::ZERO,
        }
    }
}

impl CanvasView {
    fn fit(viewport: Rect) -> f32 {
        (viewport.width().min(viewport.height()) - 32.).max(1.)
    }

    fn rect(&self, viewport: Rect) -> Rect {
        let fit = Self::fit(viewport);
        Rect::from_center_size(
            viewport.center() + self.pan * fit,
            egui::Vec2::splat(fit * self.zoom),
        )
    }

    fn pan_by(&mut self, viewport: Rect, delta: egui::Vec2) {
        let fit = Self::fit(viewport);
        self.pan += delta / fit;
        let limit = (viewport.size() * 0.5 - egui::Vec2::splat(24.)) / fit
            + egui::Vec2::splat(self.zoom * 0.5);
        let limit = limit.max(egui::Vec2::ZERO);
        self.pan = self.pan.clamp(-limit, limit);
    }

    fn zoom_at(&mut self, viewport: Rect, cursor: Pos2, steps: f32) {
        let zoom = (self.zoom * 1.18_f32.powf(steps)).clamp(0.25, 16.);
        let before = self.rect(viewport).center();
        let after = cursor + (before - cursor) * (zoom / self.zoom);
        self.zoom = zoom;
        self.pan_by(viewport, after - before);
    }
}
