mod benchmark;
mod gpu;
mod pen;
mod pigment;
mod stroke;
mod workbench;

#[cfg(test)]
mod tests;

use egui::{Color32, Pos2, Rect};
use gpu::{Brush, BrushService, Command, PaintGpu, Surface, MAX_COMMANDS, SIDE};
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
    event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

type ExportResult = Receiver<Result<PathBuf, String>>;

fn main() -> std::process::ExitCode {
    let benchmark = std::env::args().any(|argument| argument == "--benchmark");
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
        studio.drain_pen();
        let response = studio.egui_state.on_window_event(&studio.window, &event);
        let mut redraw = response.repaint;
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
                studio.end_stroke();
                redraw = true;
            }
            WindowEvent::Focused(false) => {
                studio.pan_drag = None;
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
        let animating = studio
            .workbench
            .flick
            .is_some_and(|(_, time)| time.elapsed() < Duration::from_millis(420));
        if animating {
            studio.window.request_redraw();
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
            || animating
            || ink_animating;
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
    swatches: [Color32; 6],
    brush: Brush,
    workbench: workbench::Workbench,
    placement_pending: bool,
    placement_result: Option<Receiver<Option<bool>>>,
    workbench_status: String,
    benchmark_seeded: bool,
    ink_selected: bool,
    next_ink_step: Instant,

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
        let swatches = std::array::from_fn(|i| {
            let mut amount = [0.; 6];
            amount[i] = 2.;
            let [r, g, b] = pigment::rgb(amount);
            Color32::from_rgb(r, g, b)
        });
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
            swatches,
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
            brush: Brush::Round,
            workbench: workbench::Workbench::default(),
            placement_pending: false,
            placement_result: None,
            workbench_status: String::new(),
            benchmark_seeded: false,
            ink_selected: false,
            next_ink_step: Instant::now(),
            pressure: 0.,
            commands: vec![Command::Initialize],
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

    fn service_brush(&mut self, service: BrushService) {
        self.end_stroke();
        self.snapshot();
        self.workbench.session.rinsed[self.brush.index()] = false;
        self.commands.push(Command::Service(self.brush, service));
    }

    fn wipe(&mut self) {
        let fraction = if self.workbench.session.rinsed[self.brush.index()] {
            0.
        } else {
            0.08
        };
        self.service_brush(BrushService::Wipe(fraction));
    }

    fn start_stroke(&mut self, source: Source, position: Pos2, pressure: f32, timestamp_ms: u32) {
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
                    .swatches
                    .iter()
                    .position(|occupied| !*occupied)
                {
                    self.snapshot();
                    self.workbench.session.swatches[slot] = true;
                    self.commands
                        .push(Command::SaveSwatch(slot, [point.x as u32, point.y as u32]));
                } else {
                    self.workbench_status =
                        "Swatch strip full. Remove a swatch to make room.".into();
                }
                return;
            }
            if self.ink_selected {
                return;
            }
            (Surface::Palette, point)
        } else if let Some(point) = self.to_surface(Surface::Canvas, position) {
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
            stroke.extend(
                Sample {
                    point,
                    pressure,
                    timestamp_ms,
                },
                &mut self.commands,
            );
        }
    }

    fn end_stroke(&mut self) {
        self.rag_gesture = None;
        if let Some(stroke) = self.stroke.take() {
            stroke.finish(&mut self.commands);
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
                    if self
                        .rag_gesture
                        .is_some_and(|(source, _)| source == Source::Pen)
                    {
                        self.rag_gesture = None;
                    }
                    if let Some(point) = self
                        .stroke
                        .as_ref()
                        .and_then(|stroke| self.to_surface(stroke.surface, pos))
                    {
                        if let Some(stroke) =
                            self.stroke.as_mut().filter(|s| s.source == Source::Pen)
                        {
                            stroke.extend_release(
                                Sample {
                                    point,
                                    pressure: sample.pressure,
                                    timestamp_ms: sample.timestamp_ms,
                                },
                                &mut self.commands,
                            );
                        }
                    }
                    if self
                        .stroke
                        .as_ref()
                        .is_some_and(|s| s.source == Source::Pen)
                    {
                        self.end_stroke();
                    }
                }
                Phase::Cancel => {
                    if self
                        .rag_gesture
                        .is_some_and(|(source, _)| source == Source::Pen)
                    {
                        self.rag_gesture = None;
                    }
                    if self
                        .stroke
                        .as_ref()
                        .is_some_and(|s| s.source == Source::Pen)
                    {
                        self.end_stroke();
                    }
                }
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

    fn layout(&mut self, ctx: &egui::Context) {
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
                    .clicked()
                    || (can_undo && undo_shortcut)
                {
                    self.end_stroke();
                    if self.workbench.undo() {
                        self.commands.push(Command::Undo);
                    }
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
        egui::SidePanel::right("workbench")
            .resizable(false)
            .exact_width(304.)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.workbench_ui(ui);
                    });
            });

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
                } else if ui.rect_contains_pointer(canvas.intersect(room)) {
                    ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
                }
                self.canvas = Some(physical);
            });
        self.guide_ui(ctx);
        self.update_pen_regions();
    }

    fn workbench_ui(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.);
        ui.heading("Mixing palette");
        ui.add_space(4.);
        let width = ui.available_width();
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
                .selectable_label(self.workbench.sampling, "Save swatch")
                .on_hover_text("Then tap a color on the palette")
                .clicked()
            {
                self.workbench.sampling = !self.workbench.sampling;
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
            ui.label("Tap the mixture to save it.");
        }
        ui.horizontal_wrapped(|ui| {
            for (slot, occupied) in self.workbench.session.swatches.into_iter().enumerate() {
                if !occupied {
                    continue;
                }
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(20., 19.), egui::Sense::click());
                ui.painter().image(
                    self.texture_id,
                    rect,
                    gpu::atlas_uv(gpu::swatch_bounds(slot)),
                    Color32::WHITE,
                );
                if response.on_hover_text("Click to remove swatch").clicked() {
                    self.end_stroke();
                    self.snapshot();
                    self.workbench.session.swatches[slot] = false;
                    self.commands.push(Command::RemoveSwatch(slot));
                }
            }
        });
        if !self.workbench_status.is_empty() {
            ui.label(&self.workbench_status);
        }

        ui.separator();
        ui.strong("Paint jars");
        for (i, name) in pigment::NAMES.iter().enumerate() {
            ui.horizontal(|ui| {
                let (jar, icon) =
                    ui.allocate_exact_size(egui::vec2(20., 18.), egui::Sense::click());
                let lid = Rect::from_min_size(jar.min + egui::vec2(3., 1.), egui::vec2(14., 4.));
                let body =
                    Rect::from_min_max(jar.min + egui::vec2(1., 5.), jar.max - egui::vec2(1., 1.));
                ui.painter().rect_filled(lid, 1., Color32::from_gray(140));
                ui.painter().rect_filled(body, 3., self.swatches[i]);
                let dip = ui
                    .small_button(*name)
                    .on_hover_text("Dip the active brush into this jar for a full load");
                if icon.on_hover_text(format!("Dip into {name}")).clicked() || dip.clicked() {
                    self.service_brush(BrushService::Dip(i));
                    self.ink_selected = false;
                }
                if ui
                    .add_enabled(!self.placement_pending, egui::Button::new("Dab").small())
                    .on_hover_text(format!("Put a dab of {name} on the mixing palette"))
                    .clicked()
                {
                    self.place_tubes([i as u32 + 1, 0, 0, 0]);
                }
                if ui
                    .small_button("Info")
                    .on_hover_text(format!("About {name}"))
                    .clicked()
                {
                    self.workbench.exercise = false;
                    self.workbench.guide = Some(i);
                }
            });
        }
        ui.separator();
        ui.strong("Brush rack");
        ui.horizontal(|ui| {
            self.brush_card(ui, Brush::Round, "Round");
            self.brush_card(ui, Brush::Flat, "Flat");
            if ui
                .selectable_label(self.ink_selected, "Ink Pen")
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
                let (rag, _) = ui.allocate_exact_size(egui::vec2(63., 25.), egui::Sense::hover());
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
                if ui.button("Rinse cup").clicked()
                    && !self.workbench.session.rinsed[self.brush.index()]
                {
                    self.end_stroke();
                    self.snapshot();
                    self.workbench.session.rinsed[self.brush.index()] = true;
                    self.workbench.flick = Some((self.brush.index(), Instant::now()));
                }
                if ui.button("Clean brush").clicked() {
                    self.service_brush(BrushService::Wipe(0.));
                }
            });
            if self.workbench.session.rinsed[self.brush.index()] {
                ui.label("Swipe the rag to blot the brush.");
            }
        }

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
        let (rect, response) = ui.allocate_exact_size(egui::vec2(102., 35.), egui::Sense::click());
        let color = if !self.ink_selected && self.brush == brush {
            Color32::from_rgb(49, 73, 70)
        } else {
            Color32::from_gray(43)
        };
        ui.painter().rect_filled(rect, 5., color);
        let index = brush.index();
        let flick = self
            .workbench
            .flick
            .filter(|(owner, time)| *owner == index && time.elapsed() < Duration::from_millis(420));
        let offset = flick.map_or(0., |(_, time)| {
            let t = time.elapsed().as_secs_f32() / 0.42;
            (t * 25.).sin() * (1. - t) * 5.
        });
        let y = rect.center().y + offset;
        let x = rect.left() + 7.;
        let handle = Rect::from_min_size(egui::pos2(x, y - 2.5), egui::vec2(15., 5.));
        ui.painter()
            .rect_filled(handle, 2., Color32::from_rgb(163, 110, 72));
        let ferrule = Rect::from_min_size(egui::pos2(x + 13., y - 4.), egui::vec2(7., 8.));
        ui.painter()
            .rect_filled(ferrule, 1., Color32::from_gray(158));
        let tip_height = if brush == Brush::Round { 9. } else { 15. };
        let tip = Rect::from_min_size(
            egui::pos2(x + 20., y - tip_height / 2.),
            egui::vec2(13., tip_height),
        );
        ui.painter().rect_filled(
            tip,
            if index == 0 { 4. } else { 1. },
            Color32::from_rgb(114, 95, 80),
        );
        let count = if index == 0 { 1 } else { 3 };
        for group in 0..count {
            let lane = if index == 0 { 0 } else { 1 + group };
            let height = tip.height() / count as f32;
            let area = Rect::from_min_size(
                egui::pos2(tip.left() + 1., tip.top() + group as f32 * height + 0.6),
                egui::vec2(tip.width() - 2., height - 1.2),
            );
            ui.painter().image(
                self.texture_id,
                area,
                gpu::atlas_uv([512 + lane, SIDE, 1, 1]),
                Color32::WHITE,
            );
        }
        ui.painter().text(
            egui::pos2(rect.left() + 50., rect.center().y),
            egui::Align2::LEFT_CENTER,
            name,
            egui::FontId::proportional(13.),
            Color32::WHITE,
        );
        if response
            .on_hover_text("Paint on the tip shows color and load")
            .clicked()
            && (self.ink_selected || self.brush != brush)
        {
            self.end_stroke();
            self.brush = brush;
            self.ink_selected = false;
        }
    }

    fn guide_ui(&mut self, ctx: &egui::Context) {
        self.overlay = None;
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
            self.overlay = area.map(|area| physical_rect(ctx, area.response.rect));
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
                    ui.label("2. Use a clean brush to add equal small amounts of blue to separate dabs of white. Save a swatch of each.");
                    ui.label("3. Adjust the mixes until they look equally light. Compare how much white each needed.");
                    ui.label("4. Repeat one mix with a wiped brush, then with a rinsed and blotted brush. Compare the swatches.");
                    if ui.add_enabled(!self.placement_pending, egui::Button::new("Set out the four dabs")).clicked() {
                        self.place_tubes([5, 4, 2, 4]);
                    }
                });
            self.overlay = area.map(|area| physical_rect(ctx, area.response.rect));
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
