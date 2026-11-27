//! Velocity- and whip-driven Jackson Pollock paint splatter & sling physics.

use egui::{vec2, Pos2, Vec2};

use crate::gpu::{Brush, CanvasDims, Command, Dab};

/// Adjustable physical parameters for the Jackson Pollock Splatter / Sling tool.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SplatterSettings {
    /// 0.05..=1.0: Low = fine mist & wide ballistic scatter; High = thick poured enamel strings.
    pub viscosity: f32,
    /// 0.0..=2.0: Velocity-to-throw distance and whip sensitivity.
    pub sling_throw: f32,
    /// 4.0..=48.0: Base diameter (px) of primary puddles & droplets.
    pub droplet_size: f32,
    /// 0.2..=2.0: Multiplier on droplet & satellite emission count.
    pub spray_density: f32,
}

impl Default for SplatterSettings {
    fn default() -> Self {
        Self {
            viscosity: 0.35,
            sling_throw: 1.0,
            droplet_size: 18.0,
            spray_density: 1.0,
        }
    }
}

/// Per-stroke ballistic state for poured strings, beaded drips, and whip-flung splatters.
pub struct SplatterEngine {
    prev_pos: Pos2,
    prev_vel: Vec2,
    prev_t_ms: u32,
    rng: u32,
    carry_dist: f32,
    dims: CanvasDims,
}

