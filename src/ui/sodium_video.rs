use super::{text_field::TextField, Action, Button, Gui, Painter, Rect, Screen, UiVertex};
use crate::settings::{Setting, Settings};
use winit::{event::MouseButton, keyboard::KeyCode};

const fn argb(hex: u32) -> [f32; 4] {
    let a = ((hex >> 24) & 0xFF) as f32 / 255.0;
    let r = ((hex >> 16) & 0xFF) as f32 / 255.0;
    let g = ((hex >> 8) & 0xFF) as f32 / 255.0;
    let b = (hex & 0xFF) as f32 / 255.0;
    [r, g, b, a]
}

const THEME: [f32; 4] = argb(0xFF94E4D3);
const THEME_LIGHTER: [f32; 4] = argb(0xFFCCFDEE);
const THEME_DARKER: [f32; 4] = argb(0xFF7A9E9E);
const FOREGROUND: [f32; 4] = argb(0xFFFFFFFF);
const FOREGROUND_DISABLED: [f32; 4] = argb(0xFFAAAAAA);
const BG_LIGHT: [f32; 4] = argb(0x40000000);
const BG_MEDIUM: [f32; 4] = argb(0x60000000);
const BG_DEFAULT: [f32; 4] = argb(0x90000000);
const BG_DARKER: [f32; 4] = argb(0xB0000000);
const BG_HOVER: [f32; 4] = argb(0xE0000000);
const BG_OVERLAY: [f32; 4] = argb(0xEA000000);
const BUTTON_BORDER: [f32; 4] = argb(0x8000FFEE);
const SCROLL_TRACK: [f32; 4] = argb(0x96323232);
const SCROLL_THUMB: [f32; 4] = argb(0x96646464);
const RESET_ORANGE: [f32; 4] = argb(0xFFFF8C30);

