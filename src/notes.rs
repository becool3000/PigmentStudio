//! Pinned notes and diagnostic inspection system for agent-human collaboration.
//! Notes are stored in `notes.json` in the workspace root.
//! When a note is pinned, it can automatically capture GPU layer diagnostics at that point.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

pub const DEFAULT_NOTES_FILE: &str = "notes.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Note {
    pub id: u32,
    pub x: f32, // Canvas x coordinate (0.0 .. 1024.0)
    pub y: f32, // Canvas y coordinate (0.0 .. 1024.0)
    pub text: String,
    pub author: String,
    pub created_at: String,
    #[serde(default)]
    pub done: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diagnostics: Option<GpuDiagnostics>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GpuDiagnostics {
    pub wet_channels: [f32; 8],
    pub wet_total: f32,
    pub dry_channels: [f32; 8],
    pub dry_total: f32,
    pub ink: [f32; 2],
    pub display_srgb: [u8; 3],
    pub optical: OpticalDiagnostics,
    pub brush_loads: [[f32; 8]; 4],
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OpticalDiagnostics {
    pub r_sub_mean: f32,
    pub r_int_mean: f32,
    #[serde(default)]
    pub wet_layer_hiding_ratio_pct: Option<f32>,
    pub intimate_blend_srgb: [u8; 3],
}

pub fn default_path() -> PathBuf {
    PathBuf::from(DEFAULT_NOTES_FILE)
}

pub fn read(path: &Path) -> Vec<Note> {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

pub fn write(path: &Path, notes: &[Note]) -> Result<(), std::io::Error> {
    let json = serde_json::to_string_pretty(notes).map_err(std::io::Error::other)?;
    fs::write(path, json)
}

pub fn add(
    path: &Path,
    x: f32,
    y: f32,
    text: &str,
    author: &str,
    diagnostics: Option<GpuDiagnostics>,
) -> Note {
    let mut notes = read(path);
    let next_id = notes.iter().map(|n| n.id).max().map_or(1, |m| m + 1);
    let now = {
        #[cfg(not(test))]
        {
            let dur = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default();
            format!("{}s", dur.as_secs())
        }
        #[cfg(test)]
        {
            "test_time".to_string()
        }
    };
    let note = Note {
        id: next_id,
        x,
        y,
        text: text.trim().to_string(),
        author: author.to_string(),
        created_at: now,
        done: false,
        diagnostics,
    };
    notes.push(note.clone());
    let _ = write(path, &notes);
    note
}

pub fn set_done(path: &Path, id: u32, done: bool) -> bool {
    let mut notes = read(path);
    if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
        note.done = done;
        let _ = write(path, &notes);
        true
    } else {
        false
    }
}

#[allow(dead_code)]
pub fn set_text(path: &Path, id: u32, text: &str) -> bool {
    let mut notes = read(path);
    if let Some(note) = notes.iter_mut().find(|n| n.id == id) {
        note.text = text.trim().to_string();
        let _ = write(path, &notes);
        true
    } else {
        false
    }
}

pub fn remove(path: &Path, id: u32) -> bool {
    let mut notes = read(path);
    let before_len = notes.len();
    notes.retain(|n| n.id != id);
    if notes.len() < before_len {
        let _ = write(path, &notes);
        true
    } else {
        false
    }
}

pub fn clear_done(path: &Path) -> usize {
    let mut notes = read(path);
    let before_len = notes.len();
    notes.retain(|n| !n.done);
    let removed = before_len - notes.len();
    if removed > 0 {
        let _ = write(path, &notes);
    }
    removed
}

pub fn compute_optical_diagnostics(
    wet: [f32; 8],
    dry: [f32; 8],
    substrate_bands: [f32; 31],
) -> OpticalDiagnostics {
    let wet_total: f32 = wet.iter().sum();

    let intimate_blend_srgb = {
        let mut combined = [0.0; 8];
        for i in 0..8 {
            combined[i] = wet[i] + dry[i];
        }
        crate::pigment::rgb(combined)
    };

    let spectrum = crate::pigment::spectrum();
    let mut r_sub_sum = 0.0;
    let mut r_int_sum = 0.0;
    let mut y_black = 0.0;
    let mut y_white = 0.0;
    let diffuse = |r: f32| 0.4 * r / (1.0 - 0.60 * r).max(0.00001);

    for (band_idx, band) in spectrum.iter().enumerate() {
        let k0 = band[0];
        let k1 = band[1];
        let s0 = band[2];
        let s1 = band[3];

        let r_sub = substrate_bands[band_idx];
        r_sub_sum += r_sub;

        let r_int = if wet_total <= 0.00001 {
            r_sub
        } else {
            let k_wet: f32 = (0..8).map(|i| wet[i] * [k0, k1][i / 4][i % 4]).sum();
            let s_wet: f32 = (0..8).map(|i| wet[i] * [s0, s1][i / 4][i % 4]).sum();
            let s_safe = s_wet.max(0.000001);
            let r = k_wet / s_safe;
            let a = 1.0 + r;
            let b = (r * (r + 2.0)).sqrt();
            let r_inf = 1.0 / (a + b);
            let u = (k_wet * (k_wet + 2.0 * s_wet)).sqrt();
            let e2 = (-2.0 * u).exp();

            // The same wet film over ideal black and white, weighted by CIE Y.
            let black = r_inf * (1.0 - e2) / (1.0 - e2 * r_inf * r_inf).max(0.00001);
            let white = (r_inf + e2) / (1.0 + e2 * r_inf);
            y_black += diffuse(black) * band[4][1];
            y_white += diffuse(white) * band[4][1];

            let denom = (1.0 - r_sub * r_inf).max(0.00001);
            let d_val = (r_inf - r_sub) / denom;
            let d_denom = (1.0 - e2 * r_inf * d_val).max(0.00001);
            ((r_inf - e2 * d_val) / d_denom).clamp(0.0, 1.0)
        };
        r_int_sum += r_int;
    }

    let r_sub_mean = r_sub_sum / 31.0;
    let r_int_mean = r_int_sum / 31.0;

    let wet_layer_hiding_ratio_pct =
        (y_white > 0.000001).then(|| (100.0 * y_black / y_white).clamp(0.0, 100.0));

    OpticalDiagnostics {
        r_sub_mean,
        r_int_mean,
        wet_layer_hiding_ratio_pct,
        intimate_blend_srgb,
    }
}
