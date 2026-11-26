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
    for p in values(a).chain(values(&b[..128])) {
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
    assert_eq!(&restored[..128], &streaked[..128]);
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
    assert_eq!(&restored[..128], &clean[..128]);

    let (_, custom_dipped) = rig.state(&[
        Command::Snapshot,
        Command::Service(Brush::Round, BrushService::DipCustom(0)),
    ]);
    let custom_load = values(&custom_dipped).next().unwrap();
    assert!(custom_load[1] > 10., "{custom_load:?}");
    let (_, restored_after_custom) = rig.state(&[Command::Undo]);
    assert_eq!(&restored_after_custom[..128], &clean[..128]);

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
    assert_eq!(&restored_after_rack[..128], &clean[..128]);

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
    // In masstone, green absorption is high, and red/blue deepen into ruby-carmine (G <= 15, R <= 140, B <= 85)
    assert!(
        mag_coat3[1] <= 15 && mag_coat3[0] <= 140 && mag_coat3[2] <= 85,
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

    let read_kind = |rig: &Rig, kind: crate::gpu::ExportKind| -> Vec<u8> {
        let mut encoder = rig.device.create_command_encoder(&Default::default());
        let buffer = rig
            .paint
            .readback_kind(&rig.device, &rig.queue, &mut encoder, kind);
        rig.queue.submit([encoder.finish()]);
        let (done, rx) = std::sync::mpsc::channel();
        buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = done.send(r);
        });
        let _ = rig.device.poll(wgpu::Maintain::Wait);
        rx.recv().unwrap().unwrap();
        let data = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        data
    };

    // 1. Transparent PNG export: bare pixel has alpha 0, painted center has high alpha
    let transparent = read_kind(&rig, crate::gpu::ExportKind::Transparent);
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
        let col = read_kind(&color_rig, crate::gpu::ExportKind::Color);
        let trn = read_kind(&color_rig, crate::gpu::ExportKind::Transparent);
        let c_px = pixel(&col, 500, 500);
        let t_px = pixel(&trn, 500, 500);
        eprintln!("{name}: Color={c_px:?}, Transparent={t_px:?}");
        let a = t_px[3] as f32 / 255.0;
        for ch in 0..3 {
            let over_white = (t_px[ch] as f32 * a + 255.0 * (1.0 - a)).round() as i32;
            let diff = (c_px[ch] as i32 - over_white).abs();
            assert!(
                diff <= 20,
                "{name} channel {ch} over-white drifted: Color={c_px:?}, Transparent={t_px:?}, OverWhite={over_white}"
            );
        }
        assert!(
            t_px[3] >= 200,
            "{name} dab center should have strong opacity: {t_px:?}"
        );
    }

    // 2. Height map export: painted center is higher than bare canvas and grayscale
    let height = read_kind(&rig, crate::gpu::ExportKind::Height);
    let bare_h = pixel(&height, 10, 10);
    let center_h = pixel(&height, 500, 500);
    assert_eq!(bare_h[0], bare_h[1]);
    assert_eq!(bare_h[1], bare_h[2]);
    assert!(
        center_h[0] > bare_h[0],
        "painted impasto height ({center_h:?}) must exceed bare canvas ({bare_h:?})"
    );

    // 3. Normal map export: slopes tilt outward around (500, 500)
    let normal = read_kind(&rig, crate::gpu::ExportKind::Normal);
    let left_n = pixel(&normal, 482, 500);
    let right_n = pixel(&normal, 518, 500);
    assert!(
        left_n[0] < right_n[0] && left_n[2] > 128 && right_n[2] > 128,
        "normal X must tilt left/right across dab slopes: left={left_n:?}, right={right_n:?}"
    );

    // 4. Verify display was restored to opaque white on bare canvas after export
    let restored = read_kind(&rig, crate::gpu::ExportKind::Color);
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
                brush: BrushKind::Round,
                action: ServiceAction::Load { tube: 6 },
            },
            RecordedEvent::Stroke {
                surface: SurfaceKind::Canvas,
                brush: BrushKind::Round,
                ink: false,
                charcoal: false,
                erase: false,
                diameter: 48.0,
                angle_deg: 0.0,
                tile: false,
                samples: vec![
                    RecordedSample {
                        x: 400.0,
                        y: 500.0,
                        pressure: 1.0,
                        timestamp_ms: 0,
                    },
                    RecordedSample {
                        x: 600.0,
                        y: 500.0,
                        pressure: 1.0,
                        timestamp_ms: 16,
                    },
                ],
            },
        ],
        ..Default::default()
    };
    std::fs::write(&rec_path, serde_json::to_string_pretty(&rec).unwrap()).unwrap();
    let count = render_headless(&rec_path, &out_path, crate::gpu::ExportKind::Color).unwrap();
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
                brush: BrushKind::Round,
                action: ServiceAction::Load { tube: 1 },
            });
        }
        let x = 100.0 + step as f32 * 60.0;
        events.push(RecordedEvent::Stroke {
            surface: SurfaceKind::Canvas,
            brush: BrushKind::Round,
            ink: false,
            charcoal: false,
            erase: false,
            diameter: 32.0,
            angle_deg: 0.0,
            tile: false,
            samples: vec![RecordedSample {
                x,
                y: 500.0,
                pressure: 1.0,
                timestamp_ms: step as u32 * 20,
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
    let mut jars = vec![false; crate::workbench::CUSTOM_JAR_COUNT];
    jars[0] = true;
    let meta = crate::project::ProjectMeta {
        loads: [0.5, 0.0],
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

    // Undo restores clean canvas
    let undone = rig2.canvas(&[Command::Undo]);
    assert_eq!(pixel(&undone, 450, 450), [255, 255, 255, 255]);
}