impl SplatterEngine {
    #[cfg(test)]
    pub fn start(
        pos: Pos2,
        pressure: f32,
        t_ms: u32,
        brush: Brush,
        tiled: bool,
        settings: SplatterSettings,
        commands: &mut Vec<Command>,
    ) -> Self {
        Self::start_dims(
            pos,
            pressure,
            t_ms,
            brush,
            tiled,
            settings,
            CanvasDims::default(),
            commands,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn start_dims(
        pos: Pos2,
        pressure: f32,
        t_ms: u32,
        brush: Brush,
        tiled: bool,
        settings: SplatterSettings,
        dims: CanvasDims,
        commands: &mut Vec<Command>,
    ) -> Self {
        let seed = ((pos.x.to_bits() ^ pos.y.to_bits().rotate_left(13)) ^ t_ms) | 1;
        let mut engine = Self {
            prev_pos: pos,
            prev_vel: Vec2::ZERO,
            prev_t_ms: t_ms,
            rng: seed,
            carry_dist: 0.0,
            dims,
        };
        // Initial touchdown puddle / bead
        let r = (settings.droplet_size * 0.5) * (0.45 + 0.65 * pressure.clamp(0.05, 1.0));
        commands.push(Command::Dab(Dab::splat_dims(
            clamp_canvas(pos, tiled, dims),
            r.max(1.5),
            pressure.clamp(0.25, 1.0),
            brush,
            0.0,
            1.0,
            tiled,
            dims,
        )));
        engine.emit_mist_burst(
            pos,
            Vec2::ZERO,
            0.3,
            pressure,
            brush,
            tiled,
            settings,
            commands,
        );
        engine
    }

    #[allow(clippy::too_many_arguments)]
    pub fn advance(
        &mut self,
        pos: Pos2,
        pressure: f32,
        t_ms: u32,
        brush: Brush,
        tiled: bool,
        settings: SplatterSettings,
        commands: &mut Vec<Command>,
    ) {
        let dt = (t_ms.saturating_sub(self.prev_t_ms) as f32 / 1000.0).clamp(0.004, 0.12);
        let delta = pos - self.prev_pos;
        let dist = delta.length();
        if dist < 0.25 {
            return;
        }
        let vel = delta / dt;
        let speed = vel.length();
        let accel = (vel - self.prev_vel).length() / dt;
        let dir = delta / dist;
        let angle = dir.y.atan2(dir.x);

        let visc = settings.viscosity.clamp(0.05, 1.0);
        let whip = ((speed - 120.0) / 650.0 + accel / 7000.0).clamp(0.0, 2.4)
            * settings.sling_throw.clamp(0.0, 2.0);

        // 1. Poured / beaded string along the gesture path.
        // Slow or viscous gestures lay continuous strings; fast whips break into beads.
        let base_r = (settings.droplet_size * 0.5).max(2.0);
        let step = (base_r * (0.55 + (1.0 - visc) * 0.65 + whip * 0.45)).max(2.5);
        self.carry_dist += dist;
        let string_keep = (0.35 + visc * 0.65 - whip * 0.25).clamp(0.18, 1.0);
        let mut walked = 0.0;
        while self.carry_dist >= step {
            walked += step;
            self.carry_dist -= step;
            let t = (walked / dist).min(1.0);
            let p = self.prev_pos + delta * t;
            if self.rand01() <= string_keep {
                let jitter = (self.rand_signed() * 1.8) * (1.1 - visc * 0.6);
                let perp = vec2(-dir.y, dir.x);
                let bead_pos = clamp_canvas(p + perp * jitter, tiled, self.dims);
                let r = (base_r * (0.38 + 0.52 * pressure) * (0.75 + 0.5 * self.rand01()))
                    * (1.0 - whip * 0.18).max(0.45);
                let elong = (1.0 + whip * 0.35).min(2.2);
                commands.push(Command::Dab(Dab::splat_dims(
                    bead_pos,
                    r.max(1.5),
                    pressure.clamp(0.25, 1.0),
                    brush,
                    angle,
                    elong,
                    tiled,
                    self.dims,
                )));
            }
        }

        // 2. Ballistic sling & satellite mist droplets thrown ahead along velocity vector.
        if whip > 0.05 || dist > 3.0 {
            self.emit_mist_burst(pos, vel, whip, pressure, brush, tiled, settings, commands);
        }

        self.prev_pos = pos;
        self.prev_vel = vel;
        self.prev_t_ms = t_ms;
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_mist_burst(
        &mut self,
        origin: Pos2,
        vel: Vec2,
        whip: f32,
        pressure: f32,
        brush: Brush,
        tiled: bool,
        settings: SplatterSettings,
        commands: &mut Vec<Command>,
    ) {
        let speed = vel.length();
        let visc = settings.viscosity.clamp(0.05, 1.0);
        let density = settings.spray_density.clamp(0.1, 2.5);
        let count_f = density * (0.8 + whip * 2.8);
        let count =
            (count_f.floor() as usize + usize::from(self.rand01() < count_f.fract())).min(18);
        let vel_angle = if speed > 1.0 {
            vel.y.atan2(vel.x)
        } else {
            self.rand01() * std::f32::consts::TAU
        };
        let base_r = (settings.droplet_size * 0.5).max(2.0);
        let max_throw =
            (speed * 0.14 * settings.sling_throw * (1.25 - 0.55 * visc)).clamp(6.0, 220.0);
        let cone = (0.20 + 0.65 * (1.0 - visc)) * (0.45 + 0.55 * whip.min(1.5));

        for _ in 0..count {
            let u1 = self.rand01();
            let throw_dist = (0.12 + 0.88 * u1 * u1) * max_throw;
            let spread = (self.rand_signed() + self.rand_signed()) * 0.5 * cone;
            let drop_angle = vel_angle + spread;
            let dir = vec2(drop_angle.cos(), drop_angle.sin());
            let impact = origin + dir * throw_dist;
            if !tiled
                && (impact.x < 0.0
                    || impact.y < 0.0
                    || impact.x >= self.dims.width as f32
                    || impact.y >= self.dims.height as f32)
            {
                continue;
            }
            let roll = self.rand01();
            let (r, elong) = if roll < 0.20 {
                // Primary flung blob
                (
                    base_r * (0.34 + 0.36 * self.rand01()) * (0.65 + 0.45 * pressure),
                    1.0 + 0.85 * whip.min(2.0),
                )
            } else if roll < 0.65 {
                // Medium satellite droplet / streak
                (
                    base_r * (0.14 + 0.20 * self.rand01()),
                    1.0 + 1.35 * whip.min(2.0),
                )
            } else {
                // Fine mist speckle
                ((1.4 + 2.4 * self.rand01()).max(base_r * 0.09), 1.0)
            };
            commands.push(Command::Dab(Dab::splat_dims(
                clamp_canvas(impact, tiled, self.dims),
                r,
                (0.45 + 0.55 * pressure).clamp(0.3, 1.0),
                brush,
                drop_angle,
                elong,
                tiled,
                self.dims,
            )));
        }
    }

    fn rand01(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / ((1u32 << 24) as f32)
    }

    fn rand_signed(&mut self) -> f32 {
        self.rand01() * 2.0 - 1.0
    }
}

fn clamp_canvas(p: Pos2, tiled: bool, dims: CanvasDims) -> Pos2 {
    if tiled {
        p
    } else {
        egui::pos2(
            p.x.clamp(0.0, (dims.width - 1) as f32),
            p.y.clamp(0.0, (dims.height - 1) as f32),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splatter_whip_flings_farther_and_more_droplets_than_slow_pour() {
        let settings = SplatterSettings::default();
        let mut slow_cmds = Vec::new();
        let mut slow = SplatterEngine::start(
            egui::pos2(300.0, 300.0),
            0.8,
            0,
            Brush::Round,
            false,
            settings,
            &mut slow_cmds,
        );
        slow_cmds.clear();
        slow.advance(
            egui::pos2(305.0, 300.0),
            0.8,
            100,
            Brush::Round,
            false,
            settings,
            &mut slow_cmds,
        );

        let mut fast_cmds = Vec::new();
        let mut fast = SplatterEngine::start(
            egui::pos2(300.0, 300.0),
            0.8,
            0,
            Brush::Round,
            false,
            settings,
            &mut fast_cmds,
        );
        fast_cmds.clear();
        fast.advance(
            egui::pos2(420.0, 300.0),
            0.8,
            16,
            Brush::Round,
            false,
            settings,
            &mut fast_cmds,
        );

        assert!(
            fast_cmds.len() > slow_cmds.len(),
            "fast whip ({}) should emit more droplets than slow pour ({})",
            fast_cmds.len(),
            slow_cmds.len()
        );
    }
}
