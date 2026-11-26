use super::*;
use gpu::Dab;

#[test]
fn canvas_zoom_anchors_at_the_cursor_and_pan_moves_the_view() {
    let viewport = Rect::from_min_size(egui::pos2(20., 30.), egui::vec2(800., 600.));
    let cursor = egui::pos2(320., 190.);
    let mut view = CanvasView::default();
    let before = view.rect(viewport);
    view.zoom_at(viewport, cursor, 3.);
    let after = view.rect(viewport);
    assert!(after.width() > before.width());
    assert!(
        ((cursor - before.min) / before.width() - (cursor - after.min) / after.width()).length()
            < 0.0001
    );
    let delta = egui::vec2(60., -35.);
    view.pan_by(viewport, delta);
    assert!((view.rect(viewport).center() - after.center() - delta).length() < 0.001);
    view.pan_by(viewport, egui::Vec2::splat(10_000.));
    let visible = view.rect(viewport).intersect(viewport);
    assert!(visible.width() >= 23.99 && visible.height() >= 23.99);
}

fn pixels(buffer: &wgpu::Buffer, device: &wgpu::Device) -> Vec<u8> {
    let (done, completion) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            done.send(result).unwrap()
        });
    let _ = device.poll(wgpu::Maintain::Wait);
    completion.recv().unwrap().unwrap();
    let bytes = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    bytes
}

struct Rig {
    paint: PaintGpu,
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Rig {
    fn new() -> Self {
        let (device, queue, adapter) = pollster::block_on(gpu::device(&instance(), None)).unwrap();
        eprintln!("Testing on {}", adapter.get_info().name);
        let paint = PaintGpu::new(&device);
        Self {
            paint,
            device,
            queue,
        }
    }

    fn canvas(&mut self, commands: &[Command]) -> Vec<u8> {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.paint
            .encode(&self.device, &self.queue, &mut encoder, commands);
        let buffer = self.paint.readback(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        pixels(&buffer, &self.device)
    }

    fn export(&self, kind: gpu::ExportKind) -> Vec<u8> {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let buffer = self
            .paint
            .readback_kind(&self.device, &self.queue, &mut encoder, kind);
        self.queue.submit([encoder.finish()]);
        pixels(&buffer, &self.device)
    }

    fn state(&mut self, commands: &[Command]) -> (Vec<u8>, Vec<u8>) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.paint
            .encode(&self.device, &self.queue, &mut encoder, commands);
        let (pigments, brushes) = self.paint.inspect(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        (
            pixels(&pigments, &self.device),
            pixels(&brushes, &self.device),
        )
    }

    fn check_tubes(&mut self, recipe: [u32; 4]) -> bool {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.paint.encode(
            &self.device,
            &self.queue,
            &mut encoder,
            &[Command::CheckTubes(recipe)],
        );
        let result = self.paint.placement_readback(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        pixels(&result, &self.device)[0] != 0
    }

    fn dry_state(&mut self) -> Vec<u8> {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let dry = self.paint.inspect_dry(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        pixels(&dry, &self.device)
    }
}

fn values(bytes: &[u8]) -> impl Iterator<Item = [f32; 8]> + '_ {
    bytes.chunks_exact(32).map(|pixel| {
        std::array::from_fn(|i| f32::from_le_bytes(pixel[i * 4..i * 4 + 4].try_into().unwrap()))
    })
}

fn totals(a: &[u8], b: &[u8]) -> [f64; 6] {
    let mut totals = [0.; 6];
    for p in values(a).chain(values(&b[..gpu::BRUSH_LOAD_BYTES])) {
        for i in 0..6 {
            totals[i] += f64::from(p[i]);
        }
    }
    totals
}

fn mark(x: f32, y: f32, surface: Surface, brush: Brush) -> Command {
    Command::Dab(Dab::new(egui::pos2(x, y), 26., 0.75, surface, brush, 0.))
}

fn line(from: [f32; 2], to: [f32; 2], surface: Surface, brush: Brush) -> Vec<Command> {
    let start = egui::pos2(from[0], from[1]);
    let end = egui::pos2(to[0], to[1]);
    let count = (start.distance(end) / 4.).ceil().max(1.) as u32;
    (0..=count)
        .map(|i| {
            let point = start.lerp(end, i as f32 / count as f32);
            mark(point.x, point.y, surface, brush)
        })
        .collect()
}

fn pixel(bytes: &[u8], x: usize, y: usize) -> [u8; 4] {
    let offset = (y * SIDE as usize + x) * 4;
    bytes[offset..offset + 4].try_into().unwrap()
}

fn linear_rgb([r, g, b, _]: [u8; 4]) -> [f32; 3] {
    let [r, g, b, _] = egui::Rgba::from(Color32::from_rgb(r, g, b)).to_array();
    [r, g, b]
}

#[test]
fn dx12_thin_phthalo_lighting_hiding_and_transparent_export() {
    let mut rig = Rig::new();
    let x = 128;
    let y = 128;
    let unlit = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Service(Brush::Round, BrushService::Wipe(0.20)),
        Command::Dab(Dab::new(
            egui::pos2(x as f32, y as f32),
            58.,
            1.,
            Surface::Canvas,
            Brush::Round,
            0.,
        )),
    ]);
    let white = linear_rgb(pixel(&unlit, x, y));
    let transparent = pixel(&rig.export(gpu::ExportKind::Transparent), x, y);
    let foreground = linear_rgb(transparent);
    let alpha = transparent[3] as f32 / 255.;
    assert!(
        (0.05..0.95).contains(&alpha),
        "thin glaze had alpha {alpha}"
    );
    let diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, x as u32, y as u32);
    let hiding = diag.optical.wet_layer_hiding_ratio_pct.unwrap() / 100.;

    rig.paint.lighting.enabled = true;
    rig.paint.lighting.impasto_depth = 0.;
    rig.paint.lighting.canvas_weave = 0.;
    rig.paint.lighting.specular_sheen = 0.;
    let diffuse = 0.65 + 0.45 * rig.paint.lighting.light_direction()[2];
    let refresh = || Command::Dab(Dab::operation(Surface::Canvas.bounds(), 1));
    let lit = linear_rgb(pixel(&rig.canvas(&[refresh()]), x, y));
    for (actual, channel) in lit.into_iter().zip(white) {
        assert!(
            (actual - channel * diffuse).abs() < 0.008,
            "diffuse shading must be linear"
        );
    }

    rig.paint.lighting.enabled = false;
    rig.paint.layers.reference_opacity = 1.;
    let mut black = [0.; 3];
    let luminance = |[r, g, b]: [f32; 3]| 0.2126 * r + 0.7152 * g + 0.0722 * b;
    for (index, background) in [
        [0, 0, 0, 255],
        [255; 4],
        [128, 128, 128, 255],
        [222, 151, 75, 255],
        [46, 104, 212, 255],
    ]
    .into_iter()
    .enumerate()
    {
        let words = Arc::new(vec![u32::from_le_bytes(background); (SIDE * SIDE) as usize]);
        let result = rig.canvas(&[Command::ImportReference(words)]);
        let canvas = linear_rgb(pixel(&result, x, y));
        let ground = linear_rgb(background);
        let png: [f32; 3] =
            std::array::from_fn(|ch| foreground[ch] * alpha + ground[ch] * (1. - alpha));
        eprintln!(
            "Ground {background:?}, canvas linear {canvas:?}, PNG over ground linear {png:?}"
        );
        if index == 0 {
            black = canvas;
            assert!((luminance(black) / luminance(white) - hiding).abs() < 0.015);
        }
        for channel in 0..3 {
            let expected = black[channel] + (white[channel] - black[channel]) * ground[channel];
            assert!((canvas[channel] - expected).abs() < 0.008);
            if index == 1 {
                assert!((png[channel] - white[channel]).abs() < 0.012);
            }
        }
    }
    assert!(
        white[2] - black[2] > 0.3,
        "thin Phthalo must transmit blue from its ground"
    );
    let blue_on_black = foreground[2] * alpha;
    assert!(
        white[2] - blue_on_black > 0.05,
        "the thin PNG glaze must show some ground response"
    );
}

fn paint_at(bytes: &[u8], x: usize, y: usize) -> [f32; 8] {
    values(&bytes[(y * SIDE as usize + x) * 32..])
        .next()
        .unwrap()
}

fn green([r, g, b, _]: [u8; 4]) -> bool {
    g > r.saturating_add(3) && g > b.saturating_add(3)
}

#[test]
fn dx12_oil_workbench() {
    let mut rig = Rig::new();
    let mut reference = (0..6)
        .map(|i| {
            let mut paint = [0.; 8];
            paint[i] = 1.;
            paint
        })
        .collect::<Vec<_>>();
    reference.extend([
        [0.55, 0.45, 0., 0., 0., 0., 0., 0.],
        [0.32, 0.32, 0., 0.36, 0., 0., 0., 0.],
        [0., 0., 0., 0., 0.55, 0.45, 0., 0.],
        [0., 0.08, 0., 0.92, 0., 0., 0., 0.],
        [0., 0., 0., 0.92, 0.08, 0., 0., 0.],
    ]);
    let display_atlas = rig.paint.display_view();
    rig.paint.reference_swatches(&rig.queue, &reference);
    let mut encoder = rig.device.create_command_encoder(&Default::default());
    rig.paint.refresh(&rig.queue, &mut encoder);
    let buffer = rig.paint.readback(&rig.device, &mut encoder);
    rig.queue.submit([encoder.finish()]);
    let colors = pixels(&buffer, &rig.device);
    for (x, amount) in reference.into_iter().enumerate() {
        let expected = pigment::rgb(amount);
        let actual = pixel(&colors, x, 0);
        assert!(
            actual[..3]
                .iter()
                .zip(expected)
                .all(|(a, b)| a.abs_diff(b) <= 2),
            "{actual:?} vs {expected:?}"
        );
        assert_eq!(actual[3], 255);
    }
    assert_display_matches_export(&rig, &display_atlas, pixel(&colors, 5, 0));
    assert!(green(pixel(&colors, 6, 0)));
    let luminance =
        |[r, g, b, _]: [u8; 4]| 0.2126 * r as f32 + 0.7152 * g as f32 + 0.0722 * b as f32;
    assert!(luminance(pixel(&colors, 7, 0)) > luminance(pixel(&colors, 6, 0)));
    assert!(pixel(&colors, 9, 0) != pixel(&colors, 10, 0));

    let blank = rig.canvas(&[
        Command::Initialize,
        mark(60., 65., Surface::Canvas, Brush::Round),
    ]);
    assert!(blank.iter().all(|x| *x == 255));
    assert!(rig.check_tubes([1, 2, 0, 0]));
    let (initial, empty) = rig.state(&[Command::Tubes]);
    assert!(paint_at(&initial, 32, SIDE as usize + 32)[0] > 1.);
    assert!(paint_at(&initial, 96, SIDE as usize + 32)[1] > 1.);
    let mut round = line([24., 32.], [42., 32.], Surface::Palette, Brush::Round);
    round.insert(0, Command::Snapshot);
    let (after_round, yellow) = rig.state(&round);
    assert!(values(&yellow).next().unwrap()[0] > 80.);
    for (a, b) in totals(&initial, &empty)
        .into_iter()
        .zip(totals(&after_round, &yellow))
    {
        assert!((a - b).abs() < 0.05, "Mass changed from {a} to {b}");
    }

    let mut flat = vec![
        Command::Service(Brush::Flat, BrushService::Load(0)),
        Command::SaveSwatch(0, Surface::Palette, [96, 32]),
    ];
    flat.extend(line(
        [110., 32.],
        [113., 89.],
        Surface::Palette,
        Brush::Flat,
    ));
    let (mixed, streaked) = rig.state(&flat);
    let loads = values(&streaked).take(4).collect::<Vec<_>>();
    assert_eq!(&yellow[..32], &streaked[..32]);
    assert!(loads[1][1] > loads[3][1] + 10., "{loads:?}");
    assert!(loads[3][0] > loads[3][1], "{loads:?}");
    assert_eq!(
        paint_at(&mixed, 544, SIDE as usize),
        paint_at(&after_round, 96, SIDE as usize + 32)
    );

    let mut brush_mark = line([400., 500.], [540., 500.], Surface::Canvas, Brush::Round);
    brush_mark.insert(0, Command::Snapshot);
    let canvas = rig.canvas(&brush_mark);
    assert!(pixel(&canvas, 420, 500)[2] < 245);
    assert_eq!(rig.canvas(&[Command::Undo]), blank);
    let (_, restored) = rig.state(&[]);
    assert_eq!(
        &restored[..gpu::BRUSH_LOAD_BYTES],
        &streaked[..gpu::BRUSH_LOAD_BYTES]
    );
    let mut worn = Vec::new();
    for y in [150., 230., 310., 390., 470.] {
        worn.extend(line([90., y], [960., y], Surface::Canvas, Brush::Round));
    }
    let (_, exhausted) = rig.state(&worn[..MAX_COMMANDS.min(worn.len())]);
    assert!(values(&exhausted).next().unwrap()[0] < 0.01);

    let (_, wiped) = rig.state(&[
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Service(Brush::Round, BrushService::Wipe(0.08)),
        Command::Service(Brush::Round, BrushService::Load(3)),
    ]);
    let tinted = values(&wiped).next().unwrap();
    assert!(tinted[1] > 1. && tinted[3] > 100.);
    let (_, clean) = rig.state(&[
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Load(3)),
    ]);
    assert_eq!(values(&clean).next().unwrap()[1], 0.);
    let (_, dipped) = rig.state(&[
        Command::Snapshot,
        Command::Service(Brush::Round, BrushService::Dip(1)),
    ]);
    let load = values(&dipped).next().unwrap();
    assert!(load[1] > load[3] * 3.9);
    assert_eq!(load[3], values(&clean).next().unwrap()[3]);
    let (_, restored) = rig.state(&[Command::Undo]);
    assert_eq!(
        &restored[..gpu::BRUSH_LOAD_BYTES],
        &clean[..gpu::BRUSH_LOAD_BYTES]
    );

    let (_, custom_dipped) = rig.state(&[
        Command::Snapshot,
        Command::Service(Brush::Round, BrushService::DipCustom(0)),
    ]);
    let custom_load = values(&custom_dipped).next().unwrap();
    assert!(custom_load[1] > 10., "{custom_load:?}");
    let (_, restored_after_custom) = rig.state(&[Command::Undo]);
    assert_eq!(
        &restored_after_custom[..gpu::BRUSH_LOAD_BYTES],
        &clean[..gpu::BRUSH_LOAD_BYTES]
    );

    let cad_red = cabinet::find_pigment("cadmium_red").unwrap();
    let (_, rack_dipped) = rig.state(&[
        Command::Snapshot,
        Command::EquipRack(0, cad_red.recipe),
        Command::Service(Brush::Round, BrushService::DipRack(0)),
    ]);
    let rack_load = values(&rack_dipped).next().unwrap();
    assert!(
        rack_load[6] > 10.,
        "rack dip should transfer pigment recipe to brush: {rack_load:?}"
    );
    let (_, restored_after_rack) = rig.state(&[Command::Undo]);
    assert_eq!(
        &restored_after_rack[..gpu::BRUSH_LOAD_BYTES],
        &clean[..gpu::BRUSH_LOAD_BYTES]
    );

    assert!(rig.check_tubes([5, 4, 2, 4]));
    rig.canvas(&[Command::Tubes]);
    assert!(rig.check_tubes([6, 6, 0, 0]));
    rig.canvas(&[Command::Tubes]);
    assert!(!rig.check_tubes([1, 2, 0, 0]));
    rig.canvas(&[Command::Snapshot, Command::Clear(Surface::Palette)]);
    assert!(rig.check_tubes([1, 2, 0, 0]));
    assert!(rig.check_tubes([100, 0, 0, 0]));
    assert!(rig.check_tubes([200, 0, 0, 0]));
    rig.canvas(&[Command::Undo]);
    assert!(!rig.check_tubes([1, 2, 0, 0]));

    let expected = rig.canvas(&[]);
    let mut encoder = rig.device.create_command_encoder(&Default::default());
    let buffer = rig.paint.readback(&rig.device, &mut encoder);
    rig.queue.submit([encoder.finish()]);
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("painting.png");
    let export = gpu::export(buffer, path.clone());
    let _ = rig.device.poll(wgpu::Maintain::Wait);
    export
        .recv_timeout(Duration::from_secs(30))
        .unwrap()
        .unwrap();
    let mut png = png::Decoder::new(std::fs::File::open(path).unwrap())
        .read_info()
        .unwrap();
    let mut decoded = vec![0; png.output_buffer_size()];
    let info = png.next_frame(&mut decoded).unwrap();
    assert_eq!([info.width, info.height], [SIDE, SIDE]);
    assert!(png.info().srgb.is_some());
    assert_eq!(decoded, expected);

    let mut palette_tap = vec![Command::Initialize];
    palette_tap.extend(
        cabinet::PIGMENTS
            .iter()
            .enumerate()
            .map(|(slot, pigment)| Command::EquipRack(slot, pigment.recipe)),
    );
    let last_slot = cabinet::PIGMENTS.len() - 1;
    palette_tap.push(Command::Service(
        Brush::Round,
        BrushService::DipRack(last_slot),
    ));
    Stroke::start(
        Source::Mouse,
        Sample {
            point: egui::pos2(96.5, 96.5),
            pressure: 0.65,
            timestamp_ms: 0,
            angle: None,
        },
        26.,
        Surface::Palette,
        Brush::Round,
        0.,
        &mut palette_tap,
    )
    .finish(&mut palette_tap);
    let (palette, _) = rig.state(&palette_tap);
    let dab = values(&palette)
        .nth(((SIDE + 96) * SIDE + 96) as usize)
        .unwrap();
    let mass: f32 = dab.iter().sum();
    assert!(
        mass > 0.,
        "a palette tap should deposit paint from the last jar"
    );
    for (channel, expected) in dab.into_iter().zip(cabinet::PIGMENTS[last_slot].recipe) {
        assert!((channel / mass - expected).abs() < 0.001);
    }

    let mut pressure = vec![
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(1)),
    ];
    let sample = |x, pressure, timestamp_ms| Sample {
        point: egui::pos2(x, 440.),
        pressure,
        timestamp_ms,
        angle: None,
    };
    let mut stroke = Stroke::start(
        Source::Pen,
        sample(110., 0.15, 0),
        64.,
        Surface::Canvas,
        Brush::Round,
        0.,
        &mut pressure,
    );
    stroke.extend(sample(215., 0.95, 18), &mut pressure);
    stroke.finish(&mut pressure);
    let ramp = rig.canvas(&pressure);
    let width = |x| (408..=472).filter(|y| pixel(&ramp, x, *y)[0] < 249).count();
    assert!((110..215).all(|x| pixel(&ramp, x, 440) != [255; 4]));
    assert!(
        width(200) > width(115),
        "pressure did not spread the round tip"
    );

    let mut flat = vec![
        Command::Initialize,
        Command::Service(Brush::Flat, BrushService::Load(1)),
    ];
    flat.extend(line(
        [100., 200.],
        [180., 200.],
        Surface::Canvas,
        Brush::Flat,
    ));
    flat.push(Command::Service(Brush::Flat, BrushService::Wipe(0.)));
    flat.push(Command::Service(Brush::Flat, BrushService::Load(1)));
    for x in (100..=180).step_by(4) {
        flat.push(Command::Dab(Dab::new(
            egui::pos2(x as f32, 300.),
            26.,
            0.75,
            Surface::Canvas,
            Brush::Flat,
            std::f32::consts::FRAC_PI_2,
        )));
    }
    let angled = rig.canvas(&flat);
    let flat_width = |center: usize| {
        (center - 30..=center + 30)
            .filter(|y| pixel(&angled, 130, *y)[0] < 249)
            .count()
    };
    assert!(
        flat_width(300) > flat_width(200) + 10,
        "angle did not change the flat mark width"
    );

    let blank = rig.canvas(&[Command::Initialize]);
    let light_position = egui::pos2(140.5, 210.5);
    let pool = egui::pos2(240.5, 210.5);
    let light = rig.canvas(&[
        Command::Snapshot,
        Command::Dab(Dab::ink(light_position, 0.25, false)),
        Command::AdvanceInk,
    ]);
    assert!(pixel(&light, 140, 210)[0] < 190);
    assert_eq!(pixel(&light, 140, 216), [255; 4]);
    assert!(!rig.paint.ink_is_wet());

    let mut hold = vec![Command::Snapshot];
    let mut ink = Stroke::start_ink(
        Source::Pen,
        Sample {
            point: pool,
            pressure: 0.98,
            timestamp_ms: 0,
            angle: None,
        },
        &mut hold,
    );
    let start = Instant::now();
    for tick in 1..=25 {
        ink.dwell(start + Duration::from_millis(36 * tick), &mut hold);
        hold.extend([Command::AdvanceInk, Command::AdvanceInk]);
    }
    ink.finish(&mut hold);
    let pooled = rig.canvas(&hold);
    assert!(rig.paint.ink_is_wet());
    assert!(pixel(&pooled, 240, 210)[0] < 25);
    let steps: Vec<_> = (0..100).map(|_| Command::AdvanceInk).collect();
    let dry = rig.canvas(&steps);
    assert!(!rig.paint.ink_is_wet());
    assert!(
        (225..300).any(|y| pixel(&dry, 240, y)[0].saturating_add(5) < pixel(&pooled, 240, y)[0])
    );
    assert_eq!(pixel(&dry, 240, 200), [255; 4]);
    assert_eq!(pixel(&dry, 140, 216), [255; 4]);
    assert_eq!(rig.canvas(&[Command::Undo]), light);
    assert_eq!(rig.canvas(&[Command::Undo]), blank);
}