const DONATE_URL: &str = "https://caffeinemc.net/donate";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Impact {
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ControlKind {
    TickBox,
    Cycling,
    Slider { min: i32, max: i32, step: i32 },
}

const PAGES: [(&str, &str, &[&[Setting]]); 3] = [
    (
        "General",
        "일반",
        &[
            &[Setting::Distance, Setting::Brightness],
            &[Setting::GuiScale, Setting::Fullscreen, Setting::Vsync],
            &[Setting::Fps, Setting::Bobbing, Setting::Anaglyph],
        ],
    ),
    (
        "Quality",
        "품질",
        &[
            &[
                Setting::Graphics,
                Setting::Clouds,
                Setting::Particles,
                Setting::Lighting,
            ],
            &[Setting::Mipmaps, Setting::Shadows, Setting::Alternatives],
        ],
    ),
    (
        "Performance",
        "성능",
        &[
            &[Setting::Uploads, Setting::Greedy],
            &[Setting::Culling, Setting::EntityDistance],
        ],
    ),
];

#[derive(Clone, Debug)]
pub enum Effect {
    None,
    PlayClick,
    Apply(Box<Settings>),
    Close,
    OpenUrl(&'static str),
}

#[derive(Default)]
pub struct State {
    pub search: TextField,
    pub search_focused: bool,
    pub hide_donate: bool,
    pub selected_page: usize,
    pub scroll: f32,
    pub tooltip_scroll: f32,
    pub pending: Option<Settings>,
    pub shift_held: bool,
    pub alt_held: bool,
    drag_slider: Option<(Setting, Rect)>,
    drag_scrollbar: Option<(f32, f32)>,
    last_hovered: Option<Setting>,
}

impl State {
    pub fn reset_view(&mut self) {
        self.search = TextField::default();
        self.search_focused = false;
        self.selected_page = 0;
        self.scroll = 0.0;
        self.tooltip_scroll = 0.0;
        self.pending = None;
        self.shift_held = false;
        self.alt_held = false;
        self.drag_slider = None;
        self.drag_scrollbar = None;
        self.last_hovered = None;
    }

    pub fn effective<'a>(&'a self, committed: &'a Settings) -> &'a Settings {
        self.pending.as_ref().unwrap_or(committed)
    }

    pub fn has_changes(&self, committed: &Settings) -> bool {
        self.pending
            .as_ref()
            .is_some_and(|staged| video_differs(committed, staged))
    }

    fn mutate(&mut self, committed: &Settings, f: impl FnOnce(&mut Settings)) {
        let mut next = self.pending.take().unwrap_or_else(|| committed.clone());
        f(&mut next);
        next.validate();
        if video_differs(committed, &next) {
            self.pending = Some(next);
        }
    }

    pub fn release(&mut self) {
        self.drag_slider = None;
        self.drag_scrollbar = None;
    }
}

#[derive(Clone, Copy)]
struct Layout {
    x0: f32,
    y0: f32,
    content_w: f32,
    content_h: f32,
    inset_x: bool,
    inset_y: bool,
    search_rect: Rect,
    donate_rect: Option<Rect>,
    hide_donate_rect: Option<Rect>,
    page_rect: Rect,
    list_outer: Rect,
    list_inner: Rect,
    tooltip_pane: Option<Rect>,
    undo_rect: Rect,
    apply_rect: Rect,
    done_rect: Rect,
}

#[derive(Clone, Copy)]
enum EntryKind {
    ModHeader,
    PageHeader(usize),
    Option { page: usize, setting: Setting },
}

#[derive(Clone, Copy)]
struct DocEntry {
    y: f32,
    h: f32,
    kind: EntryKind,
}

fn is_korean(lang: &str) -> bool {
    lang.eq_ignore_ascii_case("ko_kr")
}

fn compute_layout(size: [f32; 2], hide_donate: bool) -> Layout {
    let [sw, sh] = size;
    let inset_x = sw > 605.0;
    let content_w = if inset_x {
        if sw < 655.0 {
            505.0 + (sw - 605.0)
        } else {
            555.0
        }
    } else {
        sw
    };
    let inset_y = inset_x && sh > 312.0;
    let content_h = if inset_y { sh - 12.0 } else { sh };
    let x0 = ((sw - content_w) * 0.5).floor();
    let y0 = ((sh - content_h) * 0.5).floor();

    let top_x = x0 + if inset_x { 0.0 } else { 6.0 };
    let top_y = y0 + if inset_y { 0.0 } else { 6.0 };
    let top_w = (content_w - if inset_x { 0.0 } else { 12.0 }).max(80.0);

    let show_donate = !hide_donate && top_w >= 360.0;
    let (search_w, donate_rect, hide_donate_rect) = if show_donate {
        let hide_r = Rect {
            x: top_x + top_w - 20.0,
            y: top_y,
            w: 20.0,
            h: 20.0,
        };
        let don_r = Rect {
            x: hide_r.x - 103.0,
            y: top_y,
            w: 100.0,
            h: 20.0,
        };
        (top_w - 129.0, Some(don_r), Some(hide_r))
    } else {
        (top_w, None, None)
    };
    let search_rect = Rect {
        x: top_x,
        y: top_y,
        w: search_w,
        h: 20.0,
    };

    let body_y = top_y + 26.0;
    let page_w = if content_w < 360.0 {
        (content_w - 225.0).clamp(80.0, 125.0)
    } else {
        125.0
    };
    let page_x = x0 + if inset_x { 0.0 } else { 0.0 };
    let list_x = page_x + page_w + if inset_x { 3.0 } else { 0.0 };
    let list_w = (content_w - page_w - 3.0).clamp(120.0, 222.0);

    let right_x = list_x + list_w + 6.0;
    let right_edge = x0 + content_w - if inset_x { 0.0 } else { 6.0 };
    let right_w = right_edge - right_x;

    let btn_y = y0 + content_h - if inset_y { 20.0 } else { 24.0 };
    let btn_w = if content_w < 360.0 { 58.0 } else { 65.0 };
    let btn_gap = if content_w < 360.0 { 4.0 } else { 6.0 };
    let done_rect = Rect {
        x: right_edge - btn_w,
        y: btn_y,
        w: btn_w,
        h: 20.0,
    };
    let apply_rect = Rect {
        x: done_rect.x - btn_gap - btn_w,
        y: btn_y,
        w: btn_w,
        h: 20.0,
    };
    let undo_rect = Rect {
        x: apply_rect.x - btn_gap - btn_w,
        y: btn_y,
        w: btn_w,
        h: 20.0,
    };

    // On wide viewports, Undo/Apply/Done sit cleanly under the right tooltip pane.
    // On narrow viewports (e.g. 320x240), stop the sidebar and option list above the buttons.
    let buttons_overlap_list = undo_rect.x < list_x + list_w + 2.0;
    let body_bottom = if buttons_overlap_list {
        (btn_y - 4.0).max(body_y + 40.0)
    } else {
        y0 + content_h - if inset_y { 0.0 } else { 4.0 }
    };
    let body_h = (body_bottom - body_y).max(40.0);

    let page_rect = Rect {
        x: page_x,
        y: body_y,
        w: page_w,
        h: body_h,
    };
    let list_outer = Rect {
        x: list_x,
        y: body_y,
        w: list_w,
        h: body_h,
    };
    let list_inner = Rect {
        x: list_x + 3.0,
        y: body_y,
        w: (list_w - 3.0).max(60.0),
        h: body_h,
    };
    let tooltip_pane = (right_w >= 96.0).then_some(Rect {
        x: right_x,
        y: body_y,
        w: right_w,
        h: (btn_y - 6.0 - body_y).max(40.0),
    });

    Layout {
        x0,
        y0,
        content_w,
        content_h,
        inset_x,
        inset_y,
        search_rect,
        donate_rect,
        hide_donate_rect,
        page_rect,
        list_outer,
        list_inner,
        tooltip_pane,
        undo_rect,
        apply_rect,
        done_rect,
    }
}

fn control_kind(setting: Setting) -> ControlKind {
    match setting {
        Setting::Distance => ControlKind::Slider {
            min: 2,
            max: 128,
            step: 1,
        },
        Setting::Brightness => ControlKind::Slider {
            min: 0,
            max: 100,
            step: 1,
        },
        Setting::GuiScale => ControlKind::Cycling,
        Setting::Fullscreen => ControlKind::TickBox,
        Setting::Vsync => ControlKind::TickBox,
        Setting::Fps => ControlKind::Slider {
            min: 10,
            max: 260,
            step: 10,
        },
        Setting::Bobbing => ControlKind::TickBox,
        Setting::Anaglyph => ControlKind::TickBox,
        Setting::Graphics => ControlKind::Cycling,
        Setting::Clouds => ControlKind::Cycling,
        Setting::Particles => ControlKind::Cycling,
        Setting::Lighting => ControlKind::Cycling,
        Setting::Mipmaps => ControlKind::Slider {
            min: 0,
            max: 4,
            step: 1,
        },
        Setting::Shadows => ControlKind::TickBox,
        Setting::Alternatives => ControlKind::TickBox,
        Setting::Uploads => ControlKind::Slider {
            min: 1,
            max: 16,
            step: 1,
        },
        Setting::Greedy => ControlKind::TickBox,
        Setting::Culling => ControlKind::TickBox,
        Setting::EntityDistance => ControlKind::Slider {
            min: 16,
            max: 192,
            step: 8,
        },
        _ => ControlKind::Cycling,
    }
}

fn option_name(p: &Painter<'_>, setting: Setting) -> String {
    let ko = is_korean(&p.s.language);
    match setting {
        Setting::Distance => p.tr("options.renderDistance"),
        Setting::Brightness => p.tr("options.gamma"),
        Setting::GuiScale => p.tr("options.guiScale"),
        Setting::Fullscreen => p.tr("options.fullscreen"),
        Setting::Vsync => p.tr("options.vsync"),
        Setting::Fps => p.tr("options.framerateLimit"),
        Setting::Bobbing => p.tr("options.viewBobbing"),
        Setting::Anaglyph => p.tr("options.anaglyph"),
        Setting::Graphics => p.tr("options.graphics"),
        Setting::Clouds => p.tr("options.renderClouds"),
        Setting::Particles => p.tr("options.particles"),
        Setting::Lighting => p.tr("options.ao"),
        Setting::Mipmaps => p.tr("options.mipmapLevels"),
        Setting::Shadows => p.tr("options.entityShadows"),
        Setting::Alternatives => p.tr("options.blockAlternatives"),
        Setting::Uploads => {
            if ko {
                "청크 업로드 제한".into()
            } else {
                "Chunk Update Threads".into()
            }
        }
        Setting::Greedy => {
            if ko {
                "그리디 메싱 (블록 병합)".into()
            } else {
                "Compact Face Meshing".into()
            }
        }
        Setting::Culling => {
            if ko {
                "엔티티 오클루전 컬링".into()
            } else {
                "Use Entity Culling".into()
            }
        }
        Setting::EntityDistance => {
            if ko {
                "엔티티 렌더링 거리".into()
            } else {
                "Entity Distance".into()
            }
        }
        _ => format!("{setting:?}"),
    }
}

fn option_name_plain(lang: &str, setting: Setting) -> &'static str {
    let ko = is_korean(lang);
    match setting {
        Setting::Distance => {
            if ko {
                "렌더 거리 시야 render distance"
            } else {
                "render distance"
            }
        }
        Setting::Brightness => {
            if ko {
                "밝기 감마 brightness gamma"
            } else {
                "brightness gamma"
            }
        }
        Setting::GuiScale => {
            if ko {
                "gui 크기 인터페이스 배율 scale"
            } else {
                "gui scale"
            }
        }
        Setting::Fullscreen => {
            if ko {
                "전체 화면 fullscreen"
            } else {
                "fullscreen"
            }
        }
        Setting::Vsync => {
            if ko {
                "수직 동기화 vsync"
            } else {
                "vsync vertical sync"
            }
        }
        Setting::Fps => {
            if ko {
                "최대 프레임 제한 max framerate fps"
            } else {
                "max framerate fps"
            }
        }
        Setting::Bobbing => {
            if ko {
                "화면 흔들림 view bobbing"
            } else {
                "view bobbing"
            }
        }
        Setting::Anaglyph => {
            if ko {
                "3d 애너글리프 입체 anaglyph"
            } else {
                "3d anaglyph"
            }
        }
        Setting::Graphics => {
            if ko {
                "그래픽 품질 빠름 화려함 graphics quality"
            } else {
                "graphics fast fancy"
            }
        }
        Setting::Clouds => {
            if ko {
                "구름 clouds"
            } else {
                "clouds"
            }
        }
        Setting::Particles => {
            if ko {
                "입자 파티클 particles"
            } else {
                "particles"
            }
        }
        Setting::Lighting => {
            if ko {
                "부드러운 조명 smooth lighting ao"
            } else {
                "smooth lighting ambient occlusion"
            }
        }
        Setting::Mipmaps => {
            if ko {
                "밉맵 단계 mipmap levels"
            } else {
                "mipmap levels"
            }
        }
        Setting::Shadows => {
            if ko {
                "엔티티 그림자 entity shadows"
            } else {
                "entity shadows"
            }
        }
        Setting::Alternatives => {
            if ko {
                "대체 블록 텍스처 alternate blocks"
            } else {
                "alternate blocks"
            }
        }
        Setting::Uploads => {
            if ko {
                "청크 업로드 스레드 프레임 chunk uploads"
            } else {
                "chunk uploads per frame"
            }
        }
        Setting::Greedy => {
            if ko {
                "그리디 메싱 폴리곤 최적화 greedy meshing"
            } else {
                "greedy meshing compact vertex"
            }
        }
        Setting::Culling => {
            if ko {
                "엔티티 컬링 가림 최적화 entity culling"
            } else {
                "entity culling occlusion"
            }
        }
        Setting::EntityDistance => {
            if ko {
                "엔티티 거리 표시 범위 entity distance"
            } else {
                "entity distance"
            }
        }
        _ => "",
    }
}

fn option_tooltip(lang: &str, setting: Setting) -> (&'static str, Option<Impact>) {
    let ko = is_korean(lang);
    match setting {
        Setting::Distance => (
            if ko {
                "플레이어 주변에서 렌더링할 청크 반경을 조절합니다. 24청크를 초과하는 원거리 지형은 Vulkan LOD 파이프라인으로 고속 렌더링됩니다."
            } else {
                "Controls how far terrain is rendered around the player. Distances beyond 24 chunks use Vulkan distant terrain LOD."
            },
            Some(Impact::High),
        ),
        Setting::Brightness => (
            if ko {
                "어두운 동굴 및 야간 지형의 화면 밝기(감마)를 조절합니다. 성능에는 영향을 주지 않습니다."
            } else {
                "Controls the brightness (gamma) of dark surfaces and caves."
            },
            None,
        ),
        Setting::GuiScale => (
            if ko {
                "메뉴, 인벤토리 및 HUD 인터페이스의 배율을 설정합니다. '자동'은 창 크기에 맞춰 최적 배율을 선택합니다."
            } else {
                "Sets the scale factor used for menus, HUD, and inventory screens."
            },
            None,
        ),
        Setting::Fullscreen => (
            if ko {
                "테두리 없는 전체 화면 모드로 전환하여 입력 지연을 줄이고 몰입감을 높입니다."
            } else {
                "Toggles borderless fullscreen presentation on the active monitor."
            },
            None,
        ),
        Setting::Vsync => (
            if ko {
                "프레임 출력을 모니터 주사율에 동기화하여 화면 찢어짐(티어링)을 방지합니다. 끄면 프레임 부스트와 최소 입력 지연을 얻을 수 있습니다."
            } else {
                "Synchronizes frame presentation to monitor refresh rate. Disable for uncapped FPS and lowest input latency."
            },
            Some(Impact::Low),
        ),
        Setting::Fps => (
            if ko {
                "초당 최대 렌더링 프레임 수(FPS)를 제한합니다. 오른쪽 끝(무제한)으로 설정하면 제한 없이 최대 프레임을 출력합니다."
            } else {
                "Limits the maximum frame rate. Drag all the way right for Unlimited FPS."
            },
            None,
        ),
        Setting::Bobbing => (
            if ko {
                "걸어 다니거나 달릴 때 카메라 흔들림 효과를 활성화합니다."
            } else {
                "Controls camera bobbing motion while walking."
            },
            None,
        ),
        Setting::Anaglyph => (
            if ko {
                "적청 3D 안경용 애너글리프 입체 렌더링을 활성화합니다."
            } else {
                "Enables red-cyan 3D anaglyph stereoscopic rendering."
            },
            Some(Impact::Low),
        ),
        Setting::Graphics => (
            if ko {
                "나뭇잎 투명도, 비 효과 및 지형 셰이딩 품질을 제어합니다. '빠름'으로 설정하면 GPU 픽셀 부하가 크게 감소합니다."
            } else {
                "Controls leaf transparency and terrain shading quality. Fast reduces overdraw significantly."
            },
            Some(Impact::High),
        ),
        Setting::Clouds => (
            if ko {
                "하늘의 구름 렌더링 방식을 선택합니다(꺼짐 / 빠름 2D / 화려함 3D)."
            } else {
                "Controls cloud rendering quality (Off, Fast 2D, or Fancy 3D)."
            },
            Some(Impact::Low),
        ),
        Setting::Particles => (
            if ko {
                "화면에 동시에 표시되는 입자(파티클) 수를 조절합니다. 전투나 폭발 시 프레임 방어에 유용합니다."
            } else {
                "Controls the amount of visual particles spawned in the world."
            },
            Some(Impact::Medium),
        ),
        Setting::Lighting => (
            if ko {
                "블록 모서리의 부드러운 그림자(앰비언트 오클루전) 정밀도를 설정합니다."
            } else {
                "Controls smooth ambient occlusion shading across block faces."
            },
            Some(Impact::Medium),
        ),
        Setting::Mipmaps => (
            if ko {
                "원거리 블록 텍스처의 계단 현상과 자글거림을 완화하는 밉맵 필터링 단계를 설정합니다."
            } else {
                "Controls texture mipmap levels used to smooth distant block surfaces."
            },
            Some(Impact::Medium),
        ),
        Setting::Shadows => (
            if ko {
                "플레이어와 몹 발밑에 표시되는 원형 그림자를 렌더링합니다."
            } else {
                "Renders circular drop shadows beneath mobs and players."
            },
            Some(Impact::Low),
        ),
        Setting::Alternatives => (
            if ko {
                "지원되는 블록에 무작위 변형 모델을 사용하여 자연스러운 지형을 표현합니다."
            } else {
                "Uses randomized block model variants on natural terrain blocks."
            },
            Some(Impact::Low),
        ),
        Setting::Uploads => (
            if ko {
                "프레임당 GPU에 업로드할 최대 청크 메쉬 수입니다. 높이면 청크 로딩이 빨라지고, 낮추면 프레임 타임이 더 균일해집니다."
            } else {
                "Maximum chunk meshes uploaded to Vulkan per frame. Lower values keep frame times smoother."
            },
            Some(Impact::Medium),
        ),
        Setting::Greedy => (
            if ko {
                "인접한 동일 블록 면을 하나의 큰 폴리곤으로 병합하여 정점(삼각형) 수를 대폭 줄이고 FPS를 끌어올립니다."
            } else {
                "Merges adjacent coplanar block faces to dramatically reduce triangle count and boost FPS."
            },
            Some(Impact::High),
        ),
        Setting::Culling => (
            if ko {
                "지형 뒤에 완전히 가려져 보이지 않는 엔티티와 몹의 렌더링을 건너뛰어 멀티플레이 로비 및 기지 FPS를 크게 향상시킵니다."
            } else {
                "Skips rendering entities occluded behind solid terrain for major FPS gains in crowded areas."
            },
            Some(Impact::High),
        ),
        Setting::EntityDistance => (
            if ko {
                "엔티티와 플레이어가 화면에 렌더링되는 최대 거리(블록 단위)를 조절합니다."
            } else {
                "Controls the maximum distance in blocks at which entities are rendered."
            },
            Some(Impact::High),
        ),
        _ => ("", None),
    }
}

fn format_value(p: &Painter<'_>, s: &Settings, setting: Setting) -> String {
    let ko = is_korean(&p.s.language);
    match setting {
        Setting::Distance => {
            if s.render_distance > s.full_detail_distance() {
                if ko {
                    format!("{} 청크 (LOD)", s.render_distance)
                } else {
                    format!("{} Chunks (LOD)", s.render_distance)
                }
            } else if ko {
                format!("{} 청크", s.render_distance)
            } else {
                format!("{} Chunks", s.render_distance)
            }
        }
        Setting::Brightness => {
            let pct = (s.gamma * 100.0).round() as i32;
            if pct == 0 {
                p.tr("options.gamma.min")
            } else if pct == 100 {
                p.tr("options.gamma.max")
            } else {
                format!("{pct}%")
            }
        }
        Setting::GuiScale => {
            if s.gui_scale == 0 {
                p.tr("options.guiScale.auto")
            } else {
                format!("{}x", s.gui_scale)
            }
        }
        Setting::Fullscreen => String::new(),
        Setting::Vsync => String::new(),
        Setting::Fps => {
            if s.max_fps == 0 {
                p.tr("options.framerateLimit.max")
            } else {
                format!("{} FPS", s.max_fps)
            }
        }
        Setting::Bobbing | Setting::Anaglyph | Setting::Shadows | Setting::Alternatives => {
            String::new()
        }
        Setting::Graphics => p.tr(if s.fancy {
            "options.graphics.fancy"
        } else {
            "options.graphics.fast"
        }),
        Setting::Clouds => p.tr(
            [
                "options.off",
                "options.graphics.fast",
                "options.graphics.fancy",
            ][s.clouds.min(2) as usize],
        ),
        Setting::Particles => p.tr(
            [
                "options.particles.all",
                "options.particles.decreased",
                "options.particles.minimal",
            ][s.particles.min(2) as usize],
        ),
        Setting::Lighting => {
            p.tr(["options.ao.off", "options.ao.min", "options.ao.max"][s.smooth_lighting.min(2) as usize])
        }
        Setting::Mipmaps => {
            if s.mipmaps == 0 {
                p.tr("options.off")
            } else {
                format!("{}x", s.mipmaps)
            }
        }
        Setting::Uploads => s.chunk_uploads.to_string(),
        Setting::Greedy | Setting::Culling => String::new(),
        Setting::EntityDistance => format!("{} m", s.entity_distance),
        _ => String::new(),
    }
}

fn tick_checked(s: &Settings, setting: Setting) -> bool {
    match setting {
        Setting::Fullscreen => s.fullscreen,
        Setting::Vsync => s.vsync,
        Setting::Bobbing => s.bobbing,
        Setting::Anaglyph => s.anaglyph,
        Setting::Shadows => s.entity_shadows,
        Setting::Alternatives => s.alternate_blocks,
        Setting::Greedy => s.greedy_meshing,
        Setting::Culling => s.entity_culling,
        _ => false,
    }
}

fn slider_value(s: &Settings, setting: Setting) -> i32 {
    match setting {
        Setting::Distance => s.render_distance as i32,
        Setting::Brightness => (s.gamma * 100.0).round() as i32,
        Setting::Fps => {
            if s.max_fps == 0 || s.max_fps >= 260 {
                260
            } else {
                (s.max_fps as i32).clamp(10, 250)
            }
        }
        Setting::Mipmaps => s.mipmaps as i32,
        Setting::Uploads => s.chunk_uploads as i32,
        Setting::EntityDistance => s.entity_distance as i32,
        _ => 0,
    }
}

fn set_slider_from_fraction(s: &mut Settings, setting: Setting, t: f32) {
    let ControlKind::Slider { min, max, step } = control_kind(setting) else {
        return;
    };
    let raw = min as f32 + t.clamp(0.0, 1.0) * (max - min) as f32;
    let snapped = (((raw - min as f32) / step as f32).round() as i32 * step + min).clamp(min, max);
    match setting {
        Setting::Distance => s.render_distance = snapped as u32,
        Setting::Brightness => s.gamma = (snapped as f32 / 100.0).clamp(0.0, 1.0),
        Setting::Fps => s.max_fps = if snapped >= 260 { 0 } else { snapped as u32 },
        Setting::Mipmaps => s.mipmaps = snapped as u8,
        Setting::Uploads => s.chunk_uploads = snapped as u32,
        Setting::EntityDistance => s.entity_distance = snapped as u32,
        _ => {}
    }
}

fn copy_setting(dst: &mut Settings, src: &Settings, setting: Setting) {
    match setting {
        Setting::Graphics => dst.fancy = src.fancy,
        Setting::Distance => dst.render_distance = src.render_distance,
        Setting::Lighting => dst.smooth_lighting = src.smooth_lighting,
        Setting::Fps => dst.max_fps = src.max_fps,
        Setting::Anaglyph => dst.anaglyph = src.anaglyph,
        Setting::Bobbing => dst.bobbing = src.bobbing,
        Setting::GuiScale => dst.gui_scale = src.gui_scale,
        Setting::Brightness => dst.gamma = src.gamma,
        Setting::Clouds => dst.clouds = src.clouds,
        Setting::Particles => dst.particles = src.particles,
        Setting::Fullscreen => dst.fullscreen = src.fullscreen,
        Setting::Vsync => dst.vsync = src.vsync,
        Setting::Mipmaps => dst.mipmaps = src.mipmaps,
        Setting::Alternatives => dst.alternate_blocks = src.alternate_blocks,
        Setting::Shadows => dst.entity_shadows = src.entity_shadows,
        Setting::Culling => dst.entity_culling = src.entity_culling,
        Setting::EntityDistance => dst.entity_distance = src.entity_distance,
        Setting::Uploads => dst.chunk_uploads = src.chunk_uploads,
        Setting::Greedy => dst.greedy_meshing = src.greedy_meshing,
        _ => {}
    }
}

pub fn setting_differs(a: &Settings, b: &Settings, setting: Setting) -> bool {
    match setting {
        Setting::Graphics => a.fancy != b.fancy,
        Setting::Distance => a.render_distance != b.render_distance,
        Setting::Lighting => a.smooth_lighting != b.smooth_lighting,
        Setting::Fps => a.max_fps != b.max_fps,
        Setting::Anaglyph => a.anaglyph != b.anaglyph,
        Setting::Bobbing => a.bobbing != b.bobbing,
        Setting::GuiScale => a.gui_scale != b.gui_scale,
        Setting::Brightness => (a.gamma * 100.0).round() as i32 != (b.gamma * 100.0).round() as i32,
        Setting::Clouds => a.clouds != b.clouds,
        Setting::Particles => a.particles != b.particles,
        Setting::Fullscreen => a.fullscreen != b.fullscreen,
        Setting::Vsync => a.vsync != b.vsync,
        Setting::Mipmaps => a.mipmaps != b.mipmaps,
        Setting::Alternatives => a.alternate_blocks != b.alternate_blocks,
        Setting::Shadows => a.entity_shadows != b.entity_shadows,
        Setting::Culling => a.entity_culling != b.entity_culling,
        Setting::EntityDistance => a.entity_distance != b.entity_distance,
        Setting::Uploads => a.chunk_uploads != b.chunk_uploads,
        Setting::Greedy => a.greedy_meshing != b.greedy_meshing,
        _ => false,
    }
}

pub fn video_differs(a: &Settings, b: &Settings) -> bool {
    crate::settings::VIDEO
        .iter()
        .any(|&s| setting_differs(a, b, s))
}

fn matches_query(query: &str, lang: &str, setting: Setting) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    let hay = format!(
        "{} {}",
        option_name_plain(lang, setting),
        option_tooltip(lang, setting).0.to_lowercase()
    );
    q.split_whitespace().all(|term| hay.contains(term))
}

