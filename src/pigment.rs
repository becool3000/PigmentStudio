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
    [4., 3., 0.9, 0.006, 0.002, 0.002, 0.003], // 0: Hansa yellow
    [1.5, 0.8, 1.0, 3.2, 18.0, 38.0, 30.0],    // 1: Phthalo blue (PB15)
    [1.8, 1.2, 8.5, 28.0, 1.2, 0.35, 0.25],    // 2: Quinacridone magenta (PR122)
    [0.008, 0.004, 0.002, 0.001, 0.001, 0.001, 0.002], // 3: Titanium white
    [0.06, 0.09, 0.55, 2.4, 3.5, 1.6, 0.9],    // 4: Ultramarine blue
    [4.5, 4.2, 2.8, 1.6, 0.55, 0.21, 0.13],    // 5: Burnt sienna
    [35.0, 32.0, 26.0, 14.0, 0.03, 0.005, 0.002], // 6: Genuine Vermilion / Cadmium Red (PR106/PR108)
    [22.0, 22.0, 22.0, 22.0, 22.0, 21.5, 21.0],   // 7: Bone Black / Genuine Sepia (PBk9)
];
const SCATTERING: [[f32; 7]; 8] = [
    [0.42, 0.45, 0.65, 0.75, 0.67, 0.55, 0.5], // 0: Hansa yellow
    [0.25, 0.22, 0.20, 0.18, 0.15, 0.12, 0.10], // 1: Phthalo blue (PB15)
    [0.45, 0.42, 0.38, 0.35, 0.40, 0.45, 0.45], // 2: Quinacridone magenta (PR122)
    [18.0, 22.0, 26.0, 28.0, 28.0, 26.0, 24.0], // 3: Titanium white
    [0.95, 1.1, 1.1, 0.91, 0.87, 0.91, 1.0],   // 4: Ultramarine blue
    [0.68, 0.75, 0.8, 0.82, 0.81, 0.75, 0.7],  // 5: Burnt sienna
    [0.6, 0.7, 0.8, 1.2, 2.2, 2.4, 2.5],       // 6: Genuine Vermilion / Cadmium Red
    [0.3, 0.3, 0.3, 0.3, 0.3, 0.3, 0.3],       // 7: Bone Black / Genuine Sepia
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
