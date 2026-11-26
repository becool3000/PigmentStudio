pub const NAMES: [&str; 6] = [
    "Hansa yellow",
    "Phthalo blue",
    "Quinacridone magenta",
    "Titanium white",
    "Ultramarine blue",
    "Burnt sienna",
];

// CIE 1931 2° color matching functions and D65 SPD, 400–700 nm in 10 nm steps.
const OBSERVER: [[f32; 4]; 31] = [
    [0.01431, 0.000396, 0.06785, 82.7549],
    [0.04351, 0.00121, 0.2074, 91.486],
    [0.13438, 0.004, 0.6456, 93.4318],
    [0.2839, 0.0116, 1.3856, 86.6823],
    [0.34828, 0.023, 1.74706, 104.865],
    [0.3362, 0.038, 1.77211, 117.008],
    [0.2908, 0.06, 1.6692, 117.812],
    [0.19536, 0.09098, 1.28764, 114.861],
    [0.09564, 0.13902, 0.81295, 115.923],
    [0.03201, 0.20802, 0.46518, 108.811],
    [0.0049, 0.323, 0.272, 109.354],
    [0.0093, 0.503, 0.1582, 107.802],
    [0.06327, 0.71, 0.07825, 104.79],
    [0.1655, 0.862, 0.04216, 107.689],
    [0.2904, 0.954, 0.0203, 104.405],
    [0.43345, 0.99495, 0.00875, 104.046],
    [0.5945, 0.995, 0.0039, 100.0],
    [0.7621, 0.952, 0.0021, 96.3342],
    [0.9163, 0.87, 0.00165, 95.788],
    [1.0263, 0.757, 0.0011, 88.6856],
    [1.0622, 0.631, 0.0008, 90.0062],
    [1.0026, 0.503, 0.00034, 89.5991],
    [0.85445, 0.381, 0.00019, 87.6987],
    [0.6424, 0.265, 0.00005, 83.2886],
    [0.4479, 0.175, 0.00002, 83.6992],
    [0.2835, 0.107, 0.0, 80.0268],
    [0.1649, 0.061, 0.0, 80.2146],
    [0.0874, 0.032, 0.0, 82.2778],
    [0.04677, 0.017, 0.0, 78.2842],
    [0.0227, 0.00821, 0.0, 69.7213],
    [0.011359, 0.004102, 0.0, 71.6091],
];

// Hand-authored K and S anchors at 50 nm intervals, interpolated to the observer grid.
const ABSORPTION: [[f32; 7]; 8] = [
    [8.7419, 6.1915, 0.452, 0.005, 0.002, 0.002, 0.003], // 0: Hansa yellow (PY3 calibrated)
    [1.802, 0.648, 1.053, 4.175, 22.372, 39.472, 40.064], // 1: Phthalo blue (PB15:2 calibrated)
    [2.188, 1.097, 10.781, 31.101, 0.96, 0.22, 0.152], // 2: Quinacridone magenta (PR122 calibrated)
    [0.0327, 0.005, 0.002, 0.001, 0.001, 0.001, 0.0021], // 3: Titanium white (PW6 calibrated)
    [0.051, 0.081, 0.611, 2.832, 4.587, 1.51, 0.556],  // 4: Ultramarine blue (PB29 calibrated)
    [4.872, 5.67, 3.055, 1.6, 0.445, 0.187, 0.126],    // 5: Burnt sienna (PBr7 calibrated)
    [13.589, 31.068, 18.698, 14.0, 0.041, 0.006, 0.003], // 6: Cadmium Red Medium (PR108 calibrated)
    [7.56, 7.5, 8.0, 8.5, 9.0, 9.785, 10.485],         // 7: Bone Black (PBk9 calibrated)
];
const SCATTERING: [[f32; 7]; 8] = [
    [0.466, 0.427, 0.64, 0.833, 0.639, 0.765, 0.71], // 0: Hansa yellow (PY3 calibrated)
    [0.122, 0.106, 0.093, 0.076, 0.055, 0.143, 0.16], // 1: Phthalo blue (PB15:2 calibrated)
    [0.159, 0.145, 0.115, 0.133, 0.135, 0.152, 0.203], // 2: Quinacridone magenta (PR122 calibrated)
    [19.667, 24.758, 29.26, 28.0, 28.0, 24.074, 21.326], // 3: Titanium white (PW6 calibrated)
    [0.962, 1.037, 0.943, 0.848, 0.725, 0.823, 0.954], // 4: Ultramarine blue (PB29 calibrated)
    [0.433, 0.758, 0.653, 0.796, 0.682, 0.741, 1.066], // 5: Burnt sienna (PBr7 calibrated)
    [0.4, 0.901, 0.829, 1.62, 2.833, 3.24, 3.089],   // 6: Cadmium Red Medium (PR108 calibrated)
    [0.224, 0.22, 0.194, 0.2, 0.196, 0.131, 0.193],  // 7: Bone Black (PBk9 calibrated)
];

