use crate::gpu::{self, Brush};
use egui::{pos2, Color32, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, TextureId};

pub enum JarSwatch {
    Pigment(Color32),
    Mixture { texture: TextureId, uv: Rect },
}

pub fn jar(painter: &Painter, rect: Rect, swatch: JarSwatch) {
    if !painter.clip_rect().intersects(rect) {
        return;
    }

    let point = |x, y| {
        pos2(
            rect.left() + rect.width() * x / 40.,
            rect.top() + rect.height() * y / 36.,
        )
    };
    let area = |x0, y0, x1, y1| Rect::from_min_max(point(x0, y0), point(x1, y1));
    let scale = rect.width() / 40.;

    painter.rect_filled(area(5., 31., 35., 35.), 2. * scale, Color32::from_gray(17));
    let glass = area(3., 9., 37., 33.);
    painter.rect_filled(glass, 4. * scale, Color32::from_rgb(56, 57, 57));
    painter.rect_stroke(
        glass,
        4. * scale,
        Stroke::new(scale, Color32::from_rgb(113, 119, 116)),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [point(5.5, 14.), point(5.5, 28.)],
        Stroke::new(scale, Color32::from_rgb(145, 150, 143)),
    );
    painter.line_segment(
        [point(8., 31.), point(32., 31.)],
        Stroke::new(scale, Color32::from_rgb(96, 99, 95)),
    );

    // Keep the colour face flat and leave reflections on the glass around it.
    painter.rect_filled(area(7., 12., 34., 30.), 1. * scale, Color32::from_gray(23));
    let face = area(8., 13., 33., 29.);
    match swatch {
        JarSwatch::Pigment(color) => painter.rect_filled(face, 0., color),
        JarSwatch::Mixture { texture, uv } => painter.image(texture, face, uv, Color32::WHITE),
    };

    let lid = area(6., 2., 34., 10.5);
    painter.rect_filled(lid, 2. * scale, Color32::from_rgb(49, 48, 46));
    painter.rect_stroke(
        lid,
        2. * scale,
        Stroke::new(scale, Color32::from_rgb(109, 103, 93)),
        StrokeKind::Inside,
    );
    painter.line_segment(
        [point(9., 4.), point(31., 4.)],
        Stroke::new(scale, Color32::from_rgb(124, 115, 100)),
    );
    for x in [11., 15.5, 20., 24.5, 29.] {
        painter.line_segment(
            [point(x, 5.5), point(x, 8.5)],
            Stroke::new(scale, Color32::from_rgb(83, 79, 71)),
        );
    }
}

