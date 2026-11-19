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
        self.paint.encode(&self.queue, &mut encoder, commands);
        let buffer = self.paint.readback(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        pixels(&buffer, &self.device)
    }

    fn state(&mut self, commands: &[Command]) -> (Vec<u8>, Vec<u8>) {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.paint.encode(&self.queue, &mut encoder, commands);
        let (pigments, brushes) = self.paint.inspect(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        (
            pixels(&pigments, &self.device),
            pixels(&brushes, &self.device),
        )
    }

    fn check_tubes(&mut self, recipe: [u32; 4]) -> bool {
        let mut encoder = self.device.create_command_encoder(&Default::default());
        self.paint
            .encode(&self.queue, &mut encoder, &[Command::CheckTubes(recipe)]);
        let result = self.paint.placement_readback(&self.device, &mut encoder);
        self.queue.submit([encoder.finish()]);
        pixels(&result, &self.device)[0] != 0
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
        let expected = pigment::rgb(amount[..6].try_into().unwrap());
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
        Command::SaveSwatch(0, [96, 32]),
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

    assert!(rig.check_tubes([5, 4, 2, 4]));
    rig.canvas(&[Command::Tubes]);
    assert!(rig.check_tubes([6, 6, 0, 0]));
    rig.canvas(&[Command::Tubes]);
    assert!(!rig.check_tubes([1, 2, 0, 0]));
    rig.canvas(&[Command::Snapshot, Command::Clear(Surface::Palette)]);
    assert!(rig.check_tubes([1, 2, 0, 0]));
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
        .recv_timeout(Duration::from_secs(10))
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
fn undo_restores_swatches_and_rinse_state() {
    let mut bench = workbench::Workbench::default();
    bench.snapshot();
    bench.session.rinsed[0] = true;
    bench.snapshot();
    bench.session.swatches[2] = true;
    assert!(bench.undo());
    assert!(bench.session.rinsed[0]);
    assert!(!bench.session.swatches[2]);
    assert!(bench.undo());
    assert!(!bench.session.rinsed[0]);
}
