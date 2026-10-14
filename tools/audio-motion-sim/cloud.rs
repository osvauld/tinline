//! Native numerical port of design-refinement's calls/field/model.ts.
//! Same five Lissajous masses, OKLab palettes, state table and domain-warp parameters.
//! Speech replaces fake bursts with bounded externally supplied energy/spectral balance.
//! No audio, renderer, allocation or clocks here. Cue-bus integration is NOT implemented.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum State {
    Calm,
    Incoming,
    Outgoing,
    Connecting,
    Active,
    Waiting,
    Reconnecting,
    Ended,
    Failed,
}
impl State {
    pub fn label(self) -> &'static str {
        match self {
            Self::Calm => "Ready",
            Self::Incoming => "Incoming call",
            Self::Outgoing => "Calling…",
            Self::Connecting => "Connecting…",
            Self::Active | Self::Waiting => "Connected",
            Self::Reconnecting => "Reconnecting…",
            Self::Ended => "Call ended",
            Self::Failed => "Couldn’t connect",
        }
    }
    // v, a0, warp, pull, spread, chroma, kappa, swell, ak, bias. blend ms.
    fn config(self) -> ([f64; 10], f64) {
        match self {
            Self::Calm => ([0.50, 0.8, 0.08, 0., 0., 1., 1., 0.30, 0.45, 0.35], 900.),
            Self::Incoming => ([1., 0.9, 0.12, 0., 0., 1., 2.6, 0.38, 1., 0.74], 300.),
            Self::Outgoing => ([0.42, 0.85, 0.12, 0., 0., 1., 2.2, 0.38, 1.1, 0.08], 450.),
            Self::Connecting => ([0.80, 0.85, 0.012, 1., 0., 1., 1.6, 0.30, 0.7, 0.12], 350.),
            Self::Active => ([0.30, 0.6, 0.07, 0., 0., 1., 1., 0.30, 0.6, 0.30], 1200.),
            Self::Waiting => ([0.34, 0.65, 0.07, 0., 0., 1., 1.2, 0.32, 0.8, 0.52], 900.),
            Self::Reconnecting => (
                [0.22, 0.55, 0.05, 0.5, 0., 0.45, 1.2, 0.30, 0.8, 0.20],
                700.,
            ),
            Self::Ended => ([0.60, 0.8, 0.10, 0., 0.55, 1., 1.6, 0.30, 0.6, 0.40], 300.),
            Self::Failed => ([0.30, 0.7, 0.05, 0., 0., 0., 1.2, 0.30, 0.6, 0.20], 500.),
        }
    }
    pub fn speech(self) -> bool {
        matches!(self, Self::Active | Self::Waiting)
    }
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub level: f64,
    pub balance: f64,
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Lobe {
    pub x: f64,
    pub y: f64,
    pub sigma: f64,
    pub alpha: f64,
    pub lab: [f64; 3],
}
#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub tau: f64,
    pub warp: f64,
    pub cap: f64,
    pub fade: f64,
    pub bg: [f64; 3],
    pub lobes: [Lobe; 5],
}
#[derive(Clone, Debug)]
pub struct Field {
    pub tau: f64,
    pub reduced: bool,
    pub state: State,
    s: [f64; 10],
    colour: [f64; 5],
    energy: f64,
    balance: f64,
    geometry_energy: f64,
    geometry_balance: f64,
    fade: f64,
    since: f64,
}
fn clean(x: f64) -> f64 {
    if x.is_finite() { x.clamp(0., 1.) } else { 0. }
}
/// Preview-only gain on the existing level descriptor. Never changes the capture audibility gate.
pub fn microphone_input(level: u8, audible: bool, balance: f64, sensitivity: f64) -> Input {
    if !audible {
        return Input::default();
    }
    let gain = if sensitivity.is_finite() {
        sensitivity.clamp(0.5, 3.)
    } else {
        1.6
    };
    Input {
        level: clean(((60. - level as f64) / 50.) * gain),
        balance: clean(balance),
    }
}
fn smooth(x: f64) -> f64 {
    let x = clean(x);
    x * x * (3. - 2. * x)
}
fn mix(a: f64, b: f64, x: f64) -> f64 {
    a + (b - a) * x
}
const LJ: [[f64; 8]; 5] = [
    [-0.55, -0.50, 0.30, 0.34, 1.0000, 1.3247, 0.3, 1.9],
    [0.62, -0.20, 0.34, 0.28, 0.7071, 1.6180, 2.2, 0.7],
    [-0.30, 0.10, 0.28, 0.38, 1.4142, 0.8660, 4.1, 3.3],
    [0.50, 0.45, 0.36, 0.30, 1.1180, 1.3591, 1.0, 5.0],
    [-0.15, 0.80, 0.32, 0.32, 0.9069, 1.7321, 5.5, 2.4],
];
impl Field {
    pub fn new() -> Self {
        let (s, _) = State::Active.config();
        Self {
            tau: 0.,
            reduced: false,
            state: State::Active,
            s,
            colour: std::array::from_fn(|i| clean(s[9] + (i as f64 - 2.) * 0.07)),
            energy: 0.,
            balance: 0.5,
            geometry_energy: 0.,
            geometry_balance: 0.5,
            fade: 1.,
            since: 0.,
        }
    }
    pub fn is_idle(&self) -> bool {
        matches!(self.state, State::Ended | State::Failed) && self.fade == 0.
    }
    pub fn set_state(&mut self, state: State) {
        if self.state != state {
            self.state = state;
            self.since = 0.;
        }
    }
    pub fn step(&mut self, dt: f64, input: Input) {
        let dt = if dt.is_finite() {
            dt.clamp(0., 0.1)
        } else {
            0.
        };
        self.since += dt;
        let (target, blend) = self.state.config();
        for (i, value) in target.into_iter().enumerate() {
            // Freeze existing geometry, not a different static preset: no toggle snap.
            if self.reduced && matches!(i, 2 | 3 | 4 | 7) {
                continue;
            }
            let override_ms = match (self.state, i) {
                (State::Connecting, 2) => 330.,
                (State::Connecting, 3) => 520.,
                (State::Active, 3) => 1400.,
                (State::Reconnecting, 3) => 900.,
                (State::Reconnecting, 5) => 500.,
                (State::Ended, 4) => 320.,
                (State::Failed, 5) => 160.,
                _ => blend,
            };
            self.s[i] += (value - self.s[i]) * (1. - (-dt / (override_ms / 3200.)).exp());
        }
        let level = if self.state.speech() {
            clean(input.level)
        } else {
            0.
        };
        self.energy += (level - self.energy)
            * (1. - (-dt / if level > self.energy { 0.045 } else { 0.32 }).exp());
        self.balance += (clean(input.balance) - self.balance) * (1. - (-dt / 0.18).exp());
        for i in 0..5 {
            let palette = clean(target[9] + (i as f64 - 2.) * 0.07);
            // This is measured spectral balance, NOT the original cue-note pitch mapping.
            let wanted = mix(
                palette,
                clean(self.balance + (i as f64 - 2.) * 0.07),
                0.80 * self.energy,
            );
            self.colour[i] +=
                (wanted - self.colour[i]) * (1. - (-dt / (150_f64.max(blend / 4.) / 1000.)).exp());
        }
        if !self.reduced {
            self.tau += dt * self.s[0] * (1. + 0.5 * self.energy);
            self.geometry_energy +=
                (self.energy - self.geometry_energy) * (1. - (-dt / 0.07).exp());
            self.geometry_balance +=
                (self.balance - self.geometry_balance) * (1. - (-dt / 0.12).exp());
        }
        let fade = match self.state {
            State::Ended => Some((0., 0.3)),
            State::Failed => Some((0.35, 0.65)),
            _ => None,
        };
        self.fade = (self.fade
            + match fade {
                Some((delay, dur)) if self.since >= delay => -dt / dur,
                _ => dt / 0.4,
            })
        .clamp(0., 1.);
    }
    pub fn frame(&self, w: f64, h: f64, dark: bool, group: bool) -> Frame {
        let short = w.min(h).max(1.);
        let half = [w / short / 2., h / short / 2.];
        let (bg, teal, gold) = if dark {
            ([15., 21., 19.], [20., 184., 160.], [236., 126., 64.])
        } else {
            ([245., 246., 243.], [110., 210., 186.], [248., 206., 104.])
        };
        let teal = rgb_to_lab(teal);
        let gold = rgb_to_lab(gold);
        let tau = self.tau;
        let lobes = std::array::from_fn(|i| {
            let j = LJ[i];
            let mut x =
                j[0] * half[0] + j[2] * (half[0] / 0.5).max(1.) * (j[4] * 0.09 * tau + j[6]).sin();
            let mut y =
                j[1] * half[1] + j[3] * (half[1] / 0.5).max(1.) * (j[5] * 0.09 * tau + j[7]).sin();
            x = mix(x, 0., self.s[3] * 0.65);
            y = mix(
                y,
                (if group { 96. } else { h * 0.35 } - h / 2.) / short,
                self.s[3] * 0.65,
            );
            x *= 1. + self.s[4];
            y *= 1. + self.s[4];
            let geometry_band =
                (1. - ((i as f64 + 0.5) - self.geometry_balance * 5.).abs() / 1.6).max(0.);
            // Bounded coherent redistribution, not faster wandering or new particles.
            let direction = i as f64 * 2.3999632297;
            let shift = 0.085 * self.geometry_energy * (geometry_band - 0.25);
            x += shift * direction.cos();
            y += shift * direction.sin();
            let mut curve = smooth((self.colour[i] - 0.2) / 0.75);
            if dark {
                curve = smooth(smooth(curve));
            }
            let mut lab = mix_lab(teal, gold, curve);
            lab[1] *= self.s[5];
            lab[2] *= self.s[5];
            // Five persistent masses; spectral energy changes their individual swell, never spawns geometry.
            let band = (1. - ((i as f64 + 0.5) - self.balance * 5.).abs() / 1.6).max(0.);
            let lv = self.energy * band;
            Lobe {
                x,
                y,
                sigma: (0.26
                    * (1.
                        + self.s[7] * 1.6 * self.geometry_energy * geometry_band
                        + 0.36 * self.geometry_energy)
                    * (1. - 0.3 * self.s[3]))
                    .min(0.49),
                alpha: (self.s[1] + 0.18 * self.energy + 0.8 * lv).clamp(0., 2.)
                    * smooth(self.fade),
                lab,
            }
        });
        Frame {
            tau,
            warp: self.s[2] + 0.09 * self.geometry_energy,
            cap: if dark { 0.74 } else { 0.76 },
            fade: smooth(self.fade),
            bg: rgb_to_lab(bg),
            lobes,
        }
    }
}
pub fn rgb_to_lab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|x| {
        let x = x / 255.;
        if x <= 0.04045 {
            x / 12.92
        } else {
            ((x + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}
pub fn lab_to_rgb(c: [f64; 3]) -> [f64; 3] {
    let l = (c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2]).powi(3);
    let m = (c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2]).powi(3);
    let s = (c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2]).powi(3);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
    .map(|x| {
        let x = x.clamp(0., 1.);
        255. * if x <= 0.0031308 {
            12.92 * x
        } else {
            1.055 * x.powf(1. / 2.4) - 0.055
        }
    })
}
fn mix_lab(a: [f64; 3], b: [f64; 3], x: f64) -> [f64; 3] {
    let mut c = std::array::from_fn(|i| mix(a[i], b[i], x));
    let want = mix(a[1].hypot(a[2]), b[1].hypot(b[2]), x);
    let have = c[1].hypot(c[2]);
    if have > 1e-5 {
        c[1] *= want / have;
        c[2] *= want / have;
    }
    c
}