pub fn brush(painter: &Painter, origin: Pos2, brush: Brush, texture: TextureId, load: f32) {
    let point = |x, y| origin + egui::vec2(x, y);
    let polygon = |points: &[(f32, f32)]| points.iter().map(|&(x, y)| point(x, y)).collect();
    let handle = polygon(&[
        (2., 12.),
        (7., 10.5),
        (22., 11.),
        (31., 12.),
        (31., 18.),
        (16., 19.),
        (6., 18.7),
        (2., 17.),
        (1., 15.),
    ]);
    painter.add(Shape::convex_polygon(
        handle,
        Color32::from_rgb(122, 78, 58),
        Stroke::new(0.7_f32, Color32::from_rgb(68, 43, 36)),
    ));
    painter.line_segment(
        [point(5., 13.), point(23., 12.7)],
        Stroke::new(0.9_f32, Color32::from_rgb(176, 119, 83)),
    );

    let (outline, material): (&[(f32, f32)], Color32) = match brush {
        Brush::Round => (
            &[
                (32., 10.8),
                (39., 9.7),
                (45., 10.),
                (51., 11.4),
                (60., 14.9),
                (51., 18.5),
                (44., 20.),
                (38., 20.2),
                (32., 19.3),
            ],
            Color32::from_rgb(171, 128, 86),
        ),
        Brush::Flat => (
            &[
                (32., 9.5),
                (53., 7.7),
                (58., 8.),
                (58., 22.),
                (53., 22.3),
                (32., 20.5),
            ],
            Color32::from_rgb(193, 159, 114),
        ),
        Brush::Filbert => (
            &[
                (32., 9.),
                (48., 8.),
                (53., 8.7),
                (57., 11.),
                (59., 15.),
                (57., 19.),
                (53., 21.3),
                (48., 22.),
                (32., 21.),
            ],
            Color32::from_rgb(183, 136, 91),
        ),
        Brush::Fan => (
            &[
                (32., 11.5),
                (53., 2.5),
                (57., 5.3),
                (59.5, 9.6),
                (60.5, 15.),
                (59.5, 20.4),
                (57., 24.7),
                (53., 27.5),
                (32., 18.5),
            ],
            Color32::from_rgb(188, 151, 107),
        ),
        Brush::PaletteKnife => (
            &[
                (35., 12.),
                (46., 7.5),
                (54., 7.),
                (60., 9.5),
                (57.5, 13.),
                (48., 19.8),
                (37., 21.),
                (34., 18.),
            ],
            Color32::from_rgb(154, 168, 174),
        ),
        Brush::SoftMop => (
            &[
                (32., 10.),
                (39., 5.7),
                (46., 4.6),
                (52., 6.),
                (56., 9.8),
                (57., 15.),
                (56., 20.2),
                (52., 24.),
                (46., 25.4),
                (39., 24.3),
                (32., 20.),
            ],
            Color32::from_rgb(209, 184, 144),
        ),
    };
    let tip: Vec<Pos2> = polygon(outline);
    painter.add(Shape::convex_polygon(
        tip.clone(),
        material,
        Stroke::new(0.8_f32, Color32::from_gray(46)),
    ));

    if brush == Brush::PaletteKnife {
        painter.line_segment(
            [point(37., 13.), point(55., 8.9)],
            Stroke::new(0.9_f32, Color32::from_gray(218)),
        );
        painter.line_segment(
            [point(27., 15.), point(36.5, 17.5)],
            Stroke::new(3.0_f32, Color32::from_rgb(106, 120, 126)),
        );
        painter.line_segment(
            [point(27., 14.6), point(36.5, 16.9)],
            Stroke::new(0.9_f32, Color32::from_gray(199)),
        );
    } else {
        let base_y = outline[0].1;
        let tip_end = tip.iter().map(|p| p.x).fold(origin.x, f32::max) - origin.x;
        let spread = match brush {
            Brush::Fan => 8.5,
            Brush::SoftMop => 6.5,
            Brush::Flat => 5.5,
            Brush::Round | Brush::Filbert | Brush::PaletteKnife => 3.6,
        };
        for branch in [-1., 0., 1.] {
            painter.line_segment(
                [
                    point(34., 15. + branch * 2.),
                    point(tip_end - 6., 15. + branch * spread),
                ],
                Stroke::new(0.8_f32, Color32::from_black_alpha(70)),
            );
        }
        let ferrule = Rect::from_min_max(point(24., base_y - 0.6), point(34., 30.6 - base_y));
        painter.rect_filled(ferrule, 1.4, Color32::from_rgb(111, 123, 125));
        painter.hline(
            (ferrule.left() + 1.)..=(ferrule.right() - 1.),
            ferrule.top() + 1.8,
            Stroke::new(1.0_f32, Color32::from_rgb(185, 192, 185)),
        );
    }

    if load > 0.005 {
        paint_on_tip(painter, &tip, brush, texture, load.clamp(0., 1.));
    }
}

fn paint_on_tip(painter: &Painter, tip: &[Pos2], brush: Brush, texture: TextureId, load: f32) {
    let bounds = Rect::from_points(tip);
    let lanes = brush.load_lanes();
    let lane_height = bounds.height() / lanes.len() as f32;
    let center = bounds.center();
    let painted_width = (bounds.width() * load).max(1.5);

    // Clip each live pigment sample to its lane and to the tip silhouette.
    for (index, lane) in lanes.enumerate() {
        let top = bounds.top() + index as f32 * lane_height;
        let clip = Rect::from_min_max(
            pos2(bounds.right() - painted_width, top),
            pos2(bounds.right(), top + lane_height),
        );
        let uv = gpu::atlas_uv([512 + lane, gpu::SIDE, 1, 1]).center();
        let mut mesh = egui::Mesh::with_texture(texture);
        mesh.vertices
            .extend(tip.iter().map(|&point| egui::epaint::Vertex {
                pos: center + (point - center) * 0.95,
                uv,
                color: Color32::WHITE,
            }));
        for index in 1..tip.len() as u32 - 1 {
            mesh.add_triangle(0, index, index + 1);
        }
        painter.with_clip_rect(clip).add(Shape::mesh(mesh));
    }
}