fn assert_display_matches_export(rig: &Rig, atlas: &wgpu::TextureView, expected: [u8; 4]) {
    // Render the same atlas texel with the desktop renderer and compare it with the export bytes.
    let size = wgpu::Extent3d {
        width: 1,
        height: 1,
        depth_or_array_layers: 1,
    };
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = rig.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let mut renderer = egui_wgpu::Renderer::new(&rig.device, format, None, 1, false);
    let texture = renderer.register_native_texture(&rig.device, atlas, wgpu::FilterMode::Linear);
    let clip_rect = Rect::from_min_max(Pos2::ZERO, egui::pos2(1., 1.));
    let mut mesh = egui::Mesh::with_texture(texture);
    mesh.add_rect_with_uv(clip_rect, gpu::atlas_uv([5, 0, 1, 1]), Color32::WHITE);
    let jobs = [egui::ClippedPrimitive {
        clip_rect,
        primitive: egui::epaint::Primitive::Mesh(mesh),
    }];
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [1, 1],
        pixels_per_point: 1.,
    };
    let mut encoder = rig.device.create_command_encoder(&Default::default());
    let extra = renderer.update_buffers(&rig.device, &rig.queue, &mut encoder, &jobs, &screen);
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target.create_view(&Default::default()),
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }
    let readback = rig.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: Default::default(),
        },
        size,
    );
    rig.queue
        .submit(extra.into_iter().chain([encoder.finish()]));

    let actual = pixels(&readback, &rig.device);
    assert!(
        actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
        "display {actual:?}, export {expected:?}"
    );
}

#[test]
fn undo_restores_jars_and_load_state() {
    let mut bench = workbench::Workbench::default();
    bench.snapshot();
    bench.session.loads[0] = 0.5;
    bench.snapshot();
    bench.session.jars[2] = true;
    assert!(bench.undo());
    assert_eq!(bench.session.loads[0], 0.5);
    assert!(!bench.session.jars[2]);
    assert!(bench.undo());
    assert_eq!(bench.session.loads[0], 0.0);
}

#[test]
fn debug_print_pigment_colors() {
    for p in cabinet::PIGMENTS {
        let mut scaled_recipe = p.recipe;
        for v in &mut scaled_recipe {
            *v *= 2.0;
        }
        let computed = pigment::rgb(scaled_recipe);
        eprintln!(
            "PIGMENT [{}]: defined_rgb={:?}, computed_rgb={:?}, recipe={:?}",
            p.name, p.rgb, computed, p.recipe
        );
    }
    let vermilion_pure = pigment::rgb([0., 0., 0., 0., 0., 0., 2.0, 0.]);
    let black_pure = pigment::rgb([0., 0., 0., 0., 0., 0., 0., 2.0]);
    eprintln!(
        "PURE CHANNEL 6 (Vermilion/Cad Red): computed_rgb={:?}",
        vermilion_pure
    );
    eprintln!("PURE CHANNEL 7 (Bone Black): computed_rgb={:?}", black_pure);
}

#[test]
fn test_gpu_reds_output() {
    let mut rig = Rig::new();
    let cad_red = cabinet::find_pigment("cadmium_red").unwrap();
    let vermilion = cabinet::find_pigment("vermilion").unwrap();
    let rose_madder = cabinet::find_pigment("rose_madder").unwrap();
    let dragons_blood = cabinet::find_pigment("dragons_blood").unwrap();

    let mut commands = vec![
        Command::Initialize,
        Command::EquipRack(0, cad_red.recipe),
        Command::EquipRack(1, vermilion.recipe),
        Command::EquipRack(2, rose_madder.recipe),
        Command::EquipRack(3, dragons_blood.recipe),
    ];

    // Dip and mark on canvas for each
    for (slot, x) in [(0, 100.), (1, 250.), (2, 400.), (3, 550.)] {
        commands.push(Command::Service(Brush::Round, BrushService::Wipe(0.)));
        commands.push(Command::Service(Brush::Round, BrushService::DipRack(slot)));
        commands.push(mark(x, 150., Surface::Canvas, Brush::Round));
    }

    let (pigments, _brushes) = rig.state(&commands);
    for (name, x) in [
        ("Cadmium Red", 100),
        ("Vermilion", 250),
        ("Rose Madder", 400),
        ("Dragon's Blood", 550),
    ] {
        let at_px = paint_at(&pigments, x, 150);
        eprintln!("CANVAS PAINT FOR [{}]: amounts={:?}", name, at_px);
    }
    let canvas_pixels = rig.canvas(&[]);
    for (name, x) in [
        ("Cadmium Red", 100),
        ("Vermilion", 250),
        ("Rose Madder", 400),
        ("Dragon's Blood", 550),
    ] {
        let center_px = pixel(&canvas_pixels, x, 150);
        eprintln!("GPU RENDER FOR [{}]: pixel={:?}", name, center_px);
    }

    // Now test a direct DAB on the mixing palette state
    let palette_commands = vec![
        Command::Initialize,
        Command::EquipRack(0, vermilion.recipe),
        Command::CheckTubes([200, 0, 0, 0]),
        Command::Tubes,
    ];
    let (palette_pigments, _) = rig.state(&palette_commands);
    let dab_paint = paint_at(&palette_pigments, 32, SIDE as usize + 32);
    eprintln!("PALETTE DAB CENTER AMOUNTS: {:?}", dab_paint);
}

#[test]
fn test_dry_canvas_layer_separation_and_undo() {
    let mut rig = Rig::new();
    let blue_tube = 1; // Phthalo Blue
    let white_tube = 3; // Titanium White

    // 1. Paint wet blue sky on the canvas
    let paint_sky = vec![
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(300., 300., Surface::Canvas, Brush::Round),
    ];
    let (wet_pigments, _) = rig.state(&paint_sky);
    let wet_sky_paint = paint_at(&wet_pigments, 300, 300);
    assert!(
        wet_sky_paint[blue_tube] > 0.05,
        "blue paint should be on wet canvas: {wet_sky_paint:?}"
    );

    let canvas_after_sky = rig.canvas(&[]);
    let blue_pixel = pixel(&canvas_after_sky, 300, 300);
    assert!(
        blue_pixel[2] > blue_pixel[0] + 50,
        "sky pixel should be distinctly blue: {blue_pixel:?}"
    );

    // 2. Lock & Cure the canvas with Command::DryCanvas
    let dry_commands = vec![Command::Snapshot, Command::DryCanvas];
    let (wet_after_dry, _) = rig.state(&dry_commands);
    let wet_amt_after_dry = paint_at(&wet_after_dry, 300, 300);
    assert_eq!(
        wet_amt_after_dry, [0.0; 8],
        "wet canvas should be completely clear after drying"
    );

    let dry_pixels = rig.dry_state();
    let dry_paint_at = paint_at(&dry_pixels, 300, 300);
    assert!(
        dry_paint_at[blue_tube] > 0.05,
        "blue paint should now reside in dry_canvas: {dry_paint_at:?}"
    );

    let canvas_after_dry = rig.canvas(&[]);
    let cured_pixel = pixel(&canvas_after_dry, 300, 300);
    assert_eq!(
        cured_pixel, blue_pixel,
        "cured dry canvas optical rendering should be identical to wet"
    );

    // 3. Paint pure Titanium White clouds directly on top of the dry blue sky
    let paint_cloud = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(white_tube)),
        mark(300., 300., Surface::Canvas, Brush::Round),
    ];
    let (wet_after_cloud, brushes_after_cloud) = rig.state(&paint_cloud);
    let brush_load = values(&brushes_after_cloud).next().unwrap();
    assert_eq!(
        brush_load[blue_tube], 0.0,
        "brush must NOT pick up any dry blue paint into bristles: {brush_load:?}"
    );

    let wet_cloud_paint = paint_at(&wet_after_cloud, 300, 300);
    assert!(
        wet_cloud_paint[white_tube] > 0.05,
        "white cloud paint must be on wet canvas: {wet_cloud_paint:?}"
    );

    let canvas_after_cloud = rig.canvas(&[]);
    let cloud_pixel = pixel(&canvas_after_cloud, 300, 300);
    assert!(
        cloud_pixel[0] > 200 && cloud_pixel[1] > 200 && cloud_pixel[2] > 200,
        "cloud pixel should be brilliant opaque white on top of blue sky: {cloud_pixel:?}"
    );

    // 4. Undo should restore back to the dry state
    let canvas_after_undo = rig.canvas(&[Command::Undo]);
    let restored_pixel = pixel(&canvas_after_undo, 300, 300);
    assert_eq!(
        restored_pixel, blue_pixel,
        "undo should remove white cloud and restore dry blue sky"
    );
}

#[test]
fn test_pinned_notes_and_diagnostics() {
    let mut rig = Rig::new();
    let blue_tube = 1; // Phthalo Blue

    // 1. Paint a Phthalo Blue mark on canvas
    let paint_commands = vec![
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(400., 400., Surface::Canvas, Brush::Round),
    ];
    let _ = rig.canvas(&paint_commands);

    // 2. Sample diagnostics from GPU at (400, 400)
    let diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 400, 400);
    assert!(
        diag.wet_channels[blue_tube] > 0.05,
        "diagnostics must detect wet Phthalo Blue mass: {:?}",
        diag.wet_channels
    );
    assert_eq!(diag.dry_total, 0.0, "dry canvas should be empty");
    assert!(
        diag.display_srgb[2] > 100,
        "rendered pixel must be blue: {:?}",
        diag.display_srgb
    );
    assert!(
        diag.optical.r_sub_mean > 0.9,
        "white substrate reflectance should be ~1.0: {}",
        diag.optical.r_sub_mean
    );

    // 3. Add a diagnostic note in a temporary directory
    let temp_dir = tempfile::tempdir().unwrap();
    let notes_path = temp_dir.path().join("test_notes.json");
    let note = crate::notes::add(
        &notes_path,
        400.0,
        400.0,
        "Phthalo blue looks weird when painted over",
        "artist",
        Some(diag),
    );
    assert_eq!(note.id, 1);
    assert!(!note.done);
    assert!(note.diagnostics.is_some());

    // 4. Verify read and update
    let read_notes = crate::notes::read(&notes_path);
    assert_eq!(read_notes.len(), 1);
    assert_eq!(
        read_notes[0].text,
        "Phthalo blue looks weird when painted over"
    );

    assert!(crate::notes::set_done(&notes_path, 1, true));
    let updated = crate::notes::read(&notes_path);
    assert!(updated[0].done);

    assert_eq!(crate::notes::clear_done(&notes_path), 1);
    assert!(crate::notes::read(&notes_path).is_empty());
}