fn build_document(query: &str, lang: &str) -> (Vec<DocEntry>, f32) {
    let mut entries = Vec::with_capacity(32);
    let mut y = 0.0;
    entries.push(DocEntry {
        y,
        h: 18.0,
        kind: EntryKind::ModHeader,
    });
    y += 18.0;

    for (page_idx, (_, _, groups)) in PAGES.iter().enumerate() {
        let mut page_has_match = false;
        for group in *groups {
            if group.iter().any(|&s| matches_query(query, lang, s)) {
                page_has_match = true;
                break;
            }
        }
        if !page_has_match {
            continue;
        }
        y += 6.0;
        entries.push(DocEntry {
            y,
            h: 18.0,
            kind: EntryKind::PageHeader(page_idx),
        });
        y += 20.0;

        let mut first_group = true;
        for group in *groups {
            let matched: Vec<Setting> = group
                .iter()
                .copied()
                .filter(|&s| matches_query(query, lang, s))
                .collect();
            if matched.is_empty() {
                continue;
            }
            if !first_group {
                y += 4.0;
            }
            first_group = false;
            for (i, setting) in matched.into_iter().enumerate() {
                if i > 0 {
                    y += 1.0;
                }
                entries.push(DocEntry {
                    y,
                    h: 18.0,
                    kind: EntryKind::Option {
                        page: page_idx,
                        setting,
                    },
                });
                y += 18.0;
            }
        }
    }
    (entries, y)
}

