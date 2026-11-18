use super::{Action, Button, Painter, Rect, Screen, WHITE};
use crate::vanilla::random::JavaRandom;

pub(crate) struct Title {
    seed: i64,
    seasonal: Option<&'static str>,
    born: std::time::Instant,
    capture: bool,
}
impl Default for Title {
    fn default() -> Self {
        Self {
            seed: JavaRandom::independent_seed(),
            seasonal: seasonal_splash(),
            born: std::time::Instant::now(),
            capture: false,
        }
    }
}
impl Title {
    pub fn reference_capture(&mut self) {
        self.capture = true;
    }
    pub fn elapsed(&self) -> f32 {
        if self.capture {
            4.
        } else {
            self.born.elapsed().as_secs_f32()
        }
    }
}

#[cfg(windows)]
pub(crate) fn local_date() -> (u16, u16) {
    // SYSTEMTIME contains eight consecutive WORD fields. Minecraft uses the local calendar.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocalTime(time: *mut [u16; 8]);
    }
    let mut fields = [0u16; 8];
    unsafe { GetLocalTime(&mut fields) };
    (fields[1], fields[3])
}
#[cfg(not(windows))]
pub(crate) fn local_date() -> (u16, u16) { (0, 0) }
fn seasonal_splash() -> Option<&'static str> {
    match local_date() {
        (12, 24) => Some("Merry X-mas!"),
        (1, 1) => Some("Happy new year!"),
        (10, 31) => Some("OOoooOOOoooo! Spooky!"),
        _ => None,
    }
}

pub(super) fn gradient(p: &mut Painter<'_>, rect: Rect, top: [f32; 4], bottom: [f32; 4]) {
    let start = p.frame.vertices.len();
    p.quad(rect, [-1., -1., 0., 0.], 0, top);
    for i in [1, 2, 4] {
        p.frame.vertices[start + i].color = bottom;
    }
}
fn layout(w: f32, h: f32) -> [Rect; 6] {
    let cx = (w * 0.5).floor();
    let y = (h / 4.).floor() + 48.;
    let button = |offset, dy, width| Rect {
        x: cx + offset,
        y: y + dy,
        w: width,
        h: 20.,
    };
    [
        button(-100., 0., 200.),
        button(-100., 24., 200.),
        button(-100., 48., 200.),
        button(-100., 84., 98.),
        button(2., 84., 98.),
        button(-124., 84., 20.),
    ]
}

pub(super) fn draw(p: &mut Painter<'_>, title: &Title, seconds: f32) {
    let [projection_width, projection_height] = p.frame.size;
    let [w, h] = p.frame.size.map(f32::ceil);
    let cx = (w / 2.).floor();
    let rect = Rect { x: 0., y: 0., w, h };
    gradient(p, rect, [1., 1., 1., 128. / 255.], [1., 1., 1., 0.]);
    gradient(p, rect, [0.; 4], [0., 0., 0., 128. / 255.]);
    let mut random = JavaRandom::new(title.seed);
    let splashes = &p.r.splashes;
    let splash = if splashes.is_empty() {
        "missingno"
    } else {
        &splashes[random.next_int(splashes.len() as i32) as usize]
    };
    let mut logo = |offset, u, v, width| {
        p.image(
            "textures/gui/title/minecraft.png",
            Rect {
                x: cx - 137. + offset,
                y: 30.,
                w: width,
                h: 44.,
            },
            [u, v, width, 44.],
            WHITE,
        )
    };
    if random.next_float() < 0.0001 && !title.capture {
        for (x, u, width) in [
            (0., 0., 99.),
            (99., 129., 27.),
            (125., 126., 3.),
            (128., 99., 26.),
        ] {
            logo(x, u, 0., width);
        }
    } else {
        logo(0., 0., 0., 155.);
    }
    logo(155., 0., 45., 155.);
    let splash = if title.capture {
        "Minecraft!"
    } else {
        title.seasonal.unwrap_or(splash)
    };
    let pulse =
        1.8 - 0.1 * crate::vanilla::math::sin(seconds.fract() * std::f32::consts::TAU).abs();
    let scale = pulse * 100. / (p.width(splash) + 32.);
    let (sin, cos) = (-20f32).to_radians().sin_cos();
    let start = p.frame.vertices.len();
    p.center(0., -8., splash, [1., 1., 0., 1.]);
    for vertex in &mut p.frame.vertices[start..] {
        let x = (vertex.position[0] + 1.) * projection_width * 0.5;
        let y = (1. - vertex.position[1]) * projection_height * 0.5;
        vertex.position = [
            (cx + 90. + (x * cos - y * sin) * scale) * 2. / projection_width - 1.,
            1. - (70. + (x * sin + y * cos) * scale) * 2. / projection_height,
        ];
    }
    let buttons = layout(w, h);
    for ((key, action), rect) in [
        ("menu.singleplayer", Action::Screen(Screen::Worlds)),
        ("menu.multiplayer", Action::Screen(Screen::Multiplayer)),
        ("menu.online", Action::Screen(Screen::Realms)),
        ("menu.options", Action::Screen(Screen::Options)),
        ("menu.quit", Action::Quit),
    ]
    .into_iter()
    .zip(buttons)
    {
        p.button(rect, &p.tr(key), action, true);
    }
    let globe = buttons[5];
    p.image(
        "textures/gui/widgets.png",
        globe,
        [
            0.,
            if globe.contains(p.mouse) { 126. } else { 106. },
            20.,
            20.,
        ],
        WHITE,
    );
    p.frame.buttons.push(Button {
        rect: globe,
        action: Action::Screen(Screen::Languages),
        enabled: true,
    });
    p.text(2., h - 10., "Minecraft 1.8.9", WHITE);
    let footer = "Copyright Mojang AB. Do not distribute!";
    p.text(w - p.width(footer) - 2., h - 10., footer, WHITE);
}

pub(super) fn realms(p: &mut Painter<'_>) {
    let [w, h] = p.frame.size.map(f32::ceil);
    p.background();
    p.center(w / 2., h / 2. - 30., &p.tr("menu.online"), WHITE);
    let message = if p.s.language == "ko_kr" {
        "이 클라이언트에서는 Realms 서비스를 사용할 수 없습니다."
    } else {
        "Realms service is unavailable in this client."
    };
    p.center(w / 2., h / 2. - 8., message, super::GRAY);
    p.button(
        Rect {
            x: (w / 2.).floor() - 100.,
            y: h / 2. + 24.,
            w: 200.,
            h: 20.,
        },
        &p.tr("gui.back"),
        Action::Screen(Screen::Main),
        true,
    );
}
