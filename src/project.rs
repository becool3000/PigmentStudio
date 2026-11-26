use crate::gpu::{
    ActiveLayer, Brush, Layers, BRUSH_BYTES, BRUSH_COUNT, BRUSH_LOAD_BYTES, LEGACY_BRUSH_BYTES,
    PROJECT_BYTES,
};
use crate::recording::{BrushKind, LightingConfig, Recording};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::path::Path;

const MAGIC: &[u8; 8] = b"PIGMENT2";
const LEGACY_MAGIC: &[u8; 8] = b"PIGMENT1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActiveLayerKind {
    Paint,
    Sketch,
}

impl From<ActiveLayer> for ActiveLayerKind {
    fn from(a: ActiveLayer) -> Self {
        match a {
            ActiveLayer::Paint => Self::Paint,
            ActiveLayer::Sketch => Self::Sketch,
        }
    }
}

impl From<ActiveLayerKind> for ActiveLayer {
    fn from(a: ActiveLayerKind) -> Self {
        match a {
            ActiveLayerKind::Paint => Self::Paint,
            ActiveLayerKind::Sketch => Self::Sketch,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LayersMeta {
    pub active: ActiveLayerKind,
    pub paint_visible: bool,
    pub sketch_visible: bool,
    pub sketch_opacity: f32,
    pub reference_visible: bool,
    pub reference_opacity: f32,
    pub has_reference: bool,
}

impl From<Layers> for LayersMeta {
    fn from(l: Layers) -> Self {
        Self {
            active: l.active.into(),
            paint_visible: l.paint_visible,
            sketch_visible: l.sketch_visible,
            sketch_opacity: l.sketch_opacity,
            reference_visible: l.reference_visible,
            reference_opacity: l.reference_opacity,
            has_reference: l.has_reference,
        }
    }
}

impl From<LayersMeta> for Layers {
    fn from(l: LayersMeta) -> Self {
        Self {
            active: l.active.into(),
            paint_visible: l.paint_visible,
            sketch_visible: l.sketch_visible,
            sketch_opacity: l.sketch_opacity,
            reference_visible: l.reference_visible,
            reference_opacity: l.reference_opacity,
            has_reference: l.has_reference,
        }
    }
}

pub const CURRENT_CALIBRATION_VERSION: u32 = 9;

fn default_calibration_version() -> u32 {
    1
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectMeta {
    #[serde(default = "default_calibration_version")]
    pub calibration_version: u32,
    #[serde(deserialize_with = "deserialize_loads")]
    pub loads: [f32; BRUSH_COUNT],
    pub jars: Vec<bool>,
    pub sizes: [f32; 2],
    pub angle: f32,
    pub brush: BrushKind,
    pub ink_selected: bool,
    pub charcoal_erase: bool,
    pub charcoal_size: f32,
    pub lighting: LightingConfig,
    pub layers: LayersMeta,
    pub tile: bool,
    pub recording: Recording,
}

impl ProjectMeta {
    pub fn brush(&self) -> Brush {
        self.brush.into()
    }
}

fn deserialize_loads<'de, D>(deserializer: D) -> Result<[f32; BRUSH_COUNT], D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum LoadMeters {
        Independent([f32; BRUSH_COUNT]),
        Shared([f32; 2]),
    }

    Ok(match LoadMeters::deserialize(deserializer)? {
        LoadMeters::Independent(loads) => loads,
        LoadMeters::Shared([round, flat]) => {
            let mut loads = [0.0; BRUSH_COUNT];
            loads[Brush::Round.index()] = round;
            loads[Brush::Flat.index()] = flat;
            loads
        }
    })
}

fn reservoir_loads(gpu_bytes: &[u8]) -> [f32; BRUSH_COUNT] {
    let start = (PROJECT_BYTES - BRUSH_BYTES) as usize;
    Brush::ALL.map(|brush| {
        let lanes = brush.load_lanes();
        let brush_bytes =
            &gpu_bytes[start + lanes.start as usize * 32..start + lanes.end as usize * 32];
        let mass = brush_bytes
            .chunks_exact(4)
            .map(|bytes| f32::from_le_bytes(bytes.try_into().unwrap()))
            .sum::<f32>();
        (mass / 184_000.0).clamp(0.0, 1.0)
    })
}

/// PackBits-style RLE over 32-bit words:
/// `header > 0`: repeat next `u32` `header` times.
/// `header < 0`: copy next `-header` literal `u32` words.
pub(crate) fn rle_encode_u32(bytes: &[u8]) -> Vec<u8> {
    assert_eq!(bytes.len() % 4, 0);
    let words: Vec<u32> = bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes(c.try_into().unwrap()))
        .collect();
    let mut out = Vec::with_capacity(words.len() / 8);
    let mut i = 0;
    let max_run = i32::MAX as usize;

    while i < words.len() {
        let mut run = 1;
        while i + run < words.len() && words[i + run] == words[i] && run < max_run {
            run += 1;
        }
        if run >= 3 {
            out.extend_from_slice(&(run as i32).to_le_bytes());
            out.extend_from_slice(&words[i].to_le_bytes());
            i += run;
        } else {
            let start = i;
            i += run;
            while i < words.len() && (i - start) < max_run {
                if i + 2 < words.len() && words[i] == words[i + 1] && words[i] == words[i + 2] {
                    break;
                }
                i += 1;
            }
            let lit_len = i - start;
            out.extend_from_slice(&(-(lit_len as i32)).to_le_bytes());
            out.extend_from_slice(bytemuck::cast_slice(&words[start..i]));
        }
    }
    out
}

pub(crate) fn rle_decode_u32(encoded: &[u8], expected_bytes: usize) -> Result<Vec<u8>, String> {
    if !expected_bytes.is_multiple_of(4) {
        return Err("Invalid expected project size.".into());
    }
    let expected_words = expected_bytes / 4;
    let mut words = Vec::with_capacity(expected_words);
    let mut pos = 0;
    while pos < encoded.len() {
        if pos + 4 > encoded.len() {
            return Err("Truncated RLE header in .pigment file.".into());
        }
        let header = i32::from_le_bytes(encoded[pos..pos + 4].try_into().unwrap());
        pos += 4;
        if header > 0 {
            let count = header as usize;
            if pos + 4 > encoded.len() {
                return Err("Truncated RLE run word in .pigment file.".into());
            }
            let word = u32::from_le_bytes(encoded[pos..pos + 4].try_into().unwrap());
            pos += 4;
            if words.len() + count > expected_words {
                return Err("Corrupt RLE run exceeds project buffer size.".into());
            }
            words.resize(words.len() + count, word);
        } else if header < 0 {
            let count = header.unsigned_abs() as usize;
            let byte_len = count * 4;
            if pos + byte_len > encoded.len() || words.len() + count > expected_words {
                return Err("Corrupt RLE literal block in .pigment file.".into());
            }
            for chunk in encoded[pos..pos + byte_len].chunks_exact(4) {
                words.push(u32::from_le_bytes(chunk.try_into().unwrap()));
            }
            pos += byte_len;
        } else {
            return Err("Invalid zero RLE header in .pigment file.".into());
        }
    }
    if words.len() != expected_words {
        return Err(format!(
            "Project buffer size mismatch: got {} words, expected {}.",
            words.len(),
            expected_words
        ));
    }
    Ok(bytemuck::cast_slice(&words).to_vec())
}

pub fn write_project(path: &Path, mut meta: ProjectMeta, gpu_bytes: &[u8]) -> Result<(), String> {
    if gpu_bytes.len() as u64 != PROJECT_BYTES {
        return Err("Unexpected GPU buffer size for project save.".into());
    }
    meta.loads = reservoir_loads(gpu_bytes);
    let meta_json = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    let meta_len = u32::try_from(meta_json.len()).map_err(|_| "Metadata too large.".to_string())?;
    let compressed = rle_encode_u32(gpu_bytes);

    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file = tempfile::NamedTempFile::new_in(directory).map_err(|e| e.to_string())?;
    {
        let w = file.as_file_mut();
        w.write_all(MAGIC).map_err(|e| e.to_string())?;
        w.write_all(&meta_len.to_le_bytes())
            .map_err(|e| e.to_string())?;
        w.write_all(&meta_json).map_err(|e| e.to_string())?;
        w.write_all(&compressed).map_err(|e| e.to_string())?;
        w.flush().map_err(|e| e.to_string())?;
    }
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn read_project(path: &Path) -> Result<(ProjectMeta, std::sync::Arc<Vec<u8>>), String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|mut f| f.read_to_end(&mut bytes))
        .map_err(|e| e.to_string())?;
    read_project_bytes(&bytes)
}