fn effective_scroll(gui: &Gui, max_scroll: f32) -> f32 {
    let raw = if gui.video.scroll > 0.0 {
        gui.video.scroll
    } else if gui.scroll > 0 {
        gui.scroll as f32 * 55.0
    } else {
        0.0
    };
    raw.clamp(0.0, max_scroll)
}

fn clip_vertices(p: &mut Painter<'_>, start: usize, clip: Rect) {
    let batch = p.frame.vertices.split_off(start);
    let [sw, sh] = p.frame.size;
    for tri in batch.chunks_exact(3) {
        let mut verts: Vec<UiVertex> = tri
            .iter()
            .map(|v| {
                let mut v = *v;
                v.position = [
                    (v.position[0] + 1.0) * sw * 0.5,
                    (1.0 - v.position[1]) * sh * 0.5,
                ];
                v
            })
            .collect();
        for (axis, edge, high) in [
            (0, clip.x, false),
            (1, clip.y, false),
            (0, clip.x + clip.w, true),
            (1, clip.y + clip.h, true),
        ] {
            if verts.is_empty() {
                break;
            }
            let mut out = Vec::with_capacity(verts.len() + 1);
            let mut prev = *verts.last().unwrap();
            let inside = |v: UiVertex| {
                if high {
                    v.position[axis] <= edge
                } else {
                    v.position[axis] >= edge
                }
            };
            for cur in verts {
                if inside(cur) != inside(prev) {
                    let denom = cur.position[axis] - prev.position[axis];
                    let t = if denom.abs() > 1e-6 {
                        ((edge - prev.position[axis]) / denom).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let mut cut = prev;
                    for i in 0..2 {
                        cut.position[i] += t * (cur.position[i] - prev.position[i]);
                        cut.uv[i] += t * (cur.uv[i] - prev.uv[i]);
                    }
                    for i in 0..4 {
                        cut.color[i] += t * (cur.color[i] - prev.color[i]);
                    }
                    out.push(cut);
                }
                if inside(cur) {
                    out.push(cur);
                }
                prev = cur;
            }
            verts = out;
        }
        for v in &mut verts {
            v.position = [v.position[0] * 2.0 / sw - 1.0, 1.0 - v.position[1] * 2.0 / sh];
        }
        for i in 2..verts.len() {
            p.frame
                .vertices
                .extend_from_slice(&[verts[0], verts[i - 1], verts[i]]);
        }
    }
}

fn draw_border(p: &mut Painter<'_>, r: Rect, color: [f32; 4]) {
    p.rect(r.x, r.y, r.w, 1.0, color);
    p.rect(r.x, r.y + r.h - 1.0, r.w, 1.0, color);
    p.rect(r.x, r.y + 1.0, 1.0, (r.h - 2.0).max(0.0), color);
    p.rect(r.x + r.w - 1.0, r.y + 1.0, 1.0, (r.h - 2.0).max(0.0), color);
}

fn draw_flat_button(
    p: &mut Painter<'_>,
    rect: Rect,
    label: &str,
    enabled: bool,
    accent_underline: Option<(usize, usize)>,
) {
    let hovered = enabled && rect.contains(p.mouse);
    p.rect(
        rect.x,
        rect.y,
        rect.w,
        rect.h,
        if !enabled {
            BG_LIGHT
        } else if hovered {
            BG_HOVER
        } else {
            BG_DEFAULT
        },
    );
    if hovered {
        draw_border(p, rect, BUTTON_BORDER);
    }
    let text_color = if enabled {
        FOREGROUND
    } else {
        FOREGROUND_DISABLED
    };
    let tx = (rect.x + (rect.w - p.width(label)) * 0.5).floor();
    let ty = (rect.y + (rect.h - 8.0) * 0.5).floor();
    p.text(tx, ty, label, text_color);
    if let Some((start, end)) = accent_underline.filter(|_| enabled) {
        if end <= label.len() && start < end {
            let x1 = tx + p.width(&label[..start]);
            let w = p.width(&label[start..end]);
            p.rect(x1, ty + 9.0, w, 1.0, THEME_LIGHTER);
        }
    }
}

fn wrap_lines(p: &Painter<'_>, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    for para in text.split('\n') {
        let mut current = String::new();
        for word in para.split_whitespace() {
            let candidate = if current.is_empty() {
                word.to_owned()
            } else {
                format!("{current} {word}")
            };
            if p.width(&candidate) <= max_w || current.is_empty() {
                current = candidate;
            } else {
                lines.push(current);
                current = word.to_owned();
            }
        }
        if !current.is_empty() {
            lines.push(current);
        }
    }
    lines
}

