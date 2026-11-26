use crate::gpu::{Brush, Command, Dab, Surface};
use egui::Pos2;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Mouse,
    Pen,
}

#[derive(Clone, Copy)]
pub struct Sample {
    pub point: Pos2,
    pub pressure: f32,
    pub timestamp_ms: u32,
}

pub struct Stroke {
    pub source: Source,
    last: Sample,
    pressure_at_dab: f32,
    distance_since_dab: f32,
    diameter: f32,
    pub surface: Surface,
    brush: Brush,
    angle: f32,
    auto_angle: bool,
    load_ratio: f32,
    tile: bool,
    charcoal_erase: Option<bool>,
    ink_hold: Option<(Instant, Pos2)>,
}

impl Stroke {
    #[cfg(test)]
    pub fn start(
        source: Source,
        sample: Sample,
        diameter: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
        commands: &mut Vec<Command>,
    ) -> Self {
        Self::start_tiled(
            source, sample, diameter, surface, brush, angle, false, commands,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_tiled(
        source: Source,
        sample: Sample,
        diameter: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
        tile: bool,
        commands: &mut Vec<Command>,
    ) -> Self {
        Self::start_full(
            source, sample, diameter, surface, brush, angle, false, 1.0, tile, commands,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_full(
        source: Source,
        sample: Sample,
        diameter: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
        auto_angle: bool,
        load_ratio: f32,
        tile: bool,
        commands: &mut Vec<Command>,
    ) -> Self {
        let stroke = Self {
            source,
            last: sample,
            pressure_at_dab: sample.pressure,
            distance_since_dab: 0.,
            diameter,
            surface,
            brush,
            angle,
            auto_angle,
            load_ratio,
            tile,
            charcoal_erase: None,
            ink_hold: None,
        };
        stroke.dab(sample, commands);
        stroke
    }

    #[cfg(test)]
    pub fn start_ink(source: Source, sample: Sample, commands: &mut Vec<Command>) -> Self {
        Self::start_ink_tiled(source, sample, false, commands)
    }

    pub fn start_ink_tiled(
        source: Source,
        sample: Sample,
        tile: bool,
        commands: &mut Vec<Command>,
    ) -> Self {
        let stroke = Self {
            source,
            last: sample,
            pressure_at_dab: sample.pressure,
            distance_since_dab: 0.,
            diameter: 0.,
            surface: Surface::Canvas,
            brush: Brush::Round,
            angle: 0.,
            auto_angle: false,
            load_ratio: 1.0,
            tile,
            charcoal_erase: None,
            ink_hold: Some((Instant::now() + Duration::from_millis(34), sample.point)),
        };
        stroke.dab(sample, commands);
        stroke
    }

    pub fn start_charcoal(
        source: Source,
        sample: Sample,
        diameter: f32,
        erase: bool,
        tile: bool,
        commands: &mut Vec<Command>,
    ) -> Self {
        let stroke = Self {
            source,
            last: sample,
            pressure_at_dab: sample.pressure,
            distance_since_dab: 0.,
            diameter,
            surface: Surface::Canvas,
            brush: Brush::Round,
            angle: 0.,
            auto_angle: false,
            load_ratio: 1.0,
            tile,
            charcoal_erase: Some(erase),
            ink_hold: None,
        };
        stroke.dab(sample, commands);
        stroke
    }

    pub fn ink_is_held(&self) -> bool {
        self.ink_hold.is_some() && self.last.pressure > 0.56
    }

    pub fn dwell(&mut self, now: Instant, commands: &mut Vec<Command>) {
        let Some((deadline, previous)) = self.ink_hold else {
            return;
        };
        if now < deadline {
            return;
        }
        self.ink_hold = Some((now + Duration::from_millis(34), self.last.point));
        if self.ink_is_held() && previous.distance(self.last.point) < 3. {
            commands.push(Command::Dab(Dab::ink_tiled(
                self.last.point,
                self.last.pressure,
                true,
                self.tile,
            )));
        }
    }

    pub fn extend(&mut self, sample: Sample, commands: &mut Vec<Command>) {
        if sample.pressure <= 0.0 {
            self.last = sample;
            return;
        }
        if (sample.timestamp_ms.wrapping_sub(self.last.timestamp_ms) as i32) < 0 {
            return;
        }
        let distance = self.last.point.distance(sample.point);
        if self.auto_angle && self.brush.is_directional() && distance >= 1.5 {
            let delta = sample.point - self.last.point;
            let target = delta.y.atan2(delta.x) + std::f32::consts::FRAC_PI_2;
            let mut diff = (target - self.angle).rem_euclid(std::f32::consts::TAU);
            if diff > std::f32::consts::PI {
                diff -= std::f32::consts::TAU;
            }
            self.angle += diff * 0.45;
        }
        if distance == 0. {
            if (sample.pressure - self.pressure_at_dab).abs() >= 0.04 {
                self.dab(sample, commands);
                self.distance_since_dab = 0.;
                self.pressure_at_dab = sample.pressure;
            }
        } else {
            // Use the smaller endpoint radius so no interval exceeds one
            // quarter of the interpolated footprint's radius.
            let spacing = (0.25 * self.radius(sample.pressure.min(self.last.pressure))).max(0.75);
            let mut next = (spacing - self.distance_since_dab).max(0.);
            while next <= distance {
                let fraction = next / distance;
                let pressure = egui::lerp(self.last.pressure..=sample.pressure, fraction);
                self.dab(
                    Sample {
                        point: self.last.point.lerp(sample.point, fraction),
                        pressure,
                        timestamp_ms: sample.timestamp_ms,
                    },
                    commands,
                );
                self.pressure_at_dab = pressure;
                next += spacing;
            }
            self.distance_since_dab = spacing + distance - next;
        }
        self.last = sample;
    }

    pub fn finish(self, commands: &mut Vec<Command>) {
        if self.distance_since_dab > 0.01 {
            self.dab(self.last, commands);
        }
    }

    fn radius(&self, pressure: f32) -> f32 {
        if self.ink_hold.is_some() {
            return Dab::ink_radius(pressure);
        }
        (0.5 * self.diameter * (0.22 + 0.78 * pressure.sqrt())).max(0.75)
    }

    fn dab(&self, sample: Sample, commands: &mut Vec<Command>) {
        if let Some(erase) = self.charcoal_erase {
            commands.push(Command::Dab(Dab::charcoal(
                sample.point,
                self.radius(sample.pressure),
                sample.pressure,
                erase,
                self.tile,
            )));
            return;
        }
        if self.ink_hold.is_some() {
            commands.push(Command::Dab(Dab::ink_tiled(
                sample.point,
                sample.pressure,
                false,
                self.tile,
            )));
            return;
        }
        commands.push(Command::Dab(Dab::new_with_load(
            sample.point,
            self.radius(sample.pressure),
            sample.pressure,
            self.surface,
            self.brush,
            self.angle,
            self.tile,
            self.load_ratio,
        )));
    }
}