pub fn read_project_bytes(bytes: &[u8]) -> Result<(ProjectMeta, std::sync::Arc<Vec<u8>>), String> {
    if bytes.len() < 12 || (&bytes[..8] != MAGIC && &bytes[..8] != LEGACY_MAGIC) {
        return Err("Not a valid Pigment Studio (.pigment) file.".into());
    }
    let meta_len = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    if 12 + meta_len > bytes.len() {
        return Err("Corrupt .pigment metadata header.".into());
    }
    let mut meta: ProjectMeta =
        serde_json::from_slice(&bytes[12..12 + meta_len]).map_err(|e| e.to_string())?;
    meta.recording.upgrade_legacy_reservoirs();
    let payload = &bytes[12 + meta_len..];
    let gpu_bytes = if &bytes[..8] == LEGACY_MAGIC {
        let brush_offset = (PROJECT_BYTES - BRUSH_BYTES) as usize;
        let mut gpu_bytes = rle_decode_u32(payload, brush_offset + LEGACY_BRUSH_BYTES)?;
        // Round and Flat keep the first four legacy lanes. Insert the remaining
        // reservoirs before the shared transfer scratch and placement state.
        let legacy_load_end = brush_offset + 4 * 32;
        let tail = gpu_bytes.split_off(legacy_load_end);
        gpu_bytes.reserve_exact(BRUSH_LOAD_BYTES - 4 * 32 + tail.len());
        gpu_bytes.resize(brush_offset + BRUSH_LOAD_BYTES, 0);
        gpu_bytes.extend(tail);
        gpu_bytes
    } else {
        rle_decode_u32(payload, PROJECT_BYTES as usize)?
    };
    debug_assert_eq!(gpu_bytes.len(), PROJECT_BYTES as usize);
    meta.loads = reservoir_loads(&gpu_bytes);
    Ok((meta, std::sync::Arc::new(gpu_bytes)))
}
