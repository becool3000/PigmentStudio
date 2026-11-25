//! Tactile acoustic feedback for studio actions (jar seating and brass toggles).
//! Procedurally generates micro-WAV PCM buffers and dispatches them asynchronously via Win32.

use std::sync::OnceLock;

const SAMPLE_RATE: u32 = 44100;

fn create_wav(samples: &[i16]) -> Vec<u8> {
    let mut wav = Vec::with_capacity(44 + samples.len() * 2);
    // RIFF header
    wav.extend_from_slice(b"RIFF");
    let file_size = (36 + samples.len() * 2) as u32;
    wav.extend_from_slice(&file_size.to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    // fmt subchunk
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // subchunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
                                                 // data subchunk
    wav.extend_from_slice(b"data");
    let data_size = (samples.len() * 2) as u32;
    wav.extend_from_slice(&data_size.to_le_bytes());
    for s in samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    wav
}

/// Generates a wooden jar seating clack (approx. 50ms) with a subtle glass settling resonance.
fn generate_jar_clack() -> Vec<u8> {
    let count = (SAMPLE_RATE as f32 * 0.05) as usize;
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let t = i as f32 / SAMPLE_RATE as f32;
        // Muted wooden body thud (fundamental 240 Hz + harmonic 480 Hz) with rapid decay
        let wood = (2.0 * std::f32::consts::PI * 240.0 * t).sin() * (-t * 110.0).exp()
            + (2.0 * std::f32::consts::PI * 480.0 * t).sin() * (-t * 160.0).exp() * 0.5;
        // Subtle glass settling chime (2800 Hz) that decays within 15ms
        let glass = (2.0 * std::f32::consts::PI * 2800.0 * t).sin() * (-t * 280.0).exp() * 0.22;
        let sample = (wood + glass) * 0.85;
        samples.push((sample.clamp(-1.0, 1.0) * 18000.0) as i16);
    }
    create_wav(&samples)
}

/// Generates a crisp metallic brass latch click (approx. 25ms).
fn generate_brass_click() -> Vec<u8> {
    let count = (SAMPLE_RATE as f32 * 0.025) as usize;
    let mut samples = Vec::with_capacity(count);
    for i in 0..count {
        let t = i as f32 / SAMPLE_RATE as f32;
        // Crisp dual metallic impulse
        let primary = (2.0 * std::f32::consts::PI * 1850.0 * t).sin() * (-t * 220.0).exp();
        let secondary = if t > 0.007 {
            let dt = t - 0.007;
            (2.0 * std::f32::consts::PI * 3400.0 * dt).sin() * (-dt * 300.0).exp() * 0.6
        } else {
            0.0
        };
        let sample = (primary + secondary) * 0.7;
        samples.push((sample.clamp(-1.0, 1.0) * 16000.0) as i16);
    }
    create_wav(&samples)
}

static JAR_CLACK_WAV: OnceLock<Vec<u8>> = OnceLock::new();
static BRASS_CLICK_WAV: OnceLock<Vec<u8>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
pub enum Sound {
    JarClack,
    BrassClick,
}

pub fn play(sound: Sound) {
    let bytes: &'static [u8] = match sound {
        Sound::JarClack => JAR_CLACK_WAV.get_or_init(generate_jar_clack),
        Sound::BrassClick => BRASS_CLICK_WAV.get_or_init(generate_brass_click),
    };
    platform::play_wav_bytes(bytes);
}

#[cfg(target_os = "windows")]
mod platform {
    const SND_ASYNC: u32 = 0x0001;
    const SND_NODEFAULT: u32 = 0x0002;
    const SND_MEMORY: u32 = 0x0004;

    #[link(name = "winmm")]
    extern "system" {
        fn PlaySoundW(pszsound: *const u8, hmod: usize, fdwsound: u32) -> i32;
    }

    pub fn play_wav_bytes(bytes: &'static [u8]) {
        unsafe {
            PlaySoundW(bytes.as_ptr(), 0, SND_ASYNC | SND_MEMORY | SND_NODEFAULT);
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    pub fn play_wav_bytes(_bytes: &'static [u8]) {}
}
