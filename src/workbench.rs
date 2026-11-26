use std::collections::VecDeque;

pub const CUSTOM_JAR_COUNT: usize = 64;
pub const MAX_UNDO: usize = 64;

#[derive(Clone, Copy)]
pub struct Session {
    pub loads: [f32; 2],
    pub jars: [bool; CUSTOM_JAR_COUNT],
}

impl Default for Session {
    fn default() -> Self {
        Self {
            loads: [0.0; 2],
            jars: [false; CUSTOM_JAR_COUNT],
        }
    }
}

pub struct Workbench {
    pub session: Session,
    pub history: VecDeque<Session>,
    pub sizes: [f32; 2],
    pub angle: f32,
    pub sampling: bool,
    pub guide: Option<usize>,
    pub exercise: bool,
    pub lighting: crate::gpu::Lighting,
}

impl Default for Workbench {
    fn default() -> Self {
        Self {
            session: Session::default(),
            history: VecDeque::new(),
            sizes: [64., 64.],
            angle: 0.,
            sampling: false,
            guide: None,
            exercise: false,
            lighting: crate::gpu::Lighting::default(),
        }
    }
}

impl Workbench {
    pub fn snapshot(&mut self) {
        if self.history.len() == MAX_UNDO {
            self.history.pop_front();
        }
        self.history.push_back(self.session);
    }

    pub fn undo(&mut self) -> bool {
        let Some(session) = self.history.pop_back() else {
            return false;
        };
        self.session = session;
        true
    }
}

pub struct PigmentEntry {
    pub code: &'static str,
    pub character: &'static str,
    pub use_case: &'static str,
    pub prompt: &'static str,
    pub mix: [u32; 4],
    pub source: &'static str,
}

const OIL_COLORS: &str = "https://gamblincolors.com/oil-painting/color/artist-grade-oil-colors/";

pub const GUIDE: [PigmentEntry; 6] = [
    PigmentEntry {
        code: "PY74 · Hansa Yellow Medium reference",
        character: "A semi-transparent yellow with strong coloring power in mixtures.",
        use_case: "Use it for warm light or mix it into blue for greens.",
        prompt: "Work a tiny amount of Phthalo Blue into a dab of yellow. Add more blue in stages.",
        mix: [1, 2, 0, 0],
        source: OIL_COLORS,
    },
    PigmentEntry {
        code: "PB15:2 · Phthalo Blue reference",
        character: "A transparent blue with high tinting strength. A little goes a long way.",
        use_case: "Use it for vivid greens and cool blue tints.",
        prompt:
            "Use the tip of a clean brush to add a small amount to white and see how far it goes.",
        mix: [2, 4, 0, 0],
        source: "https://gamblincolors.com/phthalo-blue-6/",
    },
    PigmentEntry {
        code: "PR122 · Quinacridone Magenta reference",
        character: "A semi-transparent, cool red with strong tinting power.",
        use_case: "Mix violet with Ultramarine or add white for a pink tint.",
        prompt:
            "Pull a little magenta into Ultramarine and compare the violet with the original blue.",
        mix: [3, 5, 0, 0],
        source: OIL_COLORS,
    },
    PigmentEntry {
        code: "PW6 · Titanium White reference",
        character: "An opaque white with strong tinting power.",
        use_case: "Lighten a mixture or paint a covering highlight.",
        prompt:
            "Add white to a little Burnt Sienna, then compare the tint to the unmixed earth color.",
        mix: [4, 6, 0, 0],
        source: OIL_COLORS,
    },
    PigmentEntry {
        code: "PB29 · Ultramarine Blue reference",
        character: "A transparent, violet-leaning blue with moderate tinting strength.",
        use_case: "Use it for thin glazes or mix it with Burnt Sienna for muted shadows.",
        prompt: "Mix a blue-gray, then add another touch of Burnt Sienna and see how it warms.",
        mix: [5, 6, 0, 0],
        source: "https://gamblincolors.com/ultramarine-blue/",
    },
    PigmentEntry {
        code: "PBr7 · Burnt Sienna reference",
        character: "A warm, transparent earth brown with moderate tinting power.",
        use_case: "Use it for warm underpainting and for muting blues in shadows.",
        prompt:
            "Mix a little Ultramarine in, then compare the neutral with a tint of the plain brown.",
        mix: [6, 5, 4, 0],
        source: OIL_COLORS,
    },
];