fn truncate_text(p: &Painter<'_>, text: &str, max_w: f32) -> String {
    if p.width(text) <= max_w {
        return text.to_owned();
    }
    let ellipsis = "...";
    let budget = (max_w - p.width(ellipsis)).max(0.0);
    let mut out = String::new();
    for ch in text.chars() {
        let mut next = out.clone();
        next.push(ch);
        if p.width(&next) > budget {
            break;
        }
        out = next;
    }
    format!("{out}{ellipsis}")
}

pub(super) fn draw(
    p: &mut Painter<'_>,
    gui: &Gui,
    in_world: bool,
) {
    let shift_held = gui.video.shift_held;
    let alt_held = gui.video.alt_held;
    let [sw, sh] = p.frame.size;
    if in_world {
        super::pause::background(p);
    } else {
        p.background();
        p.rect(0.0, 0.0, sw, sh, BG_MEDIUM);
    }

    let ko = is_korean(&p.s.language);
    let active = gui.video.effective(p.s);
    let defaults = Settings::default();
    let layout = compute_layout([sw, sh], gui.video.hide_donate);

    // 1. Top Search Bar + Donation Button
    let search_hovered = layout.search_rect.contains(p.mouse);
    p.rect(
        layout.search_rect.x,
        layout.search_rect.y,
        layout.search_rect.w,
        layout.search_rect.h,
        if gui.video.search_focused || search_hovered {
            BG_HOVER
        } else {
            BG_DEFAULT
        },
    );
    if gui.video.search_focused {
        draw_border(p, layout.search_rect, THEME);
    }
    let lens_x = layout.search_rect.x + 6.0;
    let lens_y = layout.search_rect.y + 6.0;
    p.text(lens_x, lens_y, "⌕", THEME_DARKER);
    let text_x = layout.search_rect.x + 18.0;
    let text_y = layout.search_rect.y + 6.0;
    if gui.video.search.value.is_empty() && !gui.video.search_focused {
        let placeholder = if ko {
            "옵션 검색... (Ctrl+F)"
        } else {
            "Search options... (Ctrl+F)"
        };
        p.text(
            text_x,
            text_y,
            &truncate_text(p, placeholder, layout.search_rect.w - 24.0),
            FOREGROUND_DISABLED,
        );
    } else {
        let shown = truncate_text(p, &gui.video.search.value, layout.search_rect.w - 28.0);
        let end_x = p.text_end(text_x, text_y, &shown, FOREGROUND);
        if gui.video.search_focused && ((p.time * 2.0) as i32 % 2 == 0) {
            p.rect(end_x + 1.0, text_y - 1.0, 1.0, 10.0, THEME_LIGHTER);
        }
    }

    if let (Some(don_r), Some(hide_r)) = (layout.donate_rect, layout.hide_donate_rect) {
        draw_flat_button(
            p,
            don_r,
            if ko {
                "Buy us a coffee!"
            } else {
                "Buy us a coffee!"
            },
            true,
            None,
        );
        draw_flat_button(p, hide_r, "×", true, None);
    }

    // 2. Left Sidebar (PageListWidget)
    super::main_menu::gradient(p, layout.page_rect, BG_LIGHT, BG_DEFAULT);
    let sidebar_clip_start = p.frame.vertices.len();
    let hdr_x = layout.page_rect.x;
    let hdr_y = layout.page_rect.y;
    p.image(
        "sodium/config_icon",
        Rect {
            x: hdr_x + 4.0,
            y: hdr_y + 4.0,
            w: 19.0,
            h: 19.0,
        },
        [0.0, 0.0, 38.0, 38.0],
        THEME_LIGHTER,
    );
    p.text(hdr_x + 27.0, hdr_y + 5.0, "Sodium", FOREGROUND);
    p.text(hdr_x + 27.0, hdr_y + 15.0, "0.9.3", THEME_LIGHTER);
    p.rect(
        hdr_x + 4.0,
        hdr_y + 26.0,
        layout.page_rect.w - 8.0,
        1.0,
        argb(0x30FFFFFF),
    );

    let (entries, doc_h) = build_document(&gui.video.search.value, &p.s.language);
    let max_scroll = (doc_h - layout.list_inner.h).max(0.0);
    let scroll = effective_scroll(gui, max_scroll);

    // Sync active page indicator from scroll position when user hasn't filtered to empty
    let active_page = entries
        .iter()
        .filter_map(|e| match e.kind {
            EntryKind::PageHeader(idx) if e.y <= scroll + 28.0 => Some(idx),
            _ => None,
        })
        .last()
        .unwrap_or(gui.video.selected_page.min(PAGES.len() - 1));

    for (idx, (en_title, ko_title, groups)) in PAGES.iter().enumerate() {
        let ry = hdr_y + 30.0 + idx as f32 * 19.0;
        let row = Rect {
            x: hdr_x,
            y: ry,
            w: layout.page_rect.w,
            h: 18.0,
        };
        let selected = idx == active_page;
        let hovered = row.contains(p.mouse);
        if selected || hovered {
            p.rect(
                row.x,
                row.y,
                row.w,
                row.h,
                if hovered { BG_HOVER } else { BG_DEFAULT },
            );
        }
        if selected {
            p.rect(row.x + row.w - 3.0, row.y, 3.0, row.h, THEME_LIGHTER);
        }
        let page_dirty = groups
            .iter()
            .flat_map(|g| g.iter())
            .any(|&s| setting_differs(p.s, active, s));
        let base_title = if ko { *ko_title } else { *en_title };
        let title = if page_dirty {
            format!("§o{base_title} *")
        } else {
            base_title.to_owned()
        };
        p.text(
            row.x + 8.0,
            row.y + 5.0,
            &truncate_text(p, &title, row.w - 14.0),
            if selected { THEME_LIGHTER } else { FOREGROUND },
        );
    }
    clip_vertices(p, sidebar_clip_start, layout.page_rect);

    // 3. Center OptionListWidget
    let has_scrollbar = max_scroll > 0.5;
    let row_w = layout.list_inner.w - if has_scrollbar { 9.0 } else { 0.0 };
    let row_x = layout.list_inner.x;
    let list_clip_start = p.frame.vertices.len();
    let mut hovered_option: Option<(Setting, Rect)> = None;

    for entry in &entries {
        let sy = layout.list_inner.y + entry.y - scroll;
        if sy + entry.h <= layout.list_inner.y || sy >= layout.list_inner.y + layout.list_inner.h {
            continue;
        }
        let row = Rect {
            x: row_x,
            y: sy,
            w: row_w,
            h: entry.h,
        };
        let row_hovered = layout.list_inner.contains(p.mouse) && row.contains(p.mouse);

        match entry.kind {
            EntryKind::ModHeader => {
                p.rect(row.x, row.y, row.w, row.h, BG_DARKER);
                p.text(row.x + 6.0, row.y + 5.0, "§lSodium", FOREGROUND);
                let version_tag = "Vulkan 1.8.9";
                p.text(
                    row.x + row.w - p.width(version_tag) - if shift_held && row_hovered { 24.0 } else { 6.0 },
                    row.y + 5.0,
                    version_tag,
                    THEME_DARKER,
                );
                if shift_held && row_hovered {
                    draw_reset_badge(p, row, video_differs(&defaults, active));
                }
            }
            EntryKind::PageHeader(page_idx) => {
                p.rect(row.x, row.y, row.w, row.h, BG_DEFAULT);
                let (en_t, ko_t, groups) = PAGES[page_idx];
                let header = format!("◆ {}", if ko { ko_t } else { en_t });
                p.text(row.x + 6.0, row.y + 5.0, &header, THEME);
                if shift_held && row_hovered {
                    let can_reset = groups
                        .iter()
                        .flat_map(|g| g.iter())
                        .any(|&s| setting_differs(&defaults, active, s));
                    draw_reset_badge(p, row, can_reset);
                }
            }
            EntryKind::Option { setting, .. } => {
                let dragging_this = gui.video.drag_slider.is_some_and(|(s, _)| s == setting);
                p.rect(
                    row.x,
                    row.y,
                    row.w,
                    row.h,
                    if row_hovered || dragging_this {
                        BG_HOVER
                    } else {
                        BG_DEFAULT
                    },
                );
                if row_hovered || dragging_this {
                    hovered_option = Some((setting, row));
                }
                let modified = setting_differs(p.s, active, setting);
                let base_name = option_name(p, setting);
                let show_reset = shift_held && row_hovered;
                let right_limit = row.x + row.w - if show_reset { 22.0 } else { 0.0 };

                let kind = control_kind(setting);
                let value_str = format_value(p, active, setting);
                let show_slider_bar = matches!(kind, ControlKind::Slider { .. })
                    && (row_hovered || dragging_this)
                    && !show_reset;

                let control_reserve = match kind {
                    ControlKind::TickBox => 22.0,
                    ControlKind::Cycling => p.width(&value_str) + 14.0,
                    ControlKind::Slider { .. } if show_slider_bar => {
                        let bar_w = (row.w * 0.28).clamp(44.0, 62.0);
                        bar_w + p.width(&value_str) + 18.0
                    }
                    ControlKind::Slider { .. } => p.width(&value_str) + 14.0,
                };
                let label_budget = (right_limit - (row.x + 6.0) - control_reserve).max(32.0);
                let decorated = if modified {
                    format!(
                        "§o{} *",
                        truncate_text(p, &base_name, (label_budget - 10.0).max(20.0))
                    )
                } else {
                    truncate_text(p, &base_name, label_budget)
                };
                p.text(row.x + 6.0, row.y + 5.0, &decorated, FOREGROUND);

                match kind {
                    ControlKind::TickBox => {
                        let bx = right_limit - 16.0;
                        let by = row.y + 4.0;
                        let box_r = Rect {
                            x: bx,
                            y: by,
                            w: 10.0,
                            h: 10.0,
                        };
                        draw_border(p, box_r, if row_hovered { THEME_LIGHTER } else { THEME });
                        if tick_checked(active, setting) {
                            p.rect(bx + 2.0, by + 2.0, 6.0, 6.0, THEME);
                        }
                    }
                    ControlKind::Cycling => {
                        let vx = right_limit - 6.0 - p.width(&value_str);
                        p.text(
                            vx,
                            row.y + 5.0,
                            &value_str,
                            if row_hovered || modified {
                                THEME
                            } else {
                                FOREGROUND
                            },
                        );
                    }
                    ControlKind::Slider { min, max, .. } => {
                        if show_slider_bar {
                            let bar_w = (row.w * 0.28).clamp(44.0, 62.0);
                            let bar_x = right_limit - 6.0 - bar_w;
                            let bar_y = row.y + 8.0;
                            p.rect(bar_x, bar_y, bar_w, 2.0, argb(0x70FFFFFF));
                            let cur = slider_value(active, setting);
                            let frac = ((cur - min) as f32 / (max - min).max(1) as f32)
                                .clamp(0.0, 1.0);
                            p.rect(bar_x, bar_y, bar_w * frac, 2.0, THEME);
                            let thumb_x = (bar_x + frac * (bar_w - 2.0)).round();
                            p.rect(thumb_x, row.y + 4.0, 2.0, 10.0, THEME_LIGHTER);
                            let vx = bar_x - 5.0 - p.width(&value_str);
                            p.text(vx, row.y + 5.0, &value_str, THEME_LIGHTER);
                        } else {
                            let vx = right_limit - 6.0 - p.width(&value_str);
                            p.text(
                                vx,
                                row.y + 5.0,
                                &value_str,
                                if modified { THEME } else { FOREGROUND },
                            );
                        }
                    }
                }

                if show_reset {
                    draw_reset_badge(p, row, setting_differs(&defaults, active, setting));
                }

                let vis_y0 = row.y.max(layout.list_inner.y);
                let vis_y1 = (row.y + row.h).min(layout.list_inner.y + layout.list_inner.h);
                if vis_y1 - vis_y0 >= 3.0 {
                    p.frame.buttons.push(Button {
                        rect: Rect {
                            x: row.x.max(0.0),
                            y: vis_y0.max(0.0),
                            w: row.w.min(sw - row.x.max(0.0)),
                            h: (vis_y1 - vis_y0).min(sh - vis_y0.max(0.0)),
                        },
                        action: Action::Setting(setting),
                        enabled: true,
                    });
                }
            }
        }
    }
    clip_vertices(p, list_clip_start, layout.list_inner);

    // Scrollbar on right of OptionListWidget
    if has_scrollbar {
        let track_x = layout.list_inner.x + layout.list_inner.w - 6.0;
        let track_y = layout.list_inner.y;
        let track_h = layout.list_inner.h;
        p.rect(track_x, track_y, 5.0, track_h, SCROLL_TRACK);
        let thumb_h = ((track_h / doc_h) * track_h).clamp(18.0, track_h);
        let thumb_y = track_y + (scroll / max_scroll) * (track_h - thumb_h);
        p.rect(
            track_x,
            thumb_y,
            5.0,
            thumb_h,
            if gui.video.drag_scrollbar.is_some() {
                THEME
            } else {
                SCROLL_THUMB
            },
        );
    }

    // 4. Bottom-Right Action Buttons: Undo, Apply, Done
    let has_changes = gui.video.has_changes(p.s);
    let undo_label = if ko { "되돌리기" } else { "Undo" };
    let apply_label = if ko { "적용" } else { "Apply" };
    let done_label = p.tr("gui.done");
    draw_flat_button(
        p,
        layout.undo_rect,
        undo_label,
        has_changes,
        if alt_held && !ko { Some((0, 1)) } else { None },
    );
    draw_flat_button(
        p,
        layout.apply_rect,
        apply_label,
        has_changes,
        if alt_held && !ko { Some((0, 1)) } else { None },
    );
    draw_flat_button(
        p,
        layout.done_rect,
        &done_label,
        !has_changes,
        if alt_held && done_label.starts_with('D') {
            Some((0, 1))
        } else {
            None
        },
    );
    if !has_changes {
        p.frame.buttons.push(Button {
            rect: layout.done_rect,
            action: Action::Screen(Screen::Options),
            enabled: true,
        });
    }

    // 5. Right-Column ScrollableTooltip (or compact overlay on narrow viewports)
    if let Some((setting, row_rect)) = hovered_option {
        draw_option_tooltip(p, gui, &layout, setting, row_rect, ko);
    }
}