pub fn spectrum() -> [[[f32; 4]; 5]; 31] {
    let sums: [f32; 3] = std::array::from_fn(|c| OBSERVER.iter().map(|row| row[c] * row[3]).sum());
    // Normalize each channel to the D65 reference white on this truncated grid.
    let white = [0.95047, 1., 1.08883];
    std::array::from_fn(|i| {
        let anchor = (i / 5).min(5);
        let blend = (i - anchor * 5) as f32 / 5.;
        let interpolate = |curve: &[[f32; 7]; 8], offset: usize| {
            std::array::from_fn(|p| {
                curve
                    .get(p + offset)
                    .map_or(0., |c| c[anchor] * (1. - blend) + c[anchor + 1] * blend)
            })
        };
        [
            interpolate(&ABSORPTION, 0),
            interpolate(&ABSORPTION, 4),
            interpolate(&SCATTERING, 0),
            interpolate(&SCATTERING, 4),
            std::array::from_fn(|c| {
                if c == 3 {
                    0.
                } else {
                    OBSERVER[i][c] * OBSERVER[i][3] * white[c] / sums[c]
                }
            }),
        ]
    })
}

#[allow(dead_code)]
pub fn rgb_composite(wet: [f32; 8], dry: [f32; 8]) -> [u8; 3] {
    let wet_total: f32 = wet.iter().sum();
    let dry_total: f32 = dry.iter().sum();
    if wet_total <= 0.00001 && dry_total <= 0.00001 {
        return [255; 3];
    }
    // Internal reflection coefficient for oil binder (linseed oil n ~ 1.5)
    let k2 = 0.60;
    let mut xyz = [0.; 3];
    for [k0, k1, s0, s1, weight] in spectrum() {
        // 1. Dry underlayer reflectance over white gesso (Rg = 1.0)
        let r_sub = if dry_total <= 0.00001 {
            1.0
        } else {
            let k_dry: f32 = (0..8).map(|i| dry[i] * [k0, k1][i / 4][i % 4]).sum();
            let s_dry: f32 = (0..8).map(|i| dry[i] * [s0, s1][i / 4][i % 4]).sum();
            let s_safe = s_dry.max(0.000001);
            let r = k_dry / s_safe;
            let a = 1.0 + r;
            let b = (r * (r + 2.0)).sqrt();
            let r_inf = 1.0 / (a + b);
            let u = (k_dry * (k_dry + 2.0 * s_dry)).sqrt();
            let e2 = (-2.0 * u).exp();
            (r_inf + e2) / (1.0 + e2 * r_inf)
        };

        // 2. Wet layer reflectance over substrate r_sub
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

            let denom = (1.0 - r_sub * r_inf).max(0.00001);
            let d_val = (r_inf - r_sub) / denom;
            let d_denom = (1.0 - e2 * r_inf * d_val).max(0.00001);
            ((r_inf - e2 * d_val) / d_denom).clamp(0.0, 1.0)
        };

        // 3. Saunderson surface boundary correction (SPEX diffuse body reflectance):
        let r_corrected = (r_int * (1.0 - k2)) / (1.0 - k2 * r_int).max(0.00001);

        for c in 0..3 {
            xyz[c] += r_corrected * weight[c];
        }
    }
    let matrix = [
        [3.2404542, -1.5371385, -0.4985314],
        [-0.969266, 1.8760108, 0.041556],
        [0.0556434, -0.2040259, 1.0572252],
    ];
    matrix.map(|row| {
        let linear = (0..3).map(|i| xyz[i] * row[i]).sum::<f32>().clamp(0., 1.);
        let srgb = if linear <= 0.0031308 {
            12.92 * linear
        } else {
            1.055 * linear.powf(1. / 2.4) - 0.055
        };
        (srgb * 255.).round() as u8
    })
}

#[allow(dead_code)]
pub fn rgb(amount: [f32; 8]) -> [u8; 3] {
    rgb_composite(amount, [0.; 8])
}

#[allow(dead_code)]
pub fn rgb6(amount: [f32; 6]) -> [u8; 3] {
    let mut a = [0.; 8];
    a[..6].copy_from_slice(&amount);
    rgb(a)
}

