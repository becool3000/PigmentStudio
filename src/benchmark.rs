use std::time::{Duration, Instant};

const RATE: usize = 120;
const SAMPLES: usize = 10 * RATE;

pub struct FrameTiming {
    pub total: Duration,
    pub acquire: Duration,
    pub encode: Duration,
    pub present: Duration,
    pub gpu_and_submit: Duration,
}

#[derive(Default)]
pub struct Benchmark {
    start: Option<Instant>,
    sample: usize,
    frames: Vec<FrameTiming>,
}

impl Benchmark {
    pub fn next_deadline(&self) -> Instant {
        self.start.unwrap_or_else(Instant::now)
            + Duration::from_secs_f64(self.sample as f64 / RATE as f64)
    }

    pub fn ready_samples(
        &mut self,
        now: Instant,
    ) -> impl Iterator<Item = (usize, egui::Pos2, f32, u32)> {
        let elapsed = now
            .duration_since(*self.start.get_or_insert(now))
            .as_secs_f64();
        let ready = ((elapsed * RATE as f64) as usize + 1).min(SAMPLES);
        let start = self.sample;
        self.sample = ready;
        (start..ready).map(|i| {
            let t = i as f32 / RATE as f32;
            let x = 0.5 + 0.38 * (2.1 * t).sin();
            let y = 0.5 + 0.36 * (2.9 * t).cos();
            let pressure = 0.70 + 0.25 * (3.7 * t).sin();
            (i, egui::pos2(x, y), pressure, (1000 * i / RATE) as u32)
        })
    }

    pub fn finished(&self, now: Instant) -> bool {
        self.sample == SAMPLES
            && self
                .start
                .is_some_and(|start| now.duration_since(start) >= Duration::from_secs(10))
    }

    pub fn frame(&mut self, timing: FrameTiming) {
        self.frames.push(timing);
    }

    pub fn report(&self) -> Result<(), String> {
        if self.frames.is_empty() {
            return Err("The benchmark presented no paint frames.".into());
        }
        let percentile = |field: fn(&FrameTiming) -> Duration| {
            let mut times: Vec<_> = self.frames.iter().map(field).collect();
            times.sort_unstable();
            times[(times.len() * 95).div_ceil(100) - 1].as_secs_f64() * 1000.
        };
        let p95 = percentile(|frame| frame.total);
        println!(
            "Presented {} paint frames from {} samples: p95 {:.2} ms total.",
            self.frames.len(),
            self.sample,
            p95
        );
        println!(
            "Stage p95 (ms): acquire {:.2}, encode {:.2}, GPU and submit {:.2}, present {:.2}.",
            percentile(|frame| frame.acquire),
            percentile(|frame| frame.encode),
            percentile(|frame| frame.gpu_and_submit),
            percentile(|frame| frame.present)
        );
        if p95 > 16.7 {
            return Err(format!(
                "Benchmark exceeded the 16.7 ms target (p95 {p95:.2} ms)."
            ));
        }
        Ok(())
    }
}