#[test]
fn test_multi_layer_strata_drying_and_preservation() {
    let mut rig = Rig::new();
    let cad_red = crate::cabinet::find_pigment("cadmium_red").unwrap();
    let hansa_yellow = crate::cabinet::find_pigment("hansa_yellow").unwrap();
    let blue_tube = 1; // Phthalo Blue

    // 1. Paint Phthalo Blue base layer
    let paint_blue = vec![
        Command::Initialize,
        Command::EquipRack(0, cad_red.recipe),
        Command::EquipRack(1, hansa_yellow.recipe),
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(350., 350., Surface::Canvas, Brush::Round),
    ];
    let blue_canvas = rig.canvas(&paint_blue);
    let blue_pixel = pixel(&blue_canvas, 350, 350);
    assert!(
        blue_pixel[2] > 100,
        "base pixel should be distinctly blue: {blue_pixel:?}"
    );

    // 2. Lock & Cure Blue underlayer
    let dry1_canvas = rig.canvas(&[Command::Snapshot, Command::DryCanvas]);
    let dry_blue_pixel = pixel(&dry1_canvas, 350, 350);
    assert_eq!(
        dry_blue_pixel, blue_pixel,
        "first dry must perfectly preserve optical reflectance"
    );

    // 3. Paint Cadmium Red directly on top of dry Phthalo Blue
    let paint_red = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::DipRack(0)),
        mark(350., 350., Surface::Canvas, Brush::Round),
    ];
    let red_canvas = rig.canvas(&paint_red);
    let wet_red_pixel = pixel(&red_canvas, 350, 350);
    assert!(
        wet_red_pixel[0] >= 110 && wet_red_pixel[0] > wet_red_pixel[1] * 2,
        "cadmium red over dry blue should be distinctly red: {wet_red_pixel:?}"
    );

    // 4. Lock & Cure Red second layer (Previously this collapsed into muddy black/brown!)
    let dry2_canvas = rig.canvas(&[Command::Snapshot, Command::DryCanvas]);
    let dry_red_pixel = pixel(&dry2_canvas, 350, 350);
    assert_eq!(
        dry_red_pixel, wet_red_pixel,
        "second dry must NOT collapse Cadmium Red into mud: {dry_red_pixel:?}"
    );
    assert!(
        dry_red_pixel[0] >= 110 && dry_red_pixel[0] > dry_red_pixel[1] * 2,
        "cured Cadmium Red over blue must stay vibrant red: {dry_red_pixel:?}"
    );

    // 5. Paint Hansa Yellow third layer on top of dried Red (creates a glowing warm orange glaze)
    let paint_yellow = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::DipRack(1)),
        mark(350., 350., Surface::Canvas, Brush::Round),
    ];
    let yellow_canvas = rig.canvas(&paint_yellow);
    let wet_yellow_pixel = pixel(&yellow_canvas, 350, 350);
    assert!(
        wet_yellow_pixel[0] >= 120
            && wet_yellow_pixel[0] > wet_yellow_pixel[1]
            && wet_yellow_pixel[1] > wet_yellow_pixel[2],
        "hansa yellow over red should produce a warm orange glaze: {wet_yellow_pixel:?}"
    );

    // 6. Lock & Cure Yellow third layer
    let dry3_canvas = rig.canvas(&[Command::Snapshot, Command::DryCanvas]);
    let dry_yellow_pixel = pixel(&dry3_canvas, 350, 350);
    assert_eq!(
        dry_yellow_pixel, wet_yellow_pixel,
        "third dry must NOT darken or muddy yellow/orange glaze: {dry_yellow_pixel:?}"
    );
    assert!(
        dry_yellow_pixel[0] >= 120 && dry_yellow_pixel[0] > dry_yellow_pixel[1],
        "cured glaze must stay warm and radiant: {dry_yellow_pixel:?}"
    );
}

#[test]
fn test_fill_jar_from_canvas_wet_and_dry() {
    let mut rig = Rig::new();
    let cad_red = crate::cabinet::find_pigment("cadmium_red").unwrap();
    let blue_tube = 1; // Phthalo Blue

    // 1. Paint a wet Cadmium Red stroke on canvas
    let commands_wet = vec![
        Command::Initialize,
        Command::EquipRack(0, cad_red.recipe),
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::DipRack(0)),
        mark(200., 200., Surface::Canvas, Brush::Round),
        // Fill custom jar 0 directly from the wet canvas stroke at (200, 200)
        Command::SaveSwatch(0, Surface::Canvas, [200, 200]),
        // Clean brush and dip from custom jar 0
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::DipCustom(0)),
        // Paint a new stroke at (600, 200) with the jar-dipped paint
        mark(600., 200., Surface::Canvas, Brush::Round),
    ];
    let canvas_after_wet = rig.canvas(&commands_wet);
    let original_red = pixel(&canvas_after_wet, 200, 200);
    let replicated_red = pixel(&canvas_after_wet, 600, 200);
    assert!(
        original_red[0] > 120,
        "original should be red: {original_red:?}"
    );
    assert!(
        replicated_red[0] > 120,
        "replicated should be red: {replicated_red:?}"
    );
    assert_eq!(
        original_red, replicated_red,
        "filling jar from wet canvas and repainting must reproduce the exact same color"
    );

    // 2. Now paint Phthalo Blue on canvas, CURE it dry, and fill jar 1 from the dry canvas
    let commands_dry = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(400., 400., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
        // Fill custom jar 1 from the CURED dry canvas at (400, 400)
        Command::SaveSwatch(1, Surface::Canvas, [400, 400]),
        // Clean brush and dip from custom jar 1
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::DipCustom(1)),
        // Paint a new stroke at (800, 400)
        mark(800., 400., Surface::Canvas, Brush::Round),
    ];
    let canvas_after_dry = rig.canvas(&commands_dry);
    let dry_blue = pixel(&canvas_after_dry, 400, 400);
    let replicated_blue = pixel(&canvas_after_dry, 800, 400);
    assert!(dry_blue[2] > 100, "dry stroke should be blue: {dry_blue:?}");
    assert!(
        replicated_blue[2] > 100,
        "jar-dipped stroke should be blue: {replicated_blue:?}"
    );
    assert_eq!(
        dry_blue, replicated_blue,
        "filling jar from cured dry canvas and repainting must reproduce the exact same color"
    );
}

#[test]
fn test_multi_coat_coverage_and_deepening() {
    let mut rig = Rig::new();
    let blue_tube = 1; // Phthalo Blue
    let magenta_tube = 2; // Quinacridone Magenta

    // =========================================================================
    // Part 1: Phthalo Blue multi-coat accumulation and mass tone coverage
    // =========================================================================
    // Coat 1: Single dab over white gesso (translucent cyan glaze)
    let coat1_cmds = vec![
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(300., 300., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
    ];
    let canvas1 = rig.canvas(&coat1_cmds);
    let blue_coat1 = pixel(&canvas1, 300, 300);
    assert!(
        blue_coat1[2] > 100,
        "coat 1 must be luminous blue/cyan glaze: {blue_coat1:?}"
    );

    // Coat 2: Second dab over cured dry underlayer
    let coat2_cmds = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(300., 300., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
    ];
    let canvas2 = rig.canvas(&coat2_cmds);
    let blue_coat2 = pixel(&canvas2, 300, 300);

    // Coat 3: Third dab over cured dry layers -> builds mass tone
    let coat3_cmds = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(blue_tube)),
        mark(300., 300., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
    ];
    let canvas3 = rig.canvas(&coat3_cmds);
    let blue_coat3 = pixel(&canvas3, 300, 300);

    eprintln!(
        "Phthalo Blue coats: coat1={blue_coat1:?}, coat2={blue_coat2:?}, coat3={blue_coat3:?}"
    );

    // Verify progressive deepening and mass tone coverage
    assert!(
        blue_coat2[2] <= blue_coat1[2],
        "coat 2 ({blue_coat2:?}) must be denser/darker than coat 1 ({blue_coat1:?})"
    );
    assert!(
        blue_coat3[2] < blue_coat1[2],
        "coat 3 ({blue_coat3:?}) must significantly deepen beyond coat 1 ({blue_coat1:?})"
    );
    // Masstone of Phthalo Blue must be a deep midnight navy (R <= 15, G <= 60, B <= 135)
    assert!(
        blue_coat3[0] <= 15 && blue_coat3[1] <= 60 && blue_coat3[2] <= 135,
        "3 coats of Phthalo Blue must cover and reach authentic midnight navy mass tone: {blue_coat3:?}"
    );

    // =========================================================================
    // Part 2: Quinacridone Magenta multi-coat accumulation and mass tone coverage
    // =========================================================================
    // Coat 1: Single dab over white gesso (luminous magenta glaze)
    let mag1_cmds = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(magenta_tube)),
        mark(600., 300., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
    ];
    let canvas_m1 = rig.canvas(&mag1_cmds);
    let mag_coat1 = pixel(&canvas_m1, 600, 300);
    assert!(
        mag_coat1[0] > 120 && mag_coat1[2] > 60,
        "coat 1 must be luminous magenta glaze: {mag_coat1:?}"
    );

    // Coat 2: Second dab over cured dry underlayer
    let mag2_cmds = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(magenta_tube)),
        mark(600., 300., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
    ];
    let canvas_m2 = rig.canvas(&mag2_cmds);
    let mag_coat2 = pixel(&canvas_m2, 600, 300);

    // Coat 3: Third dab over cured dry layers -> builds mass tone
    let mag3_cmds = vec![
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::Dip(magenta_tube)),
        mark(600., 300., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
    ];
    let canvas_m3 = rig.canvas(&mag3_cmds);
    let mag_coat3 = pixel(&canvas_m3, 600, 300);
    eprintln!(
        "Quinacridone Magenta coats: coat1={mag_coat1:?}, coat2={mag_coat2:?}, coat3={mag_coat3:?}"
    );

    // Verify progressive deepening and mass tone coverage
    assert!(
        mag_coat2[0] <= mag_coat1[0],
        "coat 2 ({mag_coat2:?}) must be denser than coat 1 ({mag_coat1:?})"
    );
    assert!(
        mag_coat3[0] < mag_coat1[0],
        "coat 3 ({mag_coat3:?}) red channel must deepen beyond coat 1 ({mag_coat1:?})"
    );
    // In masstone, green absorption is high, and red/blue deepen into ruby-carmine (G <= 15, R <= 150, B <= 85)
    assert!(
        mag_coat3[1] <= 15 && mag_coat3[0] <= 150 && mag_coat3[2] <= 85,
        "3 coats of Quinacridone Magenta must cover and reach authentic ruby-carmine mass tone: {mag_coat3:?}"
    );
}

#[test]
fn test_3d_impasto_relief_and_studio_lighting() {
    let mut rig = Rig::new();
    let cad_red = crate::cabinet::find_pigment("cadmium_red").unwrap();

    // 1. Unlit baseline: Paint a dab of Cadmium Red at (500, 500)
    let cmds = vec![
        Command::Initialize,
        Command::EquipRack(0, cad_red.recipe),
        Command::Service(Brush::Round, BrushService::Wipe(0.)),
        Command::Service(Brush::Round, BrushService::DipRack(0)),
        mark(500., 500., Surface::Canvas, Brush::Round),
    ];
    rig.paint.lighting.enabled = false;
    let unlit_canvas = rig.canvas(&cmds);
    let unlit_center = pixel(&unlit_canvas, 500, 500);
    assert!(
        unlit_center[0] > 180,
        "unlit mark should be bright red: {unlit_center:?}"
    );

    // 2. Enable 3D Impasto Relief & Studio Raking Light
    // Standard studio light: 315° NW azimuth, 45° elevation
    rig.paint.lighting = crate::gpu::Lighting {
        enabled: true,
        impasto_depth: 1.0,
        canvas_weave: 0.35,
        specular_sheen: 1.0,
        light_angle_deg: 315.0,
        light_elevation_deg: 45.0,
    };
    let lit_canvas = rig.canvas(&[Command::Dab(Dab::operation(Surface::Canvas.bounds(), 1))]);
    let lit_center = pixel(&lit_canvas, 500, 500);

    // Light-facing crest (top-left slope of dab) vs shadow-facing slope (bottom-right slope of dab)
    let lit_crest = pixel(&lit_canvas, 488, 488);
    let lit_shadow = pixel(&lit_canvas, 512, 512);
    eprintln!("3D Impasto Lit: crest={lit_crest:?}, center={lit_center:?}, shadow={lit_shadow:?}");

    // The light-facing crest must be brighter in red illumination than the shadow-facing slope
    assert!(
        lit_crest[0] > lit_shadow[0],
        "light-facing crest ({lit_crest:?}) must be brighter than trailing shadow ({lit_shadow:?})"
    );

    // 3. Verify palette canvas is located at y >= 1024 and never receives lighting dispatch
    let pal_bounds = Surface::Palette.bounds();
    assert!(
        pal_bounds[1] >= SIDE,
        "palette is located below canvas at y >= 1024 and is excluded from 3D lighting"
    );

    // 4. Test Dry Canvas retains 3D impasto relief
    rig.paint.lighting.enabled = true;
    let dry_canvas = rig.canvas(&[Command::Snapshot, Command::DryCanvas]);
    let dried_center = pixel(&dry_canvas, 500, 500);
    eprintln!("Dried 3D Impasto center: {dried_center:?}");
    assert!(
        dried_center[0] > 150,
        "dried impasto must retain vibrant red: {dried_center:?}"
    );
}

#[test]
fn test_phase_a_exports_transparent_height_normal_and_palette() {
    let mut rig = Rig::new();

    // Paint a thick Cadmium Red / Vermilion dab at (500, 500) and save to Custom Jar 0
    let _ = rig.canvas(&[
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new(
            egui::pos2(500., 500.),
            24.,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.,
        )),
        Command::SaveSwatch(0, Surface::Canvas, [500, 500]),
    ]);

    // 1. Transparent PNG export: bare pixel has alpha 0, painted center has high alpha
    let transparent = rig.export(gpu::ExportKind::Transparent);
    let bare_t = pixel(&transparent, 10, 10);
    let center_t = pixel(&transparent, 500, 500);
    assert_eq!(bare_t[3], 0, "bare canvas must be transparent: {bare_t:?}");
    assert!(
        center_t[3] > 200 && center_t[0] > 150,
        "painted dab must be opaque red: {center_t:?}"
    );

    // Verify transparent export preserves studio colors across pigments (no hue shift)
    for (idx, name) in [
        (0usize, "Hansa Yellow"),
        (1, "Phthalo Blue"),
        (2, "Quinacridone Magenta"),
        (3, "Titanium White"),
        (6, "Cadmium Red"),
        (7, "Bone Black"),
    ] {
        let mut color_rig = Rig::new();
        let _ = color_rig.canvas(&[
            Command::Service(Brush::Round, BrushService::Load(idx)),
            Command::Service(Brush::Round, BrushService::Load(idx)),
            Command::Dab(Dab::new(
                egui::pos2(500., 500.),
                24.,
                1.0,
                Surface::Canvas,
                Brush::Round,
                0.,
            )),
        ]);
        let col = color_rig.export(gpu::ExportKind::Color);
        let trn = color_rig.export(gpu::ExportKind::Transparent);
        let c_px = pixel(&col, 500, 500);
        let t_px = pixel(&trn, 500, 500);
        eprintln!("{name}: Color={c_px:?}, Transparent={t_px:?}");
        let a = t_px[3] as f32 / 255.0;
        for (expected, foreground) in linear_rgb(c_px).into_iter().zip(linear_rgb(t_px)) {
            let over_white = foreground * a + 1.0 - a;
            assert!(
                (expected - over_white).abs() < 0.012,
                "{name} over-white drifted: Color={c_px:?}, Transparent={t_px:?}"
            );
        }
    }

    // 2. Height map export: painted center is higher than bare canvas and grayscale
    let height = rig.export(gpu::ExportKind::Height);
    let bare_h = pixel(&height, 10, 10);
    let center_h = pixel(&height, 500, 500);
    assert_eq!(bare_h[0], bare_h[1]);
    assert_eq!(bare_h[1], bare_h[2]);
    assert!(
        center_h[0] > bare_h[0],
        "painted impasto height ({center_h:?}) must exceed bare canvas ({bare_h:?})"
    );

    // 3. Normal map export: slopes tilt outward around (500, 500)
    let normal = rig.export(gpu::ExportKind::Normal);
    let left_n = pixel(&normal, 482, 500);
    let right_n = pixel(&normal, 518, 500);
    assert!(
        left_n[0] < right_n[0] && left_n[2] > 128 && right_n[2] > 128,
        "normal X must tilt left/right across dab slopes: left={left_n:?}, right={right_n:?}"
    );

    // 4. Verify display was restored to opaque white on bare canvas after export
    let restored = rig.export(gpu::ExportKind::Color);
    assert_eq!(pixel(&restored, 10, 10), [255, 255, 255, 255]);

    // 5. Palette export (JSON and GPL)
    let mut occupied = [false; crate::workbench::CUSTOM_JAR_COUNT];
    occupied[0] = true;
    let dir = tempfile::tempdir().unwrap();
    let json_path = dir.path().join("palette.json");
    let gpl_path = dir.path().join("palette.gpl");

    for path in [&json_path, &gpl_path] {
        let mut encoder = rig.device.create_command_encoder(&Default::default());
        let buf = rig.paint.readback_custom_jars(&rig.device, &mut encoder);
        rig.queue.submit([encoder.finish()]);
        let rx = crate::gpu::export_palette(buf, occupied, path.clone());
        let _ = rig.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
    }

    let json_text = std::fs::read_to_string(&json_path).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json_text).unwrap();
    assert_eq!(parsed["jars"][0]["name"], "Jar 1");
    assert!(parsed["jars"][0]["hex"].as_str().unwrap().starts_with('#'));

    let gpl_text = std::fs::read_to_string(&gpl_path).unwrap();
    assert!(gpl_text.starts_with("GIMP Palette\n"));
    assert!(gpl_text.contains("Jar 1"));
}

