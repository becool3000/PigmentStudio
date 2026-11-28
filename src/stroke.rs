use crate::gpu::{Brush, BrushContact, CanvasDims, Command, Dab, Surface};
use crate::splatter::{SplatterEngine, SplatterSettings};
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
    pub angle: Option<f32>,
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
    pending_first_dab: bool,
    continuous_knife: bool,
    round_pressure: bool,
    sweep: bool,
    legacy_sweep: bool,
    swept: bool,
    load_ratio: f32,
    tile: bool,
    dims: CanvasDims,
    charcoal_erase: Option<bool>,
    ink_hold: Option<(Instant, Pos2)>,
    splatter: Option<(SplatterEngine, SplatterSettings)>,
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

    #[cfg(test)]
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
            source, sample, diameter, surface, brush, angle, false, 1.0, tile, false, commands,
        )
    }

    #[cfg(test)]
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
        refined: bool,
        commands: &mut Vec<Command>,
    ) -> Self {
        Self::start_full_dims(
            source,
            sample,
            diameter,
            surface,
            brush,
            angle,
            auto_angle,
            load_ratio,
            tile,
            refined,
            CanvasDims::default(),
            commands,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_full_dims(
        source: Source,
        sample: Sample,
        diameter: f32,
        surface: Surface,
        brush: Brush,
        angle: f32,
        auto_angle: bool,
        load_ratio: f32,
        tile: bool,
        refined: bool,
        dims: CanvasDims,
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
            angle: sample
                .angle
                .filter(|_| brush.is_directional())
                .unwrap_or(angle),
            auto_angle,
            pending_first_dab: auto_angle
                && (brush == Brush::PaletteKnife
                    || (refined && matches!(brush, Brush::Flat | Brush::Filbert | Brush::Fan)))
                && sample.angle.is_none(),
            continuous_knife: brush == Brush::PaletteKnife
                && (auto_angle || sample.angle.is_some()),
            round_pressure: brush == Brush::Round && refined,
            sweep: refined
                && matches!(
                    brush,
                    Brush::Round
                        | Brush::PaletteKnife
                        | Brush::Flat
                        | Brush::Filbert
                        | Brush::Fan
                        | Brush::SoftMop
                ),
            swept: false,
            legacy_sweep: false,
            load_ratio,
            tile,
            dims,
            charcoal_erase: None,
            ink_hold: None,
            splatter: None,
        };
        if !stroke.pending_first_dab && !stroke.sweep {
            stroke.dab(sample, stroke.angle, commands);
        }
        stroke
    }

    #[cfg(test)]
    pub fn start_ink(source: Source, sample: Sample, commands: &mut Vec<Command>) -> Self {
        Self::start_ink_tiled(source, sample, false, commands)
    }

    #[cfg(test)]
    pub fn start_ink_tiled(
        source: Source,
        sample: Sample,
        tile: bool,
        commands: &mut Vec<Command>,
    ) -> Self {
        Self::start_ink_dims(source, sample, tile, CanvasDims::default(), commands)
    }

    pub fn start_ink_dims(
        source: Source,
        sample: Sample,
        tile: bool,
        dims: CanvasDims,
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
            pending_first_dab: false,
            continuous_knife: false,
            round_pressure: false,
            sweep: false,
            swept: false,
            legacy_sweep: false,
            load_ratio: 1.0,
            tile,
            dims,
            charcoal_erase: None,
            ink_hold: Some((Instant::now() + Duration::from_millis(34), sample.point)),
            splatter: None,
        };
        stroke.dab(sample, stroke.angle, commands);
        stroke
    }

    pub fn start_charcoal_dims(
        source: Source,
        sample: Sample,
        diameter: f32,
        erase: bool,
        tile: bool,
        dims: CanvasDims,
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
            pending_first_dab: false,
            continuous_knife: false,
            round_pressure: false,
            sweep: false,
            swept: false,
            legacy_sweep: false,
            load_ratio: 1.0,
            tile,
            dims,
            charcoal_erase: Some(erase),
            ink_hold: None,
            splatter: None,
        };
        stroke.dab(sample, stroke.angle, commands);
        stroke
    }

    pub fn start_splatter_dims(
        source: Source,
        sample: Sample,
        brush: Brush,
        tile: bool,
        settings: SplatterSettings,
        dims: CanvasDims,
        commands: &mut Vec<Command>,
    ) -> Self {
        let engine = SplatterEngine::start_dims(
            sample.point,
            sample.pressure,
            sample.timestamp_ms,
            brush,
            tile,
            settings,
            dims,
            commands,
        );
        Self {
            source,
            last: sample,
            pressure_at_dab: sample.pressure,
            distance_since_dab: 0.,
            diameter: settings.droplet_size,
            surface: Surface::Canvas,
            brush,
            angle: 0.,
            auto_angle: false,
            pending_first_dab: false,
            continuous_knife: false,
            round_pressure: false,
            sweep: false,
            swept: false,
            legacy_sweep: false,
            load_ratio: 1.0,
            tile,
            dims,
            charcoal_erase: None,
            ink_hold: None,
            splatter: Some((engine, settings)),
        }
    }

    pub fn ink_is_held(&self) -> bool {
        self.ink_hold.is_some() && self.last.pressure > 0.56
    }

    pub fn follows_path(&self) -> bool {
        self.auto_angle && self.brush.is_directional()
    }

    pub fn sweeps_contact(&self) -> bool {
        self.sweep
    }

    pub fn set_legacy_sweep(&mut self, legacy: bool) {
        self.legacy_sweep = legacy;
    }

    pub fn has_round_pressure(&self) -> bool {
        self.round_pressure
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
            if !self.pending_first_dab && !self.sweep {
                self.last = sample;
            }
            return;
        }
        if (sample.timestamp_ms.wrapping_sub(self.last.timestamp_ms) as i32) < 0 {
            return;
        }
        if let Some((engine, settings)) = &mut self.splatter {
            engine.advance(
                sample.point,
                sample.pressure,
                sample.timestamp_ms,
                self.brush,
                self.tile,
                *settings,
                commands,
            );
            self.last = sample;
            return;
        }
        let delta = sample.point - self.last.point;
        let distance = delta.length();
        let knife = self.brush == Brush::PaletteKnife;
        let explicit_angle = sample.angle.filter(|_| self.brush.is_directional());
        let mut previous_angle = self.angle;
        if let Some(angle) = explicit_angle {
            self.angle += angle_delta(self.angle, angle);
            self.continuous_knife |= knife;
        } else if self.follows_path()
            && distance
                >= if (knife || self.sweep) && !self.pending_first_dab {
                    0.25
                } else {
                    1.5
                }
        {
            let target = delta.y.atan2(delta.x) + std::f32::consts::FRAC_PI_2;
            let weight = if self.pending_first_dab {
                1.0
            } else {
                0.45 * (distance / 1.5).min(1.0)
            };
            self.angle += angle_delta(self.angle, target) * weight;
        }
        if self.pending_first_dab {
            if distance < 1.5 && explicit_angle.is_none() {
                if distance == 0.0 {
                    self.last = sample;
                }
                return;
            }
            self.pending_first_dab = false;
            if explicit_angle.is_none() {
                previous_angle = self.angle;
            }
            if !self.sweep {
                self.dab(self.last, previous_angle, commands);
                self.pressure_at_dab = self.last.pressure;
            }
        }
        if self.sweep {
            if distance > 0.0 {
                self.sweep_contact(sample, previous_angle, distance, commands);
                self.swept = true;
            }
            self.last = sample;
            return;
        }
        if distance == 0. {
            if sample.pressure - self.pressure_at_dab >= 0.04 {
                self.dab(sample, self.angle, commands);
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
                let angle = if knife || explicit_angle.is_some() {
                    egui::lerp(previous_angle..=self.angle, fraction)
                } else {
                    self.angle
                };
                self.dab(
                    Sample {
                        point: self.last.point.lerp(sample.point, fraction),
                        pressure,
                        timestamp_ms: sample.timestamp_ms,
                        angle: None,
                    },
                    angle,
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
        let needs_final_dab = if self.sweep {
            !self.swept
        } else {
            self.pending_first_dab || self.distance_since_dab > 0.01
        };
        if needs_final_dab {
            self.dab(self.last, self.angle, commands);
        }
    }

    fn sweep_contact(
        &self,
        sample: Sample,
        previous_angle: f32,
        distance: f32,
        commands: &mut Vec<Command>,
    ) {
        // Keep quadrature accurate when a long input interval crosses much of
        // the tip or turns sharply. Ordinary pen samples typically need one segment.
        let radius = self.radius(sample.pressure.min(self.last.pressure));
        let radial = matches!(self.brush, Brush::Round | Brush::SoftMop);
        let (max_distance, turn) = if self.legacy_sweep {
            let max_d = (radius * 0.25).max(0.75);
            let t = if self.brush == Brush::Round {
                0.0
            } else {
                (self.angle - previous_angle).abs() / 0.16
            };
            (max_d, t)
        } else {
            let max_d = if radial {
                (radius * 1.0).max(2.0)
            } else {
                (radius * 0.50).max(1.25)
            };
            let t = if radial {
                0.0
            } else {
                (self.angle - previous_angle).abs() / 0.20
            };
            (max_d, t)
        };
        let steps = (distance / max_distance).max(turn).ceil() as u32;
        let contact = |t| {
            let pressure = egui::lerp(self.last.pressure..=sample.pressure, t);
            BrushContact {
                point: self.last.point.lerp(sample.point, t),
                radius: self.radius(pressure),
                pressure,
                angle: egui::lerp(previous_angle..=self.angle, t),
            }
        };
        let mut start = contact(0.0);
        for i in 1..=steps {
            let end = contact(i as f32 / steps as f32);
            commands.push(Command::Dab(Dab::swept_contact_dims(
                start,
                end,
                self.brush,
                self.surface,
                self.tile,
                self.dims,
            )));
            start = end;
        }
    }

    fn radius(&self, pressure: f32) -> f32 {
        if self.ink_hold.is_some() {
            return Dab::ink_radius(pressure);
        }
        let spread = if self.round_pressure {
            0.12 + 0.88 * pressure.powf(0.75)
        } else if self.sweep && self.brush == Brush::Filbert {
            0.18 + 0.82 * pressure.powf(0.65)
        } else if self.sweep && self.brush == Brush::Fan {
            0.36 + 0.64 * pressure.sqrt()
        } else {
            0.22 + 0.78 * pressure.sqrt()
        };
        (0.5 * self.diameter * spread).max(0.75)
    }

    fn dab(&self, sample: Sample, angle: f32, commands: &mut Vec<Command>) {
        if let Some(erase) = self.charcoal_erase {
            commands.push(Command::Dab(Dab::charcoal_dims(
                sample.point,
                self.radius(sample.pressure),
                sample.pressure,
                erase,
                self.tile,
                self.dims,
            )));
            return;
        }
        if self.ink_hold.is_some() {
            commands.push(Command::Dab(Dab::ink_dims(
                sample.point,
                sample.pressure,
                false,
                self.tile,
                self.dims,
            )));
            return;
        }
        let mut dab = Dab::new_with_load_dims(
            sample.point,
            self.radius(sample.pressure),
            sample.pressure,
            self.surface,
            self.brush,
            angle,
            self.tile,
            self.load_ratio,
            self.dims,
        );
        if self.round_pressure {
            let pressure = ((sample.pressure - 0.015) / 0.735).clamp(0.0, 1.0);
            let contact = pressure * pressure * (3.0 - 2.0 * pressure);
            dab.set_refined_tip(0.08 + 0.92 * contact);
        } else if self.continuous_knife
            || (self.sweep && matches!(self.brush, Brush::Filbert | Brush::Fan))
        {
            dab.set_refined_tip(1.0);
        }
        commands.push(Command::Dab(dab));
    }
}

fn angle_delta(from: f32, to: f32) -> f32 {
    let diff = (to - from).rem_euclid(std::f32::consts::TAU);
    if diff > std::f32::consts::PI {
        diff - std::f32::consts::TAU
    } else {
        diff
    }
}

#[cfg(test)]
mod tests {
    use super::angle_delta;

    #[test]
    fn blade_turn_uses_the_short_arc_across_zero() {
        let a = 350_f32.to_radians();
        let b = 10_f32.to_radians();
        assert!((angle_delta(a, b).to_degrees() - 20.0).abs() < 0.001);
        assert!((angle_delta(b, a).to_degrees() + 20.0).abs() < 0.001);
    }
}