fn draw_reset_badge(p: &mut Painter<'_>, row: Rect, enabled: bool) {
    let rx = row.x + row.w - 20.0;
    let badge = Rect {
        x: rx,
        y: row.y,
        w: 20.0,
        h: row.h,
    };
    p.rect(
        badge.x,
        badge.y,
        badge.w,
        badge.h,
        if enabled && badge.contains(p.mouse) {
            argb(0x60FF8C30)
        } else {
            BG_DARKER
        },
    );
    p.image(
        "sodium/reset_button",
        Rect {
            x: badge.x + 5.0,
            y: badge.y + 4.0,
            w: 10.0,
            h: 10.0,
        },
        [0.0, 0.0, 10.0, 10.0],
        if enabled {
            RESET_ORANGE
        } else {
            FOREGROUND_DISABLED
        },
    );
}

fn draw_option_tooltip(
    p: &mut Painter<'_>,
    gui: &Gui,
    layout: &Layout,
    setting: Setting,
    row_rect: Rect,
    ko: bool,
) {
    let (desc, impact) = option_tooltip(&p.s.language, setting);
    if desc.is_empty() {
        return;
    }

    if let Some(pane) = layout.tooltip_pane {
        let mut lines = wrap_lines(p, desc, (pane.w - 14.0).max(40.0));
        let impact_line = impact.map(|imp| {
            let (code, en_l, ko_l) = match imp {
                Impact::Low => ("§a", "Low", "낮음"),
                Impact::Medium => ("§e", "Medium", "보통"),
                Impact::High => ("§6", "High", "높음"),
            };
            if ko {
                format!("§7성능 영향: {code}{ko_l}")
            } else {
                format!("§7Performance Impact: {code}{en_l}")
            }
        });
        let content_h =
            lines.len() as f32 * 11.0 + if impact_line.is_some() { 16.0 } else { 0.0 } + 12.0;
        let box_h = content_h.min(pane.h);
        let ideal_y = (row_rect.y - 4.0).clamp(pane.y, (pane.y + pane.h - box_h).max(pane.y));
        let box_rect = Rect {
            x: pane.x,
            y: ideal_y,
            w: pane.w,
            h: box_h,
        };
        p.rect(box_rect.x, box_rect.y, box_rect.w, box_rect.h, BG_DEFAULT);
        draw_border(p, box_rect, argb(0x3894E4D3));

        // 5x9 left-pointing tooltip arrow (fill + border) aligned to hovered row center
        let arrow_y = (row_rect.y + 4.0).clamp(box_rect.y + 3.0, box_rect.y + box_rect.h - 12.0);
        let arrow_rect = Rect {
            x: box_rect.x - 5.0,
            y: arrow_y,
            w: 5.0,
            h: 9.0,
        };
        p.image(
            "sodium/tooltip_arrows",
            arrow_rect,
            [5.0, 0.0, 5.0, 9.0],
            BG_DEFAULT,
        );
        p.image(
            "sodium/tooltip_arrows",
            arrow_rect,
            [0.0, 0.0, 5.0, 9.0],
            argb(0x3894E4D3),
        );

        let clip_start = p.frame.vertices.len();
        let max_tscroll = (content_h - box_h).max(0.0);
        let tscroll = gui.video.tooltip_scroll.clamp(0.0, max_tscroll);
        let mut ty = box_rect.y + 6.0 - tscroll;
        for line in lines.drain(..) {
            p.text(box_rect.x + 7.0, ty, &line, FOREGROUND);
            ty += 11.0;
        }
        if let Some(imp) = impact_line {
            ty += 4.0;
            p.text(box_rect.x + 7.0, ty, &imp, FOREGROUND);
        }
        clip_vertices(p, clip_start, box_rect);
    } else {
        // Compact floating overlay on narrow viewports (e.g. 320x240)
        let [sw, sh] = p.frame.size;
        let box_w = (sw - 16.0).min(210.0);
        let lines = wrap_lines(p, desc, box_w - 10.0);
        let box_h = (lines.len() as f32 * 10.0 + 8.0).min(68.0);
        let bx = (row_rect.x + 4.0).clamp(4.0, (sw - box_w - 4.0).max(4.0));
        let by = if row_rect.y + row_rect.h + box_h + 4.0 <= sh {
            row_rect.y + row_rect.h + 2.0
        } else {
            (row_rect.y - box_h - 2.0).max(4.0)
        };
        p.rect(bx, by, box_w, box_h, BG_OVERLAY);
        draw_border(p, Rect { x: bx, y: by, w: box_w, h: box_h }, THEME_DARKER);
        let clip_start = p.frame.vertices.len();
        for (i, line) in lines.iter().enumerate() {
            p.text(bx + 5.0, by + 4.0 + i as f32 * 10.0, line, FOREGROUND);
        }
        clip_vertices(
            p,
            clip_start,
            Rect {
                x: bx,
                y: by,
                w: box_w,
                h: box_h,
            },
        );
    }
}