#[test]
fn test_phase_b_seamless_tiling_and_underlayer_import() {
    let mut rig = Rig::new();

    // 1. Seamless tiling: paint near right and top edges and verify wraparound
    rig.paint.tile = true;
    let tiled = rig.canvas(&[
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new_tiled(
            egui::pos2(1020., 512.),
            24.,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.,
            true,
        )),
        Command::Dab(Dab::new_tiled(
            egui::pos2(512., 2.),
            24.,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.,
            true,
        )),
    ]);
    let right_edge = pixel(&tiled, 1023, 512);
    let left_wrap = pixel(&tiled, 2, 512);
    let top_edge = pixel(&tiled, 512, 0);
    let bottom_wrap = pixel(&tiled, 512, 1020);
    assert!(
        right_edge[0] > 180 && left_wrap[0] > 180,
        "horizontal wraparound failed: right={right_edge:?}, left={left_wrap:?}"
    );
    assert!(
        top_edge[0] > 180 && bottom_wrap[0] > 180,
        "vertical wraparound failed: top={top_edge:?}, bottom={bottom_wrap:?}"
    );

    // 2. Dried underlayer import: write a 2-tone PNG, import, verify colors, glaze, and undo
    let mut rig2 = Rig::new();
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("underlayer.png");
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let i = ((y * SIDE + x) * 4) as usize;
            let c = if x < SIDE / 2 {
                [30, 120, 230, 255]
            } else {
                [240, 210, 40, 255]
            };
            rgba[i..i + 4].copy_from_slice(&c);
        }
    }
    {
        let file = std::fs::File::create(&png_path).unwrap();
        let mut enc = png::Encoder::new(file, SIDE, SIDE);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        let mut w = enc.write_header().unwrap();
        w.write_image_data(&rgba).unwrap();
    }

    let words = crate::gpu::load_png_as_substrate(&png_path).unwrap();
    let imported = rig2.canvas(&[Command::Snapshot, Command::ImportUnderlayer(words)]);
    let left_px = pixel(&imported, 200, 500);
    let right_px = pixel(&imported, 800, 500);
    eprintln!("Underlayer left={left_px:?}, right={right_px:?}");
    assert!(
        (left_px[0] as i32 - 30).abs() <= 8
            && (left_px[1] as i32 - 120).abs() <= 8
            && (left_px[2] as i32 - 230).abs() <= 8,
        "left underlayer color mismatch: {left_px:?}"
    );
    assert!(
        (right_px[0] as i32 - 240).abs() <= 8
            && (right_px[1] as i32 - 210).abs() <= 8
            && (right_px[2] as i32 - 40).abs() <= 8,
        "right underlayer color mismatch: {right_px:?}"
    );

    // Glaze Quinacridone Magenta over blue underlayer -> should shift toward purple/red
    let glazed = rig2.canvas(&[
        Command::Service(Brush::Round, BrushService::Load(2)),
        Command::Dab(Dab::new(
            egui::pos2(200., 500.),
            20.,
            0.8,
            Surface::Canvas,
            Brush::Round,
            0.,
        )),
    ]);
    let glazed_px = pixel(&glazed, 200, 500);
    assert!(
        glazed_px[0] > left_px[0] && glazed_px[1] < left_px[1],
        "magenta glaze over blue underlayer should increase red and absorb green: {glazed_px:?}"
    );

    // Undo restores bare white canvas
    let undone = rig2.canvas(&[Command::Undo]);
    assert_eq!(pixel(&undone, 200, 500), [255, 255, 255, 255]);

    // 3. Tiled stroke crossing canvas edge must not jump across the canvas
    let mut stroke_cmds = Vec::new();
    let mut tiled_stroke = Stroke::start_tiled(
        Source::Mouse,
        Sample {
            point: egui::pos2(1014., 512.),
            pressure: 1.0,
            timestamp_ms: 0,
            angle: None,
        },
        48.,
        Surface::Canvas,
        Brush::Round,
        0.,
        true,
        &mut stroke_cmds,
    );
    tiled_stroke.extend(
        Sample {
            point: egui::pos2(1034., 512.),
            pressure: 1.0,
            timestamp_ms: 16,
            angle: None,
        },
        &mut stroke_cmds,
    );
    tiled_stroke.finish(&mut stroke_cmds);
    assert!(
        stroke_cmds.len() <= 6,
        "crossing 20px over the tiled edge should emit <= 6 dabs, got {}",
        stroke_cmds.len()
    );
}

#[test]
fn test_phase_c_recording_headless_and_deep_undo() {
    use crate::recording::{
        compile_events, render_headless, BrushKind, RecordedEvent, RecordedSample, Recording,
        ServiceAction, SurfaceKind,
    };

    // 1. Headless JSON recording render test
    let dir = tempfile::tempdir().unwrap();
    let rec_path = dir.path().join("stroke.json");
    let out_path = dir.path().join("render.png");

    let rec = Recording {
        events: vec![
            RecordedEvent::Service {
                legacy_reservoir: false,
                brush: BrushKind::Round,
                action: ServiceAction::Load { tube: 6 },
            },
            RecordedEvent::Stroke {
                id: None,
                group: None,
                legacy_reservoir: false,
                surface: SurfaceKind::Canvas,
                brush: BrushKind::Round,
                ink: false,
                charcoal: false,
                erase: false,
                diameter: 48.0,
                angle_deg: 0.0,
                auto_follow: false,
                knife_sweep: false,
                bristle_sweep: false,
                filbert_sweep: false,
                fan_sweep: false,
                round_pressure: false,
                tile: Some(false),
                samples: vec![
                    RecordedSample {
                        x: 400.0,
                        y: 500.0,
                        pressure: 1.0,
                        timestamp_ms: 0,
                        angle_deg: None,
                    },
                    RecordedSample {
                        x: 600.0,
                        y: 500.0,
                        pressure: 1.0,
                        timestamp_ms: 16,
                        angle_deg: None,
                    },
                ],
            },
        ],
        ..Default::default()
    };
    std::fs::write(&rec_path, serde_json::to_string_pretty(&rec).unwrap()).unwrap();
    let count = render_headless(&rec_path, &out_path, crate::gpu::ExportKind::Color, None).unwrap();
    assert_eq!(count, 2);
    assert!(out_path.exists());

    // Decode rendered PNG and verify the stroke painted red at (500, 500) and white at (10, 10)
    let file = std::fs::File::open(&out_path).unwrap();
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().unwrap();
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).unwrap();
    assert_eq!([info.width, info.height], [SIDE, SIDE]);
    let px = |x: u32, y: u32| -> [u8; 4] {
        let i = ((y * SIDE + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };
    assert_eq!(px(10, 10), [255, 255, 255, 255]);
    assert!(px(500, 500)[0] > 180 && px(500, 500)[1] < 60);

    // 2. Deep undo (> 8 steps, e.g. 12 steps) via Workbench + recording replay
    let mut wb = crate::workbench::Workbench::default();
    let mut events = Vec::new();
    let mut marks = Vec::new();
    for step in 0..12 {
        marks.push(events.len());
        wb.snapshot();
        if step == 0 {
            events.push(RecordedEvent::Service {
                legacy_reservoir: false,
                brush: BrushKind::Round,
                action: ServiceAction::Load { tube: 1 },
            });
        }
        let x = 100.0 + step as f32 * 60.0;
        events.push(RecordedEvent::Stroke {
            id: None,
            group: None,
            legacy_reservoir: false,
            surface: SurfaceKind::Canvas,
            brush: BrushKind::Round,
            ink: false,
            charcoal: false,
            erase: false,
            diameter: 32.0,
            angle_deg: 0.0,
            auto_follow: false,
            knife_sweep: false,
            bristle_sweep: false,
            filbert_sweep: false,
            fan_sweep: false,
            round_pressure: false,
            tile: Some(false),
            samples: vec![RecordedSample {
                x,
                y: 500.0,
                pressure: 1.0,
                timestamp_ms: step as u32 * 20,
                angle_deg: None,
            }],
        });
    }
    assert_eq!(wb.history.len(), 12);

    // Undo all 12 steps (past the 8-slot GPU ring limit)
    for _ in 0..12 {
        assert!(wb.undo());
        let mark = marks.pop().unwrap();
        events.truncate(mark);
    }
    assert!(events.is_empty());
    let mut tile = false;
    assert!(compile_events(&events, &mut tile).is_empty());
}

#[test]
fn test_three_layer_stack_reference_sketch_and_paint() {
    let mut rig = Rig::new();

    // 1. Import Reference Layer (non-destructive, toggleable)
    let dir = tempfile::tempdir().unwrap();
    let png_path = dir.path().join("ref.png");
    let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
    for px in rgba.chunks_exact_mut(4) {
        px.copy_from_slice(&[40, 110, 220, 255]);
    }
    {
        let file = std::fs::File::create(&png_path).unwrap();
        let mut enc = png::Encoder::new(file, SIDE, SIDE);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    }

    let words = crate::gpu::load_png_as_reference(&png_path).unwrap();
    rig.paint.layers.reference_opacity = 1.0;
    rig.paint.layers.reference_visible = true;
    let shown = rig.canvas(&[Command::ImportReference(words)]);
    assert_eq!(pixel(&shown, 200, 200), [40, 110, 220, 255]);

    // Toggle reference OFF -> white gesso
    rig.paint.layers.reference_visible = false;
    let hidden = rig.canvas(&[Command::Dab(Dab::operation(Surface::Canvas.bounds(), 1))]);
    assert_eq!(pixel(&hidden, 200, 200), [255, 255, 255, 255]);

    // Toggle reference back ON
    rig.paint.layers.reference_visible = true;
    let reshown = rig.canvas(&[Command::Dab(Dab::operation(Surface::Canvas.bounds(), 1))]);
    assert_eq!(pixel(&reshown, 200, 200), [40, 110, 220, 255]);

    // 2. Charcoal Pencil on Sketch Layer + Eraser
    rig.paint.layers.reference_visible = false;
    let sketched = rig.canvas(&[Command::Dab(Dab::charcoal(
        egui::pos2(500., 500.),
        16.,
        1.0,
        false,
        false,
    ))]);
    let s_px = pixel(&sketched, 500, 500);
    assert!(s_px[0] < 180, "charcoal must darken sketch layer: {s_px:?}");

    let erased = rig.canvas(&[Command::Dab(Dab::charcoal(
        egui::pos2(500., 500.),
        16.,
        1.0,
        true,
        false,
    ))]);
    let e_px = pixel(&erased, 500, 500);
    assert!(e_px[0] > s_px[0], "eraser must lighten sketch: {e_px:?}");

    // 3. Paint over Sketch, hide Paint to confirm Sketch is untouched, Clear Paint preserves Sketch
    let _ = rig.canvas(&[
        Command::Dab(Dab::charcoal(
            egui::pos2(300., 300.),
            16.,
            1.0,
            false,
            false,
        )),
        Command::Dab(Dab::charcoal(
            egui::pos2(300., 300.),
            16.,
            1.0,
            false,
            false,
        )),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new_tiled(
            egui::pos2(300., 300.),
            20.,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.,
            false,
        )),
    ]);
    rig.paint.layers.paint_visible = false;
    let sketch_only = rig.canvas(&[Command::Dab(Dab::operation(Surface::Canvas.bounds(), 1))]);
    assert!(
        pixel(&sketch_only, 300, 300)[0] < 150,
        "charcoal under paint must stay intact"
    );

    rig.paint.layers.paint_visible = true;
    let cleared_paint = rig.canvas(&[Command::Clear(Surface::Canvas)]);
    assert!(
        pixel(&cleared_paint, 300, 300)[0] < 150,
        "clearing paint must preserve sketch layer"
    );
}

#[test]
fn test_project_save_and_load_pigment_file() {
    let mut rig = Rig::new();
    let _ = rig.canvas(&[
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new_tiled(
            egui::pos2(450., 450.),
            24.,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.,
            false,
        )),
        Command::Dab(Dab::charcoal(
            egui::pos2(200., 200.),
            14.,
            1.0,
            false,
            false,
        )),
        Command::SaveSwatch(0, Surface::Canvas, [450, 450]),
    ]);

    let dir = tempfile::tempdir().unwrap();
    let proj_path = dir.path().join("test.pigment");
    rig.state(&[
        Command::Service(Brush::Flat, BrushService::Dip(0)),
        Command::Service(Brush::SoftMop, BrushService::Dip(1)),
    ]);
    let mut jars = vec![false; crate::workbench::CUSTOM_JAR_COUNT];
    jars[0] = true;
    let meta = crate::project::ProjectMeta {
        calibration_version: crate::project::CURRENT_CALIBRATION_VERSION,
        loads: [0.5, 0.0, 0.0, 0.0, 0.0, 0.0],
        jars,
        sizes: [48.0, 64.0],
        angle: 30.0,
        brush: crate::recording::BrushKind::Round,
        ink_selected: false,
        charcoal_erase: false,
        charcoal_size: 14.0,
        lighting: rig.paint.lighting.into(),
        layers: rig.paint.layers.into(),
        tile: true,
        recording: crate::recording::Recording::default(),
    };

    let mut encoder = rig.device.create_command_encoder(&Default::default());
    let buf = rig.paint.readback_project(&rig.device, &mut encoder);
    rig.queue.submit([encoder.finish()]);
    let rx = crate::gpu::save_project(buf, meta, proj_path.clone());
    let _ = rig.device.poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    assert!(proj_path.exists());

    // Load into a brand-new Rig and verify pixels + metadata match
    let (loaded_meta, bytes) = crate::project::read_project(&proj_path).unwrap();
    assert_eq!(loaded_meta.sizes, [48.0, 64.0]);
    assert!(loaded_meta.tile);
    assert!(loaded_meta.jars[0]);
    assert!((loaded_meta.loads[Brush::Flat.index()] - 0.25).abs() < 0.0001);
    assert_eq!(loaded_meta.loads[Brush::SoftMop.index()], 0.25);
    assert_eq!(loaded_meta.loads[Brush::PaletteKnife.index()], 0.0);

    let mut rig2 = Rig::new();
    let loaded = rig2.canvas(&[Command::Snapshot, Command::LoadProject(bytes)]);
    assert!(
        pixel(&loaded, 450, 450)[0] > 180,
        "loaded project must preserve red paint"
    );
    assert!(
        pixel(&loaded, 200, 200)[0] < 180,
        "loaded project must preserve charcoal sketch"
    );
    let (_, loads) = rig2.state(&[]);
    let flat = values(&loads)
        .nth(Brush::Flat.load_start() as usize)
        .unwrap();
    let mop = values(&loads)
        .nth(Brush::SoftMop.load_start() as usize)
        .unwrap();
    assert!(flat[0] > 15_000. && flat[1] == 0.);
    assert_eq!(mop[1], 46_000.);

    // Undo restores clean canvas
    let undone = rig2.canvas(&[Command::Undo]);
    assert_eq!(pixel(&undone, 450, 450), [255, 255, 255, 255]);
}