fn srgb_u8_to_linear(v: u8) -> f32 {
    let s = v as f32 / 255.0;
    if s <= 0.04045 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn invert_3x3(m: [[f32; 3]; 3]) -> [[f32; 3]; 3] {
    let c00 = m[1][1] * m[2][2] - m[1][2] * m[2][1];
    let c01 = m[1][2] * m[2][0] - m[1][0] * m[2][2];
    let c02 = m[1][0] * m[2][1] - m[1][1] * m[2][0];
    let det = m[0][0] * c00 + m[0][1] * c01 + m[0][2] * c02;
    let inv_det = 1.0 / det;
    [
        [
            c00 * inv_det,
            (m[0][2] * m[2][1] - m[0][1] * m[2][2]) * inv_det,
            (m[0][1] * m[1][2] - m[0][2] * m[1][1]) * inv_det,
        ],
        [
            c01 * inv_det,
            (m[0][0] * m[2][2] - m[0][2] * m[2][0]) * inv_det,
            (m[0][2] * m[1][0] - m[0][0] * m[1][2]) * inv_det,
        ],
        [
            c02 * inv_det,
            (m[0][1] * m[2][0] - m[0][0] * m[2][1]) * inv_det,
            (m[0][0] * m[1][1] - m[0][1] * m[1][0]) * inv_det,
        ],
    ]
}

/// Uplifts 1024x1024 sRGB RGBA pixels into 16-bit packed 31-band `DrySubstrate` words.
pub fn uplift_srgb_to_substrate(rgba: &[u8]) -> Vec<u32> {
    let spec = spectrum();
    let basis: [[f32; 3]; 31] = std::array::from_fn(|i| {
        let lam = 400.0 + 10.0 * i as f32;
        let g0 = (-((lam - 450.0) / 55.0).powi(2)).exp();
        let g1 = (-((lam - 540.0) / 50.0).powi(2)).exp();
        let g2 = (-((lam - 625.0) / 60.0).powi(2)).exp();
        let sum = g0 + g1 + g2;
        [g0 / sum, g1 / sum, g2 / sum]
    });

    let xyz_to_rgb = [
        [3.2404542, -1.5371385, -0.4985314],
        [-0.969266, 1.8760108, 0.041556],
        [0.0556434, -0.2040259, 1.0572252],
    ];
    let mut m = [[0.0f32; 3]; 3];
    for j in 0..3 {
        let mut xyz = [0.0f32; 3];
        for i in 0..31 {
            let w = spec[i][4];
            for c in 0..3 {
                xyz[c] += basis[i][j] * w[c];
            }
        }
        for row in 0..3 {
            m[row][j] = (0..3).map(|c| xyz[c] * xyz_to_rgb[row][c]).sum();
        }
    }
    let inv_m = invert_3x3(m);

    let pixel_count = rgba.len() / 4;
    let mut out = vec![0xFFFF_FFFFu32; pixel_count * 16];
    for (p, chunk) in rgba.chunks_exact(4).enumerate() {
        if chunk[0] == 255 && chunk[1] == 255 && chunk[2] == 255 {
            continue;
        }
        let a = chunk[3] as f32 / 255.0;
        let lin_r = srgb_u8_to_linear(chunk[0]) * a + (1.0 - a);
        let lin_g = srgb_u8_to_linear(chunk[1]) * a + (1.0 - a);
        let lin_b = srgb_u8_to_linear(chunk[2]) * a + (1.0 - a);
        let coeffs = [
            inv_m[0][0] * lin_r + inv_m[0][1] * lin_g + inv_m[0][2] * lin_b,
            inv_m[1][0] * lin_r + inv_m[1][1] * lin_g + inv_m[1][2] * lin_b,
            inv_m[2][0] * lin_r + inv_m[2][1] * lin_g + inv_m[2][2] * lin_b,
        ];

        let base = p * 16;
        for w in 0..16 {
            let band = |idx: usize| -> u32 {
                if idx >= 31 {
                    return 0xFFFF;
                }
                let r_corr = (coeffs[0] * basis[idx][0]
                    + coeffs[1] * basis[idx][1]
                    + coeffs[2] * basis[idx][2])
                    .clamp(0.0, 1.0);
                // Invert Saunderson boundary correction (k2 = 0.60)
                let r_int = (r_corr / (0.40 + 0.60 * r_corr)).clamp(0.0, 1.0);
                (r_int * 65535.0).round() as u32
            };
            let lo = band(w * 2);
            let hi = band(w * 2 + 1);
            out[base + w] = lo | (hi << 16);
        }
    }
    out
}

/// WGSL compute and display shader implementing 31-band Kubelka-Munk spectral mixing,
/// Saunderson boundary correction, impasto relief normals, and brush physics.
pub const PAINT_WGSL: &str = include_str!("paint.wgsl");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shader_source_is_embedded() {
        assert!(PAINT_WGSL.contains("fn composite_linear_color"));
    }

    #[test]
    fn kubelka_munk_mixing_produces_expected_hues() {
        let white = rgb([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(white[0] > 240 && white[1] > 240 && white[2] > 240);

        let green = rgb([0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        assert!(green[1] > green[0] && green[1] > green[2]);
    }
}