pub fn pointer_down(
    gui: &mut Gui,
    committed: &Settings,
    size: [f32; 2],
    button: MouseButton,
    shift_held: bool,
) -> Effect {
    if !matches!(button, MouseButton::Left | MouseButton::Right) {
        return Effect::None;
    }
    let right = button == MouseButton::Right;
    let cursor = gui.cursor;
    let layout = compute_layout(size, gui.video.hide_donate);

    // Search bar focus
    if layout.search_rect.contains(cursor) {
        gui.video.search_focused = true;
        if right {
            gui.video.search = TextField::default();
            gui.video.scroll = 0.0;
        }
        return Effect::PlayClick;
    }
    gui.video.search_focused = false;

    if !right {
        if let Some(hide_r) = layout.hide_donate_rect.filter(|r| r.contains(cursor)) {
            let _ = hide_r;
            gui.video.hide_donate = true;
            return Effect::PlayClick;
        }
        if layout.donate_rect.is_some_and(|r| r.contains(cursor)) {
            return Effect::OpenUrl(DONATE_URL);
        }

        let has_changes = gui.video.has_changes(committed);
        if layout.undo_rect.contains(cursor) {
            if has_changes {
                gui.video.pending = None;
                return Effect::PlayClick;
            }
            return Effect::None;
        }
        if layout.apply_rect.contains(cursor) {
            if let Some(staged) = gui.video.pending.take() {
                if video_differs(committed, &staged) {
                    return Effect::Apply(Box::new(staged));
                }
            }
            return Effect::None;
        }
        if layout.done_rect.contains(cursor) {
            if !has_changes {
                return Effect::Close;
            }
            return Effect::None;
        }

        // Sidebar page click
        for idx in 0..PAGES.len() {
            let row = Rect {
                x: layout.page_rect.x,
                y: layout.page_rect.y + 30.0 + idx as f32 * 19.0,
                w: layout.page_rect.w,
                h: 18.0,
            };
            if layout.page_rect.contains(cursor) && row.contains(cursor) {
                gui.video.selected_page = idx;
                gui.video.search = TextField::default();
                let (entries, doc_h) = build_document("", &committed.language);
                let max_scroll = (doc_h - layout.list_inner.h).max(0.0);
                if let Some(hdr) = entries
                    .iter()
                    .find(|e| matches!(e.kind, EntryKind::PageHeader(p) if p == idx))
                {
                    gui.video.scroll = if idx == 0 {
                        0.0
                    } else {
                        hdr.y.clamp(0.0, max_scroll)
                    };
                }
                gui.scroll = 0;
                return Effect::PlayClick;
            }
        }
    }

    let (entries, doc_h) = build_document(&gui.video.search.value, &committed.language);
    let max_scroll = (doc_h - layout.list_inner.h).max(0.0);
    let scroll = effective_scroll(gui, max_scroll);
    gui.video.scroll = scroll;
    gui.scroll = 0;

    // Scrollbar click/drag
    if !right && max_scroll > 0.5 {
        let track_x = layout.list_inner.x + layout.list_inner.w - 7.0;
        let track = Rect {
            x: track_x,
            y: layout.list_inner.y,
            w: 7.0,
            h: layout.list_inner.h,
        };
        if track.contains(cursor) {
            let thumb_h = ((track.h / doc_h) * track.h).clamp(18.0, track.h);
            let frac = ((cursor[1] - track.y - thumb_h * 0.5) / (track.h - thumb_h).max(1.0))
                .clamp(0.0, 1.0);
            gui.video.scroll = frac * max_scroll;
            gui.video.drag_scrollbar = Some((cursor[1], gui.video.scroll));
            return Effect::None;
        }
    }

    if !layout.list_inner.contains(cursor) {
        return Effect::None;
    }

    let row_w = layout.list_inner.w - if max_scroll > 0.5 { 9.0 } else { 0.0 };
    let defaults = Settings::default();
    for entry in &entries {
        let sy = layout.list_inner.y + entry.y - scroll;
        let row = Rect {
            x: layout.list_inner.x,
            y: sy,
            w: row_w,
            h: entry.h,
        };
        if !row.contains(cursor) {
            continue;
        }
        let on_reset = !right && shift_held && cursor[0] >= row.x + row.w - 20.0;
        match entry.kind {
            EntryKind::ModHeader if on_reset => {
                gui.video.mutate(committed, |s| {
                    for &opt in &crate::settings::VIDEO {
                        copy_setting(s, &defaults, opt);
                    }
                });
                return Effect::PlayClick;
            }
            EntryKind::PageHeader(page_idx) if on_reset => {
                gui.video.mutate(committed, |s| {
                    for group in PAGES[page_idx].2 {
                        for &opt in *group {
                            copy_setting(s, &defaults, opt);
                        }
                    }
                });
                return Effect::PlayClick;
            }
            EntryKind::Option { setting, .. } => {
                gui.video.last_hovered = Some(setting);
                if on_reset {
                    gui.video.mutate(committed, |s| {
                        copy_setting(s, &defaults, setting);
                    });
                    return Effect::PlayClick;
                }
                if !right {
                    if let ControlKind::Slider { .. } = control_kind(setting) {
                        let bar_w = (row.w * 0.28).clamp(44.0, 62.0);
                        let bar_x = row.x + row.w - 6.0 - bar_w;
                        let slider_hit = Rect {
                            x: bar_x - 8.0,
                            y: row.y,
                            w: bar_w + 14.0,
                            h: row.h,
                        };
                        if slider_hit.contains(cursor) {
                            let track = Rect {
                                x: bar_x,
                                y: row.y,
                                w: bar_w,
                                h: row.h,
                            };
                            let frac = ((cursor[0] - track.x) / track.w.max(1.0)).clamp(0.0, 1.0);
                            gui.video.mutate(committed, |s| {
                                set_slider_from_fraction(s, setting, frac);
                            });
                            gui.video.drag_slider = Some((setting, track));
                            return Effect::PlayClick;
                        }
                    }
                }
                gui.video.mutate(committed, |s| s.cycle(setting, right));
                return Effect::PlayClick;
            }
            _ => {}
        }
    }
    Effect::None
}