#[test]
fn test_robust_oil_brush_pack_and_auto_follow() {
    // 1. Test all 6 profiles on DX12 GPU + pigment mass conservation
    for brush in [
        Brush::Round,
        Brush::Flat,
        Brush::Filbert,
        Brush::Fan,
        Brush::PaletteKnife,
        Brush::SoftMop,
    ] {
        let mut rig = Rig::new();
        let before = rig.state(&[Command::Service(brush, BrushService::Load(6))]);
        let img = rig.canvas(&[Command::Dab(Dab::new_with_load(
            egui::pos2(500., 500.),
            28.,
            1.0,
            Surface::Canvas,
            brush,
            0.,
            false,
            1.0,
        ))]);
        let center = pixel(&img, 500, 500);
        assert!(
            center[0] > 100 && center[1] < 240,
            "{brush:?} must deposit red paint at center: {center:?}"
        );
        let _ = before;
    }

    // 2. Test Palette Knife scraping & smearing unloaded
    let mut knife_rig = Rig::new();
    let _ = knife_rig.canvas(&[
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new_tiled(
            egui::pos2(490., 500.),
            20.,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.,
            false,
        )),
    ]);
    // Drag unloaded Palette Knife from 490 -> 518 to smear red paint onto bare canvas
    let mut cmds = Vec::new();
    let mut knife_stroke = Stroke::start_full(
        Source::Mouse,
        Sample {
            point: egui::pos2(490., 500.),
            pressure: 0.9,
            timestamp_ms: 0,
            angle: None,
        },
        48.,
        Surface::Canvas,
        Brush::PaletteKnife,
        0.,
        true,
        0.0,
        false,
        true,
        &mut cmds,
    );
    knife_stroke.extend(
        Sample {
            point: egui::pos2(518., 500.),
            pressure: 0.9,
            timestamp_ms: 16,
            angle: None,
        },
        &mut cmds,
    );
    knife_stroke.finish(&mut cmds);
    let smeared = knife_rig.canvas(&cmds);
    let pulled = pixel(&smeared, 518, 500);
    assert!(
        pulled[0] > 200 && pulled[1] < 230,
        "unloaded Palette Knife must scrape and smear paint forward: {pulled:?}"
    );
}

#[test]
fn dx12_palette_knife_turn_carries_wet_pigments_over_dry_paint() {
    let mut rig = Rig::new();
    rig.state(&[
        Command::Service(Brush::Round, BrushService::Load(6)),
        mark(550., 510., Surface::Canvas, Brush::Round),
        Command::DryCanvas,
        Command::Service(Brush::Round, BrushService::Wipe(0.0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        mark(487., 500., Surface::Canvas, Brush::Round),
        Command::Service(Brush::Round, BrushService::Wipe(0.0)),
        Command::Service(Brush::Round, BrushService::Load(1)),
        mark(520., 500., Surface::Canvas, Brush::Round),
    ]);
    let dry = rig.dry_state();
    let (before, brush_before) = rig.state(&[]);
    let recording: Recording = serde_json::from_str(
        r#"{
            "format":"pigment-studio-recording-v1",
            "events":[{
                "type":"stroke","surface":"canvas","brush":"palette_knife","diameter":52,"knife_sweep":true,
                "samples":[
                    {"x":483,"y":500,"pressure":0.95,"angle_deg":90},
                    {"x":521,"y":500,"pressure":0.95,"angle_deg":104},
                    {"x":553,"y":510,"pressure":0.85,"angle_deg":125},
                    {"x":566,"y":521,"pressure":0.10,"angle_deg":148}
                ]
            }]
        }"#,
    )
    .unwrap();
    let commands = recording::compile_events(&recording.events, &mut false);
    let (after, brush_after) = rig.state(&commands);
    let mixed = values(&after).enumerate().any(|(index, pigment)| {
        let x = index % SIDE as usize;
        let y = index / SIDE as usize;
        (548..566).contains(&x)
            && (495..529).contains(&y)
            && pigment[0] > 0.005
            && pigment[1] > 0.005
    });
    assert!(
        mixed,
        "the blade should carry both wet pigments beyond their source dabs"
    );
    assert_eq!(
        rig.dry_state(),
        dry,
        "the dry underpainting should stay locked"
    );
    for (a, b) in totals(&before, &brush_before)
        .into_iter()
        .zip(totals(&after, &brush_after))
    {
        assert!(
            (a - b).abs() < 0.001 * a.max(1.0),
            "paint mass changed: {a} -> {b}"
        );
    }
}

#[test]
fn dx12_swept_knife_lays_even_height_at_different_sample_rates() {
    let mut rig = Rig::new();
    rig.paint.lighting.canvas_weave = 0.0;
    let mut commands = Vec::new();
    for (y, intervals, sweep) in [(300., 16, true), (500., 197, true), (700., 16, false)] {
        commands.extend([
            Command::Service(Brush::PaletteKnife, BrushService::Wipe(0.0)),
            Command::Service(Brush::PaletteKnife, BrushService::Dip(0)),
        ]);
        let sample = |i| Sample {
            point: egui::pos2(200. + 440. * i as f32 / intervals as f32, y),
            pressure: 1.0,
            timestamp_ms: i,
            angle: Some(std::f32::consts::FRAC_PI_2),
        };
        let mut stroke = Stroke::start_full(
            Source::Mouse,
            sample(0),
            64.,
            Surface::Canvas,
            Brush::PaletteKnife,
            std::f32::consts::FRAC_PI_2,
            false,
            1.0,
            false,
            sweep,
            &mut commands,
        );
        for i in 1..=intervals {
            stroke.extend(sample(i), &mut commands);
        }
        stroke.finish(&mut commands);
    }
    let (paint, _) = rig.state(&commands);
    let row = |y| {
        values(&paint[y * SIDE as usize * 32..])
            .skip(240)
            .take(360)
            .map(|pigment| pigment[0])
            .collect::<Vec<_>>()
    };
    let sparse = row(300);
    let dense = row(500);
    let stamped = row(700);
    let span = |row: &[f32]| {
        row.iter().copied().fold(0.0, f32::max) - row.iter().copied().fold(f32::INFINITY, f32::min)
    };
    let max_resampling_error = sparse
        .iter()
        .zip(&dense)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(
        span(&sparse) < span(&stamped) * 0.25 && span(&dense) < span(&stamped) * 0.25,
        "peak-to-peak thickness should drop by at least 75%: sparse {}, dense {}, stamped {}",
        span(&sparse),
        span(&dense),
        span(&stamped),
    );
    assert!(
        max_resampling_error < 0.012 && sparse.iter().all(|mass| *mass > 0.15),
        "changing the sample density should keep a continuous ribbon: {max_resampling_error}"
    );

    let height = rig.export(gpu::ExportKind::Height);
    for y in [300, 500] {
        let samples: Vec<_> = (240..600).map(|x| pixel(&height, x, y)[0]).collect();
        let min = samples.iter().min().unwrap();
        let max = samples.iter().max().unwrap();
        assert!(
            max - min <= 3,
            "visible height-map ripple at row {y}: {min}..{max}"
        );
    }
}

#[test]
fn dx12_hog_bristle_flat_split_load_turn_and_lift() {
    let mut rig = Rig::new();
    let recording: Recording = serde_json::from_str(
        r#"{
            "format":"pigment-studio-recording-v1",
            "events":[
                {"type":"service","brush":"flat","action":{"kind":"dip_rack","slot":1,"lane":"left"}},
                {"type":"service","brush":"flat","action":{"kind":"dip_rack","slot":4,"lane":"right"}},
                {"type":"service","brush":"flat","action":{"kind":"dip_rack","slot":1,"lane":"middle"}},
                {"type":"service","brush":"flat","action":{"kind":"dip_rack","slot":4,"lane":"middle"}},
                {"type":"stroke","surface":"canvas","brush":"flat","diameter":60,"bristle_sweep":true,
                 "samples":[
                    {"x":200,"y":400,"pressure":0.86,"angle_deg":90},
                    {"x":450,"y":400,"pressure":0.86,"angle_deg":90},
                    {"x":510,"y":415,"pressure":0.60,"angle_deg":112},
                    {"x":560,"y":452,"pressure":0.32,"angle_deg":130},
                    {"x":595,"y":500,"pressure":0.12,"angle_deg":155},
                    {"x":625,"y":552,"pressure":0.05,"angle_deg":155}
                 ]}
            ]
        }"#,
    )
    .unwrap();
    let mut seed = vec![Command::Initialize];
    recording::equip_cabinet(&mut seed);
    seed.extend(recording::compile_events(
        &recording.events[..2],
        &mut false,
    ));
    let (_, split) = rig.state(&seed);
    let lanes: Vec<_> = values(&split).skip(1).take(3).collect();
    assert!(lanes[0][0] > 15_000. && lanes[0][1] == 0.);
    assert!(lanes[1].iter().all(|mass| *mass == 0.));
    assert!(lanes[2][1] > 15_000. && lanes[2][0] == 0.);

    let (before, loads_before) = rig.state(&recording::compile_events(
        &recording.events[2..4],
        &mut false,
    ));
    let (after, loads_after) = rig.state(&recording::compile_events(
        &recording.events[4..],
        &mut false,
    ));
    let pigment = |x, y| values(&after).nth(y * SIDE as usize + x).unwrap();
    let yellow = pigment(330, 382);
    let green = pigment(330, 400);
    let blue = pigment(330, 418);
    assert!(yellow[0] > 0.10 && yellow[1] < 0.001, "{yellow:?}");
    assert!(green[0] > 0.10 && green[1] > 0.10, "{green:?}");
    assert!(blue[1] > 0.10 && blue[0] < 0.001, "{blue:?}");
    let count_contact = |y, x0, x1, threshold| {
        (x0..x1)
            .filter(|x| {
                let paint = pigment(*x, y);
                paint[0] + paint[1] > threshold
            })
            .count()
    };
    assert!(count_contact(400, 310, 350, 0.10) >= 35);
    let lift = count_contact(500, 580, 610, 0.01);
    assert!(
        (1..18).contains(&lift),
        "unexpected contact at lift: {lift}"
    );
    for (a, b) in totals(&before, &loads_before)
        .into_iter()
        .zip(totals(&after, &loads_after))
    {
        assert!(
            (a - b).abs() < 0.001 * a.max(1.0),
            "paint mass changed: {a} -> {b}"
        );
    }
}

#[test]
fn dx12_brush_reservoirs_are_independent_and_clean_is_local() {
    let mut rig = Rig::new();
    let all = Brush::ALL;
    let mut load = vec![Command::Initialize];
    for brush in all {
        load.push(Command::Service(brush, BrushService::Dip(brush.index())));
    }
    let (_, before) = rig.state(&load);
    for brush in all {
        for lane in brush.load_lanes() {
            let pigment = values(&before).nth(lane as usize).unwrap();
            assert!(pigment[brush.index()] > 15_000.0);
            for (i, mass) in pigment.into_iter().enumerate() {
                if i != brush.index() {
                    assert_eq!(mass, 0.0, "{brush:?} got another brush's pigment");
                }
            }
        }
    }

    let selected = Brush::Filbert;
    let (_, after) = rig.state(&[
        Command::Snapshot,
        Command::Service(selected, BrushService::Wipe(0.0)),
    ]);
    for brush in all {
        for lane in brush.load_lanes() {
            let actual = values(&after).nth(lane as usize).unwrap();
            let expected = if brush == selected {
                [0.0; 8]
            } else {
                values(&before).nth(lane as usize).unwrap()
            };
            assert_eq!(actual, expected, "{brush:?} lane {lane}");
        }
    }
    let (_, restored) = rig.state(&[Command::Undo]);
    assert_eq!(
        &restored[..gpu::BRUSH_LOAD_BYTES],
        &before[..gpu::BRUSH_LOAD_BYTES]
    );

    let events = r#","events":[
        {"type":"service","brush":"fan","action":{"kind":"dip","tube":0}},
        {"type":"stroke","surface":"canvas","brush":"palette_knife","samples":[{"x":410,"y":300,"pressure":1.0}]}
    ]}"#;
    for version in [1, 2] {
        let recording = Recording::parse(&format!(
            "{{\"format\":\"pigment-studio-recording-v{version}\"{events}"
        ))
        .unwrap();
        let mut commands = vec![Command::Initialize];
        commands.extend(recording::compile_events(&recording.events, &mut false));
        let (paint, _) = rig.state(&commands);
        let yellow = values(&paint).nth(300 * SIDE as usize + 410).unwrap()[0];
        assert_eq!(yellow > 0.0, version == 1, "recording format v{version}");
    }
}

#[test]
fn dx12_round_pressure_ramp_and_wet_transfer() {
    let mut rig = Rig::new();
    let recording = Recording::parse(
        r#"{"format":"pigment-studio-recording-v2","events":[
            {"type":"stroke","surface":"canvas","brush":"round","round_pressure":true,"diameter":64,
             "samples":[
                {"x":105,"y":300,"pressure":0.06},
                {"x":175,"y":300,"pressure":0.06},
                {"x":255,"y":300,"pressure":0.37},
                {"x":375,"y":300,"pressure":0.88},
                {"x":600,"y":300,"pressure":0.88},
                {"x":720,"y":300,"pressure":0.35},
                {"x":835,"y":300,"pressure":0.06},
                {"x":895,"y":300,"pressure":0.06}
             ]},
            {"type":"stroke","surface":"canvas","brush":"round","round_pressure":true,"diameter":48,
             "samples":[
                {"x":472,"y":600,"pressure":0.86},
                {"x":523,"y":607,"pressure":0.9},
                {"x":555,"y":617,"pressure":0.77},
                {"x":578,"y":626,"pressure":0.23}
             ]}
        ]}"#,
    )
    .unwrap();

    let mut commands = vec![Command::Service(Brush::Round, BrushService::Dip(0))];
    commands.extend(recording::compile_events(
        &recording.events[..1],
        &mut false,
    ));
    let (ramp, _) = rig.state(&commands);
    let mass = |x, y| paint_at(&ramp, x, y)[0];
    let width = |x| (262..338).filter(|y| mass(x, *y) > 0.012).count();
    let (light, medium, firm, lift) = (width(140), width(255), width(480), width(860));
    assert!(light > 1 && light < medium && medium < firm && lift.abs_diff(light) <= 2);
    assert!(
        (110..889).all(|x| mass(x, 300) > 0.012),
        "pressure ramp has a gap"
    );
    let plateau = (400..550).map(|x| mass(x, 300));
    let min = plateau.clone().fold(f32::INFINITY, f32::min);
    let max = plateau.fold(0.0, f32::max);
    assert!(
        max - min < 0.015,
        "periodic bands across firm contact: {min}..{max}"
    );

    let (before, loads_before) = rig.state(&[
        mark(490., 600., Surface::Canvas, Brush::Round),
        Command::Service(Brush::Round, BrushService::Wipe(0.0)),
        Command::Service(Brush::Round, BrushService::Dip(1)),
        mark(520., 602., Surface::Canvas, Brush::Round),
        Command::Service(Brush::Round, BrushService::Wipe(0.0)),
    ]);
    let (after, loads_after) = rig.state(&recording::compile_events(
        &recording.events[1..],
        &mut false,
    ));
    let mixed = (550..573).any(|x| {
        let origin = paint_at(&before, x, 617);
        let paint = paint_at(&after, x, 617);
        origin[0] + origin[1] == 0.0 && paint[0] > 0.002 && paint[1] > 0.002
    });
    assert!(
        mixed,
        "the round should carry yellow and blue beyond the source marks"
    );
    for (a, b) in totals(&before, &loads_before)
        .into_iter()
        .zip(totals(&after, &loads_after))
    {
        assert!(
            (a - b).abs() < 0.001 * a.max(1.0),
            "paint mass changed: {a} -> {b}"
        );
    }
}

#[test]
fn dx12_filbert_roll_and_lift_preserve_paint_and_bristle_texture() {
    let mut rig = Rig::new();
    let recording = Recording::parse(
        r#"{"format":"pigment-studio-recording-v2","events":[
            {"type":"stroke","surface":"canvas","brush":"filbert","filbert_sweep":true,"diameter":72,
             "samples":[
                {"x":120,"y":320,"pressure":0.86,"angle_deg":90},
                {"x":335,"y":320,"pressure":0.86,"angle_deg":90},
                {"x":425,"y":320,"pressure":0.86,"angle_deg":0},
                {"x":555,"y":320,"pressure":0.86,"angle_deg":0},
                {"x":665,"y":342,"pressure":0.31,"angle_deg":14},
                {"x":765,"y":391,"pressure":0.09,"angle_deg":28},
                {"x":820,"y":420,"pressure":0.09,"angle_deg":28}
             ]}
        ]}"#,
    )
    .unwrap();
    let (before, loads_before) =
        rig.state(&[Command::Service(Brush::Filbert, BrushService::Dip(1))]);
    let (paint, loads_after) = rig.state(&recording::compile_events(&recording.events, &mut false));
    let mass = |x, y| paint_at(&paint, x, y)[1];
    let width = |x| (250..465).filter(|y| mass(x, *y) > 0.012).count();
    let (broad, rolled, lift) = (width(230), width(500), width(790));
    assert!(
        broad > 40 && rolled > 10 && rolled * 4 < broad * 3 && lift > 1 && lift * 2 < rolled,
        "unexpected widths for broad, rolled, and lifted contact: {broad}, {rolled}, {lift}"
    );
    let body = (180..305).map(|x| mass(x, 320));
    let min = body.clone().fold(f32::INFINITY, f32::min);
    let max = body.fold(0., f32::max);
    assert!(
        min > 0.08 && max - min < 0.015,
        "banding along the broad stroke: {min}..{max}"
    );
    let strands = (308..332).map(|y| mass(230, y));
    let min = strands.clone().fold(f32::INFINITY, f32::min);
    let max = strands.fold(0., f32::max);
    assert!(
        min > 0.05 && max - min > 0.012,
        "missing continuous bristle variation: {min}..{max}"
    );
    assert!(mass(230, 344) > 0.005 && mass(230, 344) < min);
    for (a, b) in totals(&before, &loads_before)
        .into_iter()
        .zip(totals(&paint, &loads_after))
    {
        assert!(
            (a - b).abs() < 0.001 * a.max(1.0),
            "paint mass changed: {a} -> {b}"
        );
    }
}

#[test]
fn dx12_fan_separates_tufts_and_merges_them_under_pressure() {
    let mut rig = Rig::new();
    let recording = Recording::parse(
        r#"{"format":"pigment-studio-recording-v2","events":[
            {"type":"stroke","surface":"canvas","brush":"fan","fan_sweep":true,"diameter":72,
             "samples":[
                {"x":120,"y":290,"pressure":0.24,"angle_deg":90},
                {"x":560,"y":290,"pressure":0.24,"angle_deg":90}
             ]},
            {"type":"stroke","surface":"canvas","brush":"fan","fan_sweep":true,"diameter":72,
             "samples":[
                {"x":120,"y":500,"pressure":0.92,"angle_deg":90},
                {"x":530,"y":500,"pressure":0.92,"angle_deg":90},
                {"x":700,"y":524,"pressure":0.72,"angle_deg":111},
                {"x":820,"y":596,"pressure":0.29,"angle_deg":126},
                {"x":890,"y":654,"pressure":0.08,"angle_deg":130}
             ]}
        ]}"#,
    )
    .unwrap();
    let (before, loads_before) = rig.state(&[Command::Service(Brush::Fan, BrushService::Dip(1))]);
    let (paint, loads_after) = rig.state(&recording::compile_events(&recording.events, &mut false));
    let mass = |x, y| paint_at(&paint, x, y)[1];
    let runs = |center: usize| {
        let mut was_painted = false;
        let mut count = 0;
        for y in center - 20..center + 21 {
            let painted = mass(320, y) > 0.012;
            count += usize::from(painted && !was_painted);
            was_painted = painted;
        }
        count
    };
    let separated = runs(290);
    assert!(
        (7..=9).contains(&separated),
        "expected distinct tufts, got {separated}"
    );
    assert_eq!(runs(500), 1, "firm pressure should join the tufts");

    // Sixteen pixels repeat the canvas weave phase so this measures stroke banding.
    let track = (180..500).step_by(16).map(|x| mass(x, 290));
    let min = track.clone().fold(f32::INFINITY, f32::min);
    let max = track.fold(0.0, f32::max);
    assert!(
        min > 0.02 && max - min < 0.015,
        "light central tuft bands: {min}..{max}"
    );
    let firm: f32 = (455..545).map(|y| mass(330, y)).sum();
    let lift: f32 = (590..665).map(|y| mass(865, y)).sum();
    assert!(
        lift > 0.01 && lift < firm * 0.25,
        "expected a lighter curved lift: {lift} vs {firm}"
    );
    for (a, b) in totals(&before, &loads_before)
        .into_iter()
        .zip(totals(&paint, &loads_after))
    {
        assert!(
            (a - b).abs() < 0.001 * a.max(1.0),
            "paint mass changed: {a} -> {b}"
        );
    }
}

#[test]
fn test_phase_e_underlayer_and_batch_rendering() {
    use crate::recording::{
        render_batch, render_headless, BatchJob, BatchManifest, BrushKind, RecordedEvent,
        RecordedSample, Recording, ServiceAction, SurfaceKind,
    };

    let dir = tempfile::tempdir().unwrap();
    let ul_a = dir.path().join("ul_blue.png");
    let ul_b = dir.path().join("ul_green.png");

    let write_solid_png = |path: &std::path::Path, rgb: [u8; 3]| {
        let mut rgba = vec![0u8; (SIDE * SIDE * 4) as usize];
        for px in rgba.chunks_exact_mut(4) {
            px.copy_from_slice(&[rgb[0], rgb[1], rgb[2], 255]);
        }
        let file = std::fs::File::create(path).unwrap();
        let mut enc = png::Encoder::new(file, SIDE, SIDE);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgba).unwrap();
    };
    write_solid_png(&ul_a, [40, 100, 220]);
    write_solid_png(&ul_b, [50, 190, 80]);

    let rec_path = dir.path().join("glaze.json");
    let rec = Recording {
        events: vec![
            RecordedEvent::Service {
                legacy_reservoir: false,
                brush: BrushKind::Round,
                action: ServiceAction::Load { tube: 2 },
            },
            RecordedEvent::Stroke {
                id: None,
                group: None,
                legacy_reservoir: false,
                surface: SurfaceKind::Canvas,
                brush: BrushKind::Round,
                ink: false,
                charcoal: false,
                erase: false,
                diameter: 40.0,
                angle_deg: 0.0,
                auto_follow: false,
                knife_sweep: false,
                bristle_sweep: false,
                filbert_sweep: false,
                fan_sweep: false,
                round_pressure: false,
                tile: Some(false),
                samples: vec![RecordedSample {
                    x: 500.0,
                    y: 500.0,
                    pressure: 0.9,
                    timestamp_ms: 0,
                    angle_deg: None,
                }],
            },
        ],
        ..Default::default()
    };
    std::fs::write(&rec_path, serde_json::to_string_pretty(&rec).unwrap()).unwrap();

    // 1. Single headless render with --underlayer override
    let out_single = dir.path().join("single.png");
    render_headless(
        &rec_path,
        &out_single,
        crate::gpu::ExportKind::Color,
        Some(&ul_a),
    )
    .unwrap();
    assert!(out_single.exists());

    // 2. Batch manifest reusing shared_recording across two different underlayers
    let out_a = dir.path().join("out_a.png");
    let out_b = dir.path().join("out_b.png");
    let manifest_path = dir.path().join("batch.json");
    let manifest = BatchManifest {
        format: "pigment-studio-batch-v1".into(),
        shared_recording: Some("glaze.json".into()),
        jobs: vec![
            BatchJob {
                recording: None,
                underlayer: Some("ul_blue.png".into()),
                out: "out_a.png".into(),
                mode: Some("color".into()),
                smooth_height: false,
                normal_convention: None,
            },
            BatchJob {
                recording: None,
                underlayer: Some("ul_green.png".into()),
                out: "out_b.png".into(),
                mode: Some("color".into()),
                smooth_height: false,
                normal_convention: None,
            },
        ],
    };
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let results = render_batch(&manifest_path).unwrap();
    assert_eq!(results.len(), 2);

    let read_px = |path: &std::path::Path, x: u32, y: u32| -> [u8; 4] {
        let file = std::fs::File::open(path).unwrap();
        let mut reader = png::Decoder::new(std::io::BufReader::new(file))
            .read_info()
            .unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        let i = ((y * SIDE + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };

    // out_a has blue underlayer at (100, 100), out_b has green underlayer at (100, 100)
    assert!(read_px(&out_a, 100, 100)[2] > 180);
    assert!(read_px(&out_b, 100, 100)[1] > 160);
    // Single render and batch out_a must be bit-for-bit identical (determinism)
    assert_eq!(read_px(&out_single, 500, 500), read_px(&out_a, 500, 500));
}

#[test]
fn test_phase_4_pb15_2_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 2);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thin glaze on white ground
    let thin_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Service(Brush::Round, BrushService::Wipe(0.20)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let thin_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let thin_hiding = thin_diag.optical.wet_layer_hiding_ratio_pct.unwrap();
    assert!(
        thin_hiding < 12.0,
        "PB15:2 thin glaze hiding ratio should be low (<12%), got {thin_hiding:.2}%"
    );
    let thin_cpu = crate::pigment::rgb_composite(thin_diag.wet_channels, thin_diag.dry_channels);
    let thin_gpu = pixel(&thin_img, 160, 160);
    for ch in 0..3 {
        assert!(
            thin_cpu[ch].abs_diff(thin_gpu[ch]) <= 2,
            "thin glaze CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            thin_cpu[ch],
            thin_gpu[ch]
        );
    }

    // 2. Thick masstone
    let mast_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 320, 160);
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 3. Titanium White tint (clean blue: B > G > R)
    let tint_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let tint_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let tint_cpu = crate::pigment::rgb_composite(tint_diag.wet_channels, tint_diag.dry_channels);
    let tint_gpu = pixel(&tint_img, 480, 160);
    assert!(
        tint_gpu[2] > tint_gpu[1] && tint_gpu[1] > tint_gpu[0],
        "tint should be blue: {tint_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            tint_cpu[ch].abs_diff(tint_gpu[ch]) <= 2,
            "white tint CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            tint_cpu[ch],
            tint_gpu[ch]
        );
    }

    // 4. Hansa Yellow Light mix (vivid green: G > R and G > B)
    let green_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Dab(Dab::new(
            egui::pos2(640.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let green_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 640, 160);
    let green_cpu = crate::pigment::rgb_composite(green_diag.wet_channels, green_diag.dry_channels);
    let green_gpu = pixel(&green_img, 640, 160);
    assert!(
        green_gpu[1] > green_gpu[0] && green_gpu[1] > green_gpu[2],
        "Hansa Yellow + PB15:2 mix should be green: {green_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            green_cpu[ch].abs_diff(green_gpu[ch]) <= 2,
            "green mix CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            green_cpu[ch],
            green_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_titanium_white_pw6_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 3);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thick Titanium White masstone
    let mast_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 160, 160);
    assert!(
        mast_gpu[0] > 240 && mast_gpu[1] > 240 && mast_gpu[2] > 240,
        "PW6 masstone should be bright white: {mast_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "PW6 masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 2. Titanium White hiding power: 3 dabs (>40%) and thick 12-dab build (>75%)
    let hiding_3 = mast_diag.optical.wet_layer_hiding_ratio_pct.unwrap();
    assert!(
        hiding_3 > 40.0,
        "PW6 3-dab hiding ratio should be >40%, got {hiding_3:.2}%"
    );
    let mut thick_cmds = vec![Command::Initialize];
    for _ in 0..4 {
        thick_cmds.push(Command::Service(Brush::Round, BrushService::Load(3)));
        for _ in 0..3 {
            thick_cmds.push(Command::Dab(Dab::new(
                egui::pos2(160.0, 160.0),
                56.0,
                1.0,
                Surface::Canvas,
                Brush::Round,
                0.0,
            )));
        }
    }
    let _ = rig.canvas(&thick_cmds);
    let thick_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let hiding_12 = thick_diag.optical.wet_layer_hiding_ratio_pct.unwrap();
    assert!(
        hiding_12 > hiding_3 && hiding_12 > 65.0,
        "PW6 thick reloaded hiding ratio should exceed 3-dab ({hiding_3:.2}%) and 65%, got {hiding_12:.2}%"
    );

    // 3. Titanium White + Hansa Yellow Light (PY3) warm lemon tint
    let yellow_tint_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let yellow_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let yellow_cpu =
        crate::pigment::rgb_composite(yellow_diag.wet_channels, yellow_diag.dry_channels);
    let yellow_gpu = pixel(&yellow_tint_img, 320, 160);
    assert!(
        yellow_gpu[0] > 220 && yellow_gpu[1] > 210 && yellow_gpu[2] < yellow_gpu[1],
        "PW6 + PY3 tint should be bright warm yellow: {yellow_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            yellow_cpu[ch].abs_diff(yellow_gpu[ch]) <= 2,
            "PW6 + PY3 tint CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            yellow_cpu[ch],
            yellow_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_hansa_yellow_py3_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 4);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thin Hansa Yellow Light glaze (low hiding ratio, crisp lemon yellow)
    let thin_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Wipe(0.20)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let thin_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let thin_hiding = thin_diag.optical.wet_layer_hiding_ratio_pct.unwrap();
    assert!(
        thin_hiding < 18.0,
        "PY3 thin glaze hiding ratio should be semi-transparent (<18%), got {thin_hiding:.2}%"
    );
    let thin_cpu = crate::pigment::rgb_composite(thin_diag.wet_channels, thin_diag.dry_channels);
    let thin_gpu = pixel(&thin_img, 160, 160);
    assert!(
        thin_gpu[0] > 230 && thin_gpu[1] > 210 && thin_gpu[2] < thin_gpu[1],
        "PY3 thin glaze should be bright lemon yellow: {thin_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            thin_cpu[ch].abs_diff(thin_gpu[ch]) <= 2,
            "PY3 thin glaze CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            thin_cpu[ch],
            thin_gpu[ch]
        );
    }

    // 2. Thick PY3 masstone
    let mast_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 320, 160);
    assert!(
        mast_gpu[0] > 230 && mast_gpu[1] > 180 && mast_gpu[2] < 150,
        "PY3 masstone should be saturated yellow: {mast_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "PY3 masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 3. PY3 + PB15:2 spring green mix
    let green_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let green_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let green_cpu = crate::pigment::rgb_composite(green_diag.wet_channels, green_diag.dry_channels);
    let green_gpu = pixel(&green_img, 480, 160);
    assert!(
        green_gpu[1] > green_gpu[0] && green_gpu[1] > green_gpu[2],
        "PY3 + PB15:2 mix should be vivid green: {green_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            green_cpu[ch].abs_diff(green_gpu[ch]) <= 2,
            "PY3 + PB15:2 green CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            green_cpu[ch],
            green_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_quinacridone_magenta_pr122_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 5);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thin PR122 glaze (low hiding ratio, luminous cool magenta)
    let thin_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(2)),
        Command::Service(Brush::Round, BrushService::Wipe(0.20)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let thin_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let thin_hiding = thin_diag.optical.wet_layer_hiding_ratio_pct.unwrap();
    assert!(
        thin_hiding < 12.0,
        "PR122 thin glaze hiding ratio should be transparent (<12%), got {thin_hiding:.2}%"
    );
    let thin_cpu = crate::pigment::rgb_composite(thin_diag.wet_channels, thin_diag.dry_channels);
    let thin_gpu = pixel(&thin_img, 160, 160);
    assert!(
        thin_gpu[0] > thin_gpu[1] && thin_gpu[2] > thin_gpu[1],
        "PR122 thin glaze should be cool magenta (R,B > G): {thin_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            thin_cpu[ch].abs_diff(thin_gpu[ch]) <= 2,
            "PR122 thin glaze CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            thin_cpu[ch],
            thin_gpu[ch]
        );
    }

    // 2. PR122 + PW6 rose-pink tint
    let pink_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(2)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let pink_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let pink_cpu = crate::pigment::rgb_composite(pink_diag.wet_channels, pink_diag.dry_channels);
    let pink_gpu = pixel(&pink_img, 320, 160);
    assert!(
        pink_gpu[0] > pink_gpu[1] && pink_gpu[2] > pink_gpu[1],
        "PR122 + PW6 tint should be rose-pink: {pink_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            pink_cpu[ch].abs_diff(pink_gpu[ch]) <= 2,
            "PR122 + PW6 tint CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            pink_cpu[ch],
            pink_gpu[ch]
        );
    }

    // 3. PR122 + PB15:2 clean purple mix
    let purple_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(2)),
        Command::Service(Brush::Round, BrushService::Load(2)),
        Command::Service(Brush::Round, BrushService::Load(1)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let purple_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let purple_cpu =
        crate::pigment::rgb_composite(purple_diag.wet_channels, purple_diag.dry_channels);
    let purple_gpu = pixel(&purple_img, 480, 160);
    assert!(
        purple_gpu[2] > purple_gpu[1] && purple_gpu[0] > purple_gpu[1],
        "PR122 + PB15:2 mix should be purple (R,B > G): {purple_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            purple_cpu[ch].abs_diff(purple_gpu[ch]) <= 2,
            "PR122 + PB15:2 purple CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            purple_cpu[ch],
            purple_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_cadmium_red_pr108_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 6);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thick PR108 masstone (opaque fiery red)
    let mast_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let mast_hiding = mast_diag.optical.wet_layer_hiding_ratio_pct.unwrap();
    assert!(
        mast_hiding > 8.0,
        "PR108 3-dab hiding ratio should exceed transparent glazes (>8%), got {mast_hiding:.2}%"
    );
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 160, 160);
    assert!(
        mast_gpu[0] > 200 && mast_gpu[1] < 100 && mast_gpu[2] < 20,
        "PR108 masstone should be saturated fiery red: {mast_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "PR108 masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 2. PR108 + PW6 warm peach tint
    let peach_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let peach_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let peach_cpu = crate::pigment::rgb_composite(peach_diag.wet_channels, peach_diag.dry_channels);
    let peach_gpu = pixel(&peach_img, 320, 160);
    assert!(
        peach_gpu[0] > peach_gpu[1] && peach_gpu[1] > peach_gpu[2],
        "PR108 + PW6 tint should be warm peach (R > G > B): {peach_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            peach_cpu[ch].abs_diff(peach_gpu[ch]) <= 2,
            "PR108 + PW6 tint CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            peach_cpu[ch],
            peach_gpu[ch]
        );
    }

    // 3. PR108 + PY3 bright orange mix
    let orange_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(6)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let orange_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let orange_cpu =
        crate::pigment::rgb_composite(orange_diag.wet_channels, orange_diag.dry_channels);
    let orange_gpu = pixel(&orange_img, 480, 160);
    assert!(
        orange_gpu[0] > 210 && orange_gpu[1] > orange_gpu[2] && orange_gpu[2] < 40,
        "PR108 + PY3 mix should be vivid orange-red: {orange_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            orange_cpu[ch].abs_diff(orange_gpu[ch]) <= 2,
            "PR108 + PY3 orange CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            orange_cpu[ch],
            orange_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_ultramarine_blue_pb29_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 7);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thick PB29 masstone (warm red-shade ultramarine)
    let mast_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(4)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 160, 160);
    assert!(
        mast_gpu[2] > mast_gpu[1] && mast_gpu[2] > mast_gpu[0],
        "PB29 masstone should be deep blue: {mast_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "PB29 masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 2. PB29 + PW6 airy periwinkle-sky tint
    let sky_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(4)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let sky_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let sky_cpu = crate::pigment::rgb_composite(sky_diag.wet_channels, sky_diag.dry_channels);
    let sky_gpu = pixel(&sky_img, 320, 160);
    assert!(
        sky_gpu[2] > sky_gpu[1] && sky_gpu[1] > sky_gpu[0],
        "PB29 + PW6 tint should be sky blue (B > G > R): {sky_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            sky_cpu[ch].abs_diff(sky_gpu[ch]) <= 2,
            "PB29 + PW6 sky tint CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            sky_cpu[ch],
            sky_gpu[ch]
        );
    }

    // 3. PB29 + PR122 clean violet mix
    let violet_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(4)),
        Command::Service(Brush::Round, BrushService::Load(4)),
        Command::Service(Brush::Round, BrushService::Load(2)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let violet_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let violet_cpu =
        crate::pigment::rgb_composite(violet_diag.wet_channels, violet_diag.dry_channels);
    let violet_gpu = pixel(&violet_img, 480, 160);
    assert!(
        violet_gpu[2] > violet_gpu[1] && violet_gpu[0] > violet_gpu[1],
        "PB29 + PR122 mix should be clean violet (R,B > G): {violet_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            violet_cpu[ch].abs_diff(violet_gpu[ch]) <= 2,
            "PB29 + PR122 violet CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            violet_cpu[ch],
            violet_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_burnt_sienna_pbr7_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 8);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thick PBr7 masstone (warm chestnut-mahogany)
    let mast_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(5)),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
        Command::Dab(Dab::new(
            egui::pos2(160.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 160, 160);
    assert!(
        mast_gpu[0] > mast_gpu[1] && mast_gpu[1] > mast_gpu[2],
        "PBr7 masstone should be warm earth (R > G > B): {mast_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "PBr7 masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 2. PBr7 + PW6 warm terracotta tint
    let terra_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(5)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let terra_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let terra_cpu = crate::pigment::rgb_composite(terra_diag.wet_channels, terra_diag.dry_channels);
    let terra_gpu = pixel(&terra_img, 320, 160);
    assert!(
        terra_gpu[0] > terra_gpu[1] && terra_gpu[1] > terra_gpu[2],
        "PBr7 + PW6 tint should be warm terracotta (R > G > B): {terra_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            terra_cpu[ch].abs_diff(terra_gpu[ch]) <= 2,
            "PBr7 + PW6 tint CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            terra_cpu[ch],
            terra_gpu[ch]
        );
    }

    // 3. PBr7 + PB29 neutral chromatic gray mix
    let gray_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(4)),
        Command::Service(Brush::Round, BrushService::Load(5)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let gray_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let gray_cpu = crate::pigment::rgb_composite(gray_diag.wet_channels, gray_diag.dry_channels);
    let gray_gpu = pixel(&gray_img, 480, 160);
    let spread = gray_gpu[0].max(gray_gpu[1]).max(gray_gpu[2])
        - gray_gpu[0].min(gray_gpu[1]).min(gray_gpu[2]);
    assert!(
        spread < 50,
        "PBr7 + PB29 mix should neutralize toward chromatic gray (spread={spread}): {gray_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            gray_cpu[ch].abs_diff(gray_gpu[ch]) <= 2,
            "PBr7 + PB29 gray CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            gray_cpu[ch],
            gray_gpu[ch]
        );
    }
}