pub fn pointer_move(gui: &mut Gui, committed: &Settings, size: [f32; 2]) {
    let cursor = gui.cursor;
    let layout = compute_layout(size, gui.video.hide_donate);

    if let Some((setting, track)) = gui.video.drag_slider {
        let frac = ((cursor[0] - track.x) / track.w.max(1.0)).clamp(0.0, 1.0);
        gui.video.mutate(committed, |s| {
            set_slider_from_fraction(s, setting, frac);
        });
        return;
    }

    if let Some((start_y, start_scroll)) = gui.video.drag_scrollbar {
        let (_, doc_h) = build_document(&gui.video.search.value, &committed.language);
        let max_scroll = (doc_h - layout.list_inner.h).max(0.0);
        if max_scroll > 0.5 {
            let track_h = layout.list_inner.h;
            let thumb_h = ((track_h / doc_h) * track_h).clamp(18.0, track_h);
            let dy = cursor[1] - start_y;
            let delta = dy * (max_scroll / (track_h - thumb_h).max(1.0));
            gui.video.scroll = (start_scroll + delta).clamp(0.0, max_scroll);
            gui.scroll = 0;
        }
        return;
    }

    let (entries, doc_h) = build_document(&gui.video.search.value, &committed.language);
    let max_scroll = (doc_h - layout.list_inner.h).max(0.0);
    let scroll = effective_scroll(gui, max_scroll);
    let row_w = layout.list_inner.w - if max_scroll > 0.5 { 9.0 } else { 0.0 };
    if layout.list_inner.contains(cursor) {
        for entry in &entries {
            if let EntryKind::Option { setting, .. } = entry.kind {
                let row = Rect {
                    x: layout.list_inner.x,
                    y: layout.list_inner.y + entry.y - scroll,
                    w: row_w,
                    h: entry.h,
                };
                if row.contains(cursor) {
                    if gui.video.last_hovered != Some(setting) {
                        gui.video.last_hovered = Some(setting);
                        gui.video.tooltip_scroll = 0.0;
                    }
                    break;
                }
            }
        }
    }
}

pub fn wheel(gui: &mut Gui, committed: &Settings, size: [f32; 2], y: f32, ctrl: bool) {
    if ctrl {
        let reverse = y < 0.0;
        gui.video.mutate(committed, |s| s.cycle(Setting::GuiScale, reverse));
        return;
    }
    let layout = compute_layout(size, gui.video.hide_donate);
    if layout
        .tooltip_pane
        .is_some_and(|pane| pane.contains(gui.cursor))
    {
        gui.video.tooltip_scroll = (gui.video.tooltip_scroll - y.signum() * 14.0).max(0.0);
        return;
    }
    let (_, doc_h) = build_document(&gui.video.search.value, &committed.language);
    let max_scroll = (doc_h - layout.list_inner.h).max(0.0);
    let current = effective_scroll(gui, max_scroll);
    gui.video.scroll = (current - y.signum() * 24.0).clamp(0.0, max_scroll);
    gui.scroll = 0;
}

pub fn key_down(
    gui: &mut Gui,
    committed: &Settings,
    key: KeyCode,
    shift: bool,
    ctrl: bool,
    alt: bool,
) -> Effect {
    if ctrl && key == KeyCode::KeyF {
        gui.video.search_focused = true;
        return Effect::None;
    }
    if alt {
        match key {
            KeyCode::KeyU if gui.video.has_changes(committed) => {
                gui.video.pending = None;
                return Effect::PlayClick;
            }
            KeyCode::KeyA if gui.video.has_changes(committed) => {
                if let Some(staged) = gui.video.pending.take() {
                    return Effect::Apply(Box::new(staged));
                }
            }
            KeyCode::KeyD if !gui.video.has_changes(committed) => {
                return Effect::Close;
            }
            _ => {}
        }
    }
    if key == KeyCode::Escape {
        if gui.video.search_focused || !gui.video.search.value.is_empty() {
            gui.video.search = TextField::default();
            gui.video.search_focused = false;
            gui.video.scroll = 0.0;
            return Effect::None;
        }
        gui.video.pending = None;
        return Effect::Close;
    }
    if gui.video.search_focused {
        if matches!(key, KeyCode::Enter | KeyCode::NumpadEnter) {
            gui.video.search_focused = false;
            return Effect::None;
        }
        let before = gui.video.search.value.clone();
        gui.video.search.key(key, shift, ctrl, 64);
        if gui.video.search.value != before {
            gui.video.scroll = 0.0;
            gui.scroll = 0;
        }
    }
    Effect::None
}

pub fn text_input(gui: &mut Gui, text: &str) {
    if gui.video.search_focused {
        let before = gui.video.search.value.clone();
        gui.video.search.insert(text, 64);
        if gui.video.search.value != before {
            gui.video.scroll = 0.0;
            gui.scroll = 0;
        }
    }
}