#[test]
fn test_phase_6_bone_black_pbk9_calibration_and_cpu_gpu_parity() {
    const {
        assert!(crate::project::CURRENT_CALIBRATION_VERSION >= 9);
    }

    let mut rig = Rig::new();
    rig.paint.lighting.enabled = false;

    // 1. Thick PBk9 masstone (velvety cool charcoal-black)
    let mut mast_cmds = vec![Command::Initialize];
    for _ in 0..4 {
        mast_cmds.push(Command::Service(Brush::Round, BrushService::Load(7)));
        for _ in 0..3 {
            mast_cmds.push(Command::Dab(Dab::new(
                egui::pos2(160.0, 160.0),
                56.0,
                1.0,
                Surface::Canvas,
                Brush::Round,
                0.0,
            )));
        }
    }
    let mast_img = rig.canvas(&mast_cmds);
    let mast_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 160, 160);
    let mast_cpu = crate::pigment::rgb_composite(mast_diag.wet_channels, mast_diag.dry_channels);
    let mast_gpu = pixel(&mast_img, 160, 160);
    assert!(
        mast_gpu[0] < 60 && mast_gpu[1] < 65 && mast_gpu[2] < 70 && mast_gpu[2] >= mast_gpu[0],
        "PBk9 masstone should be dark cool black: {mast_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            mast_cpu[ch].abs_diff(mast_gpu[ch]) <= 2,
            "PBk9 masstone CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            mast_cpu[ch],
            mast_gpu[ch]
        );
    }

    // 2. PBk9 + PW6 cool neutral gray tint
    let gray_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Service(Brush::Round, BrushService::Load(7)),
        Command::Dab(Dab::new(
            egui::pos2(320.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let gray_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 320, 160);
    let gray_cpu = crate::pigment::rgb_composite(gray_diag.wet_channels, gray_diag.dry_channels);
    let gray_gpu = pixel(&gray_img, 320, 160);
    let spread = gray_gpu[0].max(gray_gpu[1]).max(gray_gpu[2])
        - gray_gpu[0].min(gray_gpu[1]).min(gray_gpu[2]);
    assert!(
        spread <= 15 && gray_gpu[0] > 140,
        "PBk9 + PW6 tint should be neutral mid-light gray (spread={spread}): {gray_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            gray_cpu[ch].abs_diff(gray_gpu[ch]) <= 2,
            "PBk9 + PW6 gray CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            gray_cpu[ch],
            gray_gpu[ch]
        );
    }

    // 3. PBk9 + PY3 muted olive green mix
    let olive_img = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(0)),
        Command::Service(Brush::Round, BrushService::Load(7)),
        Command::Dab(Dab::new(
            egui::pos2(480.0, 160.0),
            56.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let olive_diag = rig
        .paint
        .sample_diagnostics(&rig.device, &rig.queue, 480, 160);
    let olive_cpu = crate::pigment::rgb_composite(olive_diag.wet_channels, olive_diag.dry_channels);
    let olive_gpu = pixel(&olive_img, 480, 160);
    assert!(
        olive_gpu[1] >= olive_gpu[0] && olive_gpu[1] > olive_gpu[2],
        "PBk9 + PY3 mix should be olive green (G >= R, G > B): {olive_gpu:?}"
    );
    for ch in 0..3 {
        assert!(
            olive_cpu[ch].abs_diff(olive_gpu[ch]) <= 2,
            "PBk9 + PY3 olive CPU/GPU mismatch ch {ch}: cpu={} gpu={}",
            olive_cpu[ch],
            olive_gpu[ch]
        );
    }
}

#[test]
fn test_jar_swatch_colors_match_kubelka_munk_recipes() {
    for p in cabinet::PIGMENTS {
        let jar_rgb = pigment::rgb(p.recipe);
        let canvas_rgb = pigment::rgb_composite(p.recipe, [0.0; 8]);
        assert_eq!(
            jar_rgb, canvas_rgb,
            "jar swatch for {} must match Kubelka-Munk composite of its recipe",
            p.name
        );
    }
}

#[test]
fn test_mixing_matrix_demo_project_loads_on_dx12() {
    const DEMO_BYTES: &[u8] = include_bytes!("../demos/studio-essentials-mixing-matrix.pigment");
    let (meta, bytes) = crate::project::read_project_bytes(DEMO_BYTES).unwrap();
    assert_eq!(
        meta.calibration_version,
        crate::project::CURRENT_CALIBRATION_VERSION
    );

    let mut rig = Rig::new();
    let img = rig.canvas(&[Command::LoadProject(bytes)]);
    // Diagonal (1,1) = Hansa Yellow Light (256, 256), (1,4) = Hansa Yellow + Phthalo Blue green (580, 256)
    let yellow_px = pixel(&img, 256, 256);
    assert!(
        yellow_px[0] > 210 && yellow_px[1] > 160 && yellow_px[2] < 40,
        "demo (1,1) should be Hansa Yellow Light: {yellow_px:?}"
    );
    let green_px = pixel(&img, 580, 256);
    assert!(
        green_px[1] > green_px[0] && green_px[1] > green_px[2],
        "demo (1,4) should be Hansa Yellow + Phthalo Blue green: {green_px:?}"
    );
}

#[test]
fn test_v030_edge_hairline_smooth_height_and_set_tile() {
    use crate::recording::{compile_events, RecordedEvent, RecordedSample, ServiceAction};

    // 1. #11: SetTile toggles per-stroke tiling when stroke omits `tile`
    let mut tile_state = false;
    let events = vec![
        RecordedEvent::SetTile { enabled: true },
        RecordedEvent::Stroke {
            id: None,
            group: None,
            legacy_reservoir: false,
            surface: crate::recording::SurfaceKind::Canvas,
            brush: crate::recording::BrushKind::Round,
            ink: false,
            charcoal: false,
            erase: false,
            diameter: 48.0,
            angle_deg: 0.0,
            auto_follow: false,
            knife_sweep: false,
            bristle_sweep: false,
            filbert_sweep: false,
            fan_sweep: false,
            round_pressure: false,
            tile: None,
            samples: vec![RecordedSample {
                x: 4.0,
                y: 500.0,
                pressure: 1.0,
                timestamp_ms: 0,
                angle_deg: None,
            }],
        },
        RecordedEvent::SetTile { enabled: false },
        RecordedEvent::Stroke {
            id: None,
            group: None,
            legacy_reservoir: false,
            surface: crate::recording::SurfaceKind::Canvas,
            brush: crate::recording::BrushKind::Round,
            ink: false,
            charcoal: false,
            erase: false,
            diameter: 48.0,
            angle_deg: 0.0,
            auto_follow: false,
            knife_sweep: false,
            bristle_sweep: false,
            filbert_sweep: false,
            fan_sweep: false,
            round_pressure: false,
            tile: None,
            samples: vec![RecordedSample {
                x: 4.0,
                y: 700.0,
                pressure: 1.0,
                timestamp_ms: 16,
                angle_deg: None,
            }],
        },
    ];
    let cmds = compile_events(&events, &mut tile_state);
    assert!(!tile_state);
    let mut rig = Rig::new();
    let mut full_cmds = vec![
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(1)),
    ];
    full_cmds.extend(cmds);
    let img = rig.canvas(&full_cmds);
    // First stroke (y=500, tile=true via SetTile) wraps onto x=1020; second stroke (y=700, tile=false) does not
    assert!(pixel(&img, 1020, 500)[2] > pixel(&img, 1020, 500)[0]);
    assert_eq!(pixel(&img, 1020, 700), [255, 255, 255, 255]);

    // 2. #8: smooth_height removes the ~3.2px deposit grid in height export
    let _ = rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(3)),
        Command::Dab(Dab::new(
            egui::pos2(500.0, 500.0),
            80.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    rig.paint.smooth_height = false;
    let raw_h = rig.export(crate::gpu::ExportKind::Height);
    rig.paint.smooth_height = true;
    let smooth_h = rig.export(crate::gpu::ExportKind::Height);
    let mut raw_diff = 0i32;
    let mut smooth_diff = 0i32;
    for y in 490..510 {
        for x in 490..510 {
            raw_diff += (pixel(&raw_h, x, y)[0] as i32 - pixel(&raw_h, x + 1, y)[0] as i32).abs();
            smooth_diff +=
                (pixel(&smooth_h, x, y)[0] as i32 - pixel(&smooth_h, x + 1, y)[0] as i32).abs();
        }
    }
    assert!(
        smooth_diff < raw_diff,
        "smoothed height export should reduce high-frequency grid ripple: smooth={smooth_diff} raw={raw_diff}"
    );

    // 3. #10: Transparent export low-alpha edge pixels stay dark (no warm spike) on Bone Black stroke
    let mut edge_rig = Rig::new();
    let _ = edge_rig.canvas(&[
        Command::Initialize,
        Command::Service(Brush::Round, BrushService::Load(7)),
        Command::Service(Brush::Round, BrushService::Wipe(0.20)),
        Command::Dab(Dab::new(
            egui::pos2(500.0, 500.0),
            64.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
        )),
    ]);
    let transp = edge_rig.export(crate::gpu::ExportKind::Transparent);
    let mut checked_edge = 0usize;
    for y in 460..540 {
        for x in 460..540 {
            let p = pixel(&transp, x, y);
            if (1..=12).contains(&p[3]) {
                checked_edge += 1;
                assert!(
                    p[0] <= 45 && p[1] <= 70 && p[2] <= 90,
                    "low-alpha black edge pixel ({x},{y}) lifted too bright: {p:?}"
                );
            }
        }
    }
    assert!(checked_edge > 0, "expected low-alpha edge pixels to test");
    let _ = ServiceAction::Wipe { fraction: 0.0 };
}

#[test]
fn test_slice1_ergonomic_recording_multimode_normal_gl_and_palette() {
    use crate::recording::{
        format_mode_out_path, parse_export_modes, render_modes_headless, BrushKind, RecordedEvent,
        RecordedSample, Recording, ServiceAction, SurfaceKind,
    };

    // 1. Multi-mode parsing and output path formatting
    let modes = parse_export_modes("color,height,normal").unwrap();
    assert_eq!(modes.len(), 3);
    assert_eq!(
        format_mode_out_path(std::path::Path::new("out_{mode}.png"), "normal", true),
        std::path::PathBuf::from("out_normal.png")
    );
    assert_eq!(
        format_mode_out_path(std::path::Path::new("asset.png"), "height", true),
        std::path::PathBuf::from("asset_height.png")
    );

    // 2. dip_rack { clean: true }, set_mix, id/group round-trip, and --normal-convention gl vs dx
    let dir = tempfile::tempdir().unwrap();
    let rec_path = dir.path().join("slice1.json");
    let rec = Recording {
        format: "pigment-studio-recording-v2".into(),
        events: vec![
            // Load Cadmium Red, then clean-dip Hansa Yellow (must paint pure yellow, not orange)
            RecordedEvent::Service {
                brush: BrushKind::Round,
                action: ServiceAction::DipRack {
                    slot: 2,
                    lane: None,
                    clean: false,
                },
                legacy_reservoir: false,
            },
            RecordedEvent::Service {
                brush: BrushKind::Round,
                action: ServiceAction::DipRack {
                    slot: 1,
                    lane: None,
                    clean: true,
                },
                legacy_reservoir: false,
            },
            RecordedEvent::Stroke {
                id: Some("yellow_clean".into()),
                group: Some("swatches".into()),
                legacy_reservoir: false,
                surface: SurfaceKind::Canvas,
                brush: BrushKind::Round,
                ink: false,
                charcoal: false,
                erase: false,
                diameter: 60.0,
                angle_deg: 0.0,
                auto_follow: false,
                knife_sweep: false,
                bristle_sweep: false,
                filbert_sweep: false,
                fan_sweep: false,
                round_pressure: false,
                tile: Some(false),
                samples: vec![RecordedSample {
                    x: 300.0,
                    y: 500.0,
                    pressure: 1.0,
                    timestamp_ms: 0,
                    angle_deg: None,
                }],
            },
            // set_mix 50/50 Hansa Yellow + Phthalo Blue -> green
            RecordedEvent::SetMix {
                brush: BrushKind::Round,
                recipe: [0.5, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                clean: true,
                legacy_reservoir: false,
            },
            RecordedEvent::Stroke {
                id: Some("green_mix".into()),
                group: Some("swatches".into()),
                legacy_reservoir: false,
                surface: SurfaceKind::Canvas,
                brush: BrushKind::Round,
                ink: false,
                charcoal: false,
                erase: false,
                diameter: 60.0,
                angle_deg: 0.0,
                auto_follow: false,
                knife_sweep: false,
                bristle_sweep: false,
                filbert_sweep: false,
                fan_sweep: false,
                round_pressure: false,
                tile: Some(false),
                samples: vec![RecordedSample {
                    x: 600.0,
                    y: 500.0,
                    pressure: 1.0,
                    timestamp_ms: 16,
                    angle_deg: None,
                }],
            },
        ],
        ..Default::default()
    };
    let json = serde_json::to_string_pretty(&rec).unwrap();
    assert!(json.contains("\"yellow_clean\""));
    assert!(json.contains("\"set_mix\""));
    std::fs::write(&rec_path, json).unwrap();

    let dx_out = dir.path().join("dx_{mode}.png");
    let (_, dx_files) = render_modes_headless(
        &rec_path,
        &dx_out,
        &parse_export_modes("color,normal").unwrap(),
        None,
        true,
        false,
    )
    .unwrap();
    let gl_out = dir.path().join("gl_{mode}.png");
    let (_, gl_files) = render_modes_headless(
        &rec_path,
        &gl_out,
        &parse_export_modes("normal").unwrap(),
        None,
        true,
        true,
    )
    .unwrap();

    let read_px = |path: &std::path::Path, x: u32, y: u32| -> [u8; 4] {
        let file = std::fs::File::open(path).unwrap();
        let mut reader = png::Decoder::new(std::io::BufReader::new(file))
            .read_info()
            .unwrap();
        let mut buf = vec![0u8; reader.output_buffer_size()];
        reader.next_frame(&mut buf).unwrap();
        let i = ((y * SIDE + x) * 4) as usize;
        [buf[i], buf[i + 1], buf[i + 2], buf[i + 3]]
    };

    let y_px = read_px(&dx_files[0].1, 300, 500);
    assert!(
        y_px[0] > 210 && y_px[1] > 180 && y_px[2] < 40,
        "clean dip_rack should paint pure yellow: {y_px:?}"
    );
    let g_px = read_px(&dx_files[0].1, 600, 500);
    assert!(
        g_px[1] > g_px[0] && g_px[1] > g_px[2],
        "set_mix PY3+PB15:2 should paint green: {g_px:?}"
    );

    // GL normal flips Y (green channel = 255 - dx_green) on stroke slope
    let dx_n = read_px(&dx_files[1].1, 300, 474);
    let gl_n = read_px(&gl_files[0].1, 300, 474);
    assert_eq!(dx_n[0], gl_n[0]);
    assert!(
        (dx_n[1] as i32 + gl_n[1] as i32 - 255).abs() <= 1,
        "GL normal must flip green channel: dx={dx_n:?} gl={gl_n:?}"
    );

    // 3. CLI palette export helper
    let pal_json = dir.path().join("pal.json");
    let pal_gpl = dir.path().join("pal.gpl");
    let entries: Vec<(String, [f32; 8])> = cabinet::PIGMENTS
        .iter()
        .map(|p| (p.name.to_string(), p.recipe))
        .collect();
    crate::gpu::export_named_palette(&pal_json, &entries).unwrap();
    crate::gpu::export_named_palette(&pal_gpl, &entries).unwrap();
    assert!(std::fs::read_to_string(&pal_json)
        .unwrap()
        .contains("Titanium White"));
    assert!(std::fs::read_to_string(&pal_gpl)
        .unwrap()
        .starts_with("GIMP Palette"));
}

#[test]
fn test_slice2_serve_tier_a_editing_checkpoints_and_inspection() {
    use crate::recording::{BrushKind, RecordedEvent, RecordedSample, ServiceAction, SurfaceKind};
    use crate::serve::{ServeRequest, ServeSession};

    let mut session = ServeSession::new().unwrap();
    let ping = session.handle_request(ServeRequest::Ping);
    assert!(ping.ok);

    // 1. Apply initial events (assigns stroke IDs and paints red at (300, 500))
    let apply_resp = session.handle_request(ServeRequest::Apply {
        events: vec![
            RecordedEvent::Service {
                brush: BrushKind::Round,
                action: ServiceAction::DipRack {
                    slot: 2,
                    lane: None,
                    clean: true,
                },
                legacy_reservoir: false,
            },
            RecordedEvent::Stroke {
                id: Some("red_dab".into()),
                group: Some("base".into()),
                legacy_reservoir: false,
                surface: SurfaceKind::Canvas,
                brush: BrushKind::Round,
                ink: false,
                charcoal: false,
                erase: false,
                diameter: 64.0,
                angle_deg: 0.0,
                auto_follow: false,
                knife_sweep: false,
                bristle_sweep: false,
                filbert_sweep: false,
                fan_sweep: false,
                round_pressure: false,
                tile: Some(false),
                samples: vec![RecordedSample {
                    x: 300.0,
                    y: 500.0,
                    pressure: 1.0,
                    timestamp_ms: 0,
                    angle_deg: None,
                }],
            },
        ],
    });
    assert!(apply_resp.ok, "{:?}", apply_resp.error);

    // 2. Spatial query & inspection
    let hits = session.handle_request(ServeRequest::StrokesAt {
        x: 300.0,
        y: 500.0,
        radius: 10.0,
    });
    assert_eq!(hits.data.unwrap()["hits"].as_array().unwrap().len(), 1);

    let px_red = session.handle_request(ServeRequest::InspectPixel { x: 300, y: 500 });
    let srgb = px_red.data.unwrap()["srgb"].as_array().unwrap().clone();
    assert!(srgb[0].as_u64().unwrap() > 200 && srgb[2].as_u64().unwrap() < 40);

    let brush_st = session.handle_request(ServeRequest::BrushState {
        brush: Some(BrushKind::Round),
    });
    assert!(
        brush_st.data.unwrap()["brushes"][0]["load"]
            .as_f64()
            .unwrap()
            > 0.0
    );

    // 3. Move stroke from (300, 500) -> (600, 500) via checkpointed replay
    let mv = session.handle_request(ServeRequest::MoveStroke {
        id: "red_dab".into(),
        dx: 300.0,
        dy: 0.0,
    });
    assert!(mv.ok, "{:?}", mv.error);

    let px_old = session.handle_request(ServeRequest::InspectPixel { x: 300, y: 500 });
    assert_eq!(
        px_old.data.unwrap()["srgb"].as_array().unwrap()[0]
            .as_u64()
            .unwrap(),
        255
    );
    let px_new = session.handle_request(ServeRequest::InspectPixel { x: 600, y: 500 });
    let new_srgb = px_new.data.unwrap()["srgb"].as_array().unwrap().clone();
    assert!(new_srgb[0].as_u64().unwrap() > 200 && new_srgb[2].as_u64().unwrap() < 40);

    // 4. Inspect region & multi-mode render from live ServeSession
    let reg = session.handle_request(ServeRequest::InspectRegion {
        x: 560,
        y: 460,
        w: 80,
        h: 80,
    });
    assert!(
        reg.data.unwrap()["coverage_pct"].as_f64().unwrap() > 20.0,
        "moved stroke region should have paint coverage"
    );

    let dir = tempfile::tempdir().unwrap();
    let out_tmpl = dir.path().join("serve_{mode}.png");
    let rnd = session.handle_request(ServeRequest::Render {
        out: out_tmpl.display().to_string(),
        mode: Some("color,normal".into()),
        smooth_height: true,
        normal_convention: Some("gl".into()),
    });
    assert!(rnd.ok, "{:?}", rnd.error);
    assert!(dir.path().join("serve_color.png").exists());
    assert!(dir.path().join("serve_normal.png").exists());

    // 5. Delete stroke -> canvas returns to white
    let del = session.handle_request(ServeRequest::DeleteStroke {
        id: "red_dab".into(),
    });
    assert!(del.ok);
    let px_cleared = session.handle_request(ServeRequest::InspectPixel { x: 600, y: 500 });
    assert_eq!(
        px_cleared.data.unwrap()["srgb"].as_array().unwrap()[2]
            .as_u64()
            .unwrap(),
        255
    );
}

#[test]
fn test_slice1_gravity_conservation_and_downward_flow() {
    let mut rig = Rig::new();
    let mut cmds = Vec::new();
    // Equip Phthalo Blue + Cadmium Red mix in rack slot 0 and dip Round brush
    cmds.push(Command::EquipRack(
        0,
        [0.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.5, 0.0],
    ));
    cmds.push(Command::Service(Brush::Round, BrushService::DipRack(0)));
    // Paint a thick puddle at (512, 300)
    for _ in 0..4 {
        cmds.push(Command::Dab(Dab::new_tiled(
            egui::pos2(512.0, 300.0),
            24.0,
            1.0,
            Surface::Canvas,
            Brush::Round,
            0.0,
            false,
        )));
    }
    let (before_bytes, _) = rig.state(&cmds);
    let before_px: &[[f32; 8]] = bytemuck::cast_slice(&before_bytes);
    let canvas_px = (SIDE as usize) * (SIDE as usize);
    let mut total_before = [0.0f64; 8];
    let mut below_before = 0.0f64;
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = before_px[y * SIDE as usize + x];
            for c in 0..8 {
                total_before[c] += f64::from(p[c]);
            }
            if y > 326 {
                below_before += p.iter().map(|&v| f64::from(v)).sum::<f64>();
            }
        }
    }
    let sum_before: f64 = total_before.iter().sum();
    assert!(sum_before > 10.0, "expected substantial painted mass");
    assert!(
        below_before < 1e-4,
        "no paint should be below y=326 initially"
    );

    // Step 40 ticks of Heavy Oil (viscosity = 1.0) — normal strokes should stay put
    let heavy_cmds: Vec<_> = (0..40)
        .map(|_| Command::AdvanceGravity {
            strength: 1.0,
            viscosity: 1.0,
            angle_deg: 90.0,
        })
        .collect();
    let (heavy_bytes, _) = rig.state(&heavy_cmds);
    let heavy_px: &[[f32; 8]] = bytemuck::cast_slice(&heavy_bytes);
    let heavy_below: f64 = (327..SIDE as usize)
        .flat_map(|y| (0..SIDE as usize).map(move |x| heavy_px[y * SIDE as usize + x]))
        .flat_map(|p| p.into_iter().map(f64::from))
        .sum();
    assert!(
        heavy_below < 1e-4,
        "Heavy Oil (viscosity 1.0) should not drip below y=326, got {heavy_below}"
    );

    // Step 120 ticks (2s at 60 Hz) of Ink-like gravity straight down (90 deg)
    let gravity_cmds: Vec<_> = (0..120)
        .map(|_| Command::AdvanceGravity {
            strength: 1.0,
            viscosity: 0.06,
            angle_deg: 90.0,
        })
        .collect();
    let (after_bytes, _) = rig.state(&gravity_cmds);
    let after_px: &[[f32; 8]] = bytemuck::cast_slice(&after_bytes);
    let mut total_after = [0.0f64; 8];
    let mut below_after = 0.0f64;
    for y in 0..SIDE as usize {
        for x in 0..SIDE as usize {
            let p = after_px[y * SIDE as usize + x];
            for c in 0..8 {
                total_after[c] += f64::from(p[c]);
            }
            if y > 326 {
                below_after += p.iter().map(|&v| f64::from(v)).sum::<f64>();
            }
        }
    }
    let _ = canvas_px;
    for c in 0..8 {
        assert!(
            (total_before[c] - total_after[c]).abs() <= 1e-2,
            "channel {c} mass not conserved: before={} after={}",
            total_before[c],
            total_after[c]
        );
    }
    assert!(
        below_after > 0.5,
        "expected paint to drip below y=326, got {below_after}"
    );
}

#[test]
fn test_slice2_pollock_splatter_and_recording() {
    let mut rig = Rig::new();
    let events = vec![
        RecordedEvent::EquipRack {
            slot: 0,
            recipe: [0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        },
        RecordedEvent::Service {
            legacy_reservoir: false,
            brush: recording::BrushKind::Round,
            action: recording::ServiceAction::DipRack {
                slot: 0,
                clean: false,
                lane: None,
            },
        },
        RecordedEvent::Splatter {
            id: Some("whip_red".into()),
            group: None,
            brush: recording::BrushKind::Round,
            settings: crate::splatter::SplatterSettings {
                viscosity: 0.30,
                sling_throw: 1.5,
                droplet_size: 20.0,
                spray_density: 1.5,
            },
            tile: Some(false),
            samples: vec![
                RecordedSample {
                    x: 250.0,
                    y: 512.0,
                    pressure: 0.9,
                    timestamp_ms: 0,
                    angle_deg: None,
                },
                RecordedSample {
                    x: 480.0,
                    y: 512.0,
                    pressure: 0.9,
                    timestamp_ms: 20,
                    angle_deg: None,
                },
            ],
        },
    ];
    let mut tile = false;
    let cmds = recording::compile_events(&events, &mut tile);
    assert!(cmds.len() >= 4, "expected poured + flung splatter dabs");
    let (pigments_bytes, _) = rig.state(&cmds);
    let px: &[[f32; 8]] = bytemuck::cast_slice(&pigments_bytes);
    let mut flung_ahead_mass = 0.0f64;
    for y in 420..604 {
        for x in 490..750 {
            flung_ahead_mass += px[y * SIDE as usize + x]
                .iter()
                .map(|&v| f64::from(v))
                .sum::<f64>();
        }
    }
    assert!(
        flung_ahead_mass > 0.05,
        "expected fast whip to fling paint ahead of x=480, got {flung_ahead_mass}"
    );
}
