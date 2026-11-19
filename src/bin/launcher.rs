#![cfg_attr(not(test), windows_subsystem = "windows")]

#[path = "../ms_auth.rs"]
mod ms_auth;
#[path = "../resource_dump.rs"]
mod resource_dump;

use ms_auth::{AccountStore, AuthenticatedAccount, DeviceCodePrompt, PollOutcome};
use std::{
    ffi::c_void,
    path::PathBuf,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, Sender},
        Arc, Mutex, OnceLock,
    },
    time::{Duration, Instant},
};

const WM_DESTROY: u32 = 0x0002;
const WM_PAINT: u32 = 0x000F;
const WM_ERASEBKGND: u32 = 0x0014;
const WM_SETCURSOR: u32 = 0x0020;
const WM_MOUSEMOVE: u32 = 0x0200;
const WM_LBUTTONDOWN: u32 = 0x0201;
const WM_LBUTTONUP: u32 = 0x0202;
const WM_MOUSEWHEEL: u32 = 0x020A;
const WM_TIMER: u32 = 0x0113;
const WM_APP_UPDATE: u32 = 0x8001;

const WS_OVERLAPPED: u32 = 0x0000_0000;
const WS_CAPTION: u32 = 0x00C0_0000;
const WS_SYSMENU: u32 = 0x0008_0000;
const WS_MINIMIZEBOX: u32 = 0x0002_0000;
const WS_VISIBLE: u32 = 0x1000_0000;

const CS_HREDRAW: u32 = 0x0002;
const CS_VREDRAW: u32 = 0x0001;
const IDC_ARROW: usize = 32512;
const IDC_HAND: usize = 32649;
const TRANSPARENT: i32 = 1;
const SRCCOPY: u32 = 0x00CC_0020;
const DT_SINGLELINE: u32 = 0x0020;
const DT_VCENTER: u32 = 0x0004;
const DT_CENTER: u32 = 0x0001;
const DT_NOPREFIX: u32 = 0x0800;
const DT_END_ELLIPSIS: u32 = 0x8000;
const CF_UNICODETEXT: u32 = 13;
const GMEM_MOVEABLE: u32 = 0x0002;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Point {
    x: i32,
    y: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct WinRect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

impl WinRect {
    fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self {
            left: x,
            top: y,
            right: x + w,
            bottom: y + h,
        }
    }
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.left && x < self.right && y >= self.top && y < self.bottom
    }
    fn inset(&self, dx: i32, dy: i32) -> Self {
        Self {
            left: self.left + dx,
            top: self.top + dy,
            right: self.right - dx,
            bottom: self.bottom - dy,
        }
    }
}

#[repr(C)]
struct PaintStruct {
    hdc: isize,
    erase: i32,
    rc_paint: WinRect,
    restore: i32,
    inc_update: i32,
    rgb_reserved: [u8; 32],
}

#[repr(C)]
struct WndClassExW {
    cb_size: u32,
    style: u32,
    lpfn_wnd_proc: unsafe extern "system" fn(isize, u32, usize, isize) -> isize,
    cb_cls_extra: i32,
    cb_wnd_extra: i32,
    h_instance: isize,
    h_icon: isize,
    h_cursor: isize,
    hbr_background: isize,
    lpsz_menu_name: *const u16,
    lpsz_class_name: *const u16,
    h_icon_sm: isize,
}

#[repr(C)]
struct Msg {
    hwnd: isize,
    message: u32,
    w_param: usize,
    l_param: isize,
    time: u32,
    pt: Point,
    l_private: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn SetProcessDPIAware() -> i32;
    fn RegisterClassExW(wc: *const WndClassExW) -> u16;
    fn CreateWindowExW(
        ex_style: u32,
        class_name: *const u16,
        window_name: *const u16,
        style: u32,
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        parent: isize,
        menu: isize,
        instance: isize,
        param: *mut c_void,
    ) -> isize;
    fn DefWindowProcW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> isize;
    fn GetMessageW(msg: *mut Msg, hwnd: isize, min: u32, max: u32) -> i32;
    fn TranslateMessage(msg: *const Msg) -> i32;
    fn DispatchMessageW(msg: *const Msg) -> isize;
    fn PostQuitMessage(code: i32);
    fn PostMessageW(hwnd: isize, msg: u32, w_param: usize, l_param: isize) -> i32;
    fn InvalidateRect(hwnd: isize, rect: *const WinRect, erase: i32) -> i32;
    fn BeginPaint(hwnd: isize, ps: *mut PaintStruct) -> isize;
    fn EndPaint(hwnd: isize, ps: *const PaintStruct) -> i32;
    fn GetClientRect(hwnd: isize, rect: *mut WinRect) -> i32;
    fn AdjustWindowRectEx(rect: *mut WinRect, style: u32, menu: i32, ex_style: u32) -> i32;
    fn GetSystemMetrics(index: i32) -> i32;
    fn LoadCursorW(instance: isize, cursor_name: *const u16) -> isize;
    fn SetCursor(cursor: isize) -> isize;
    fn FillRect(hdc: isize, rc: *const WinRect, brush: isize) -> i32;
    fn FrameRect(hdc: isize, rc: *const WinRect, brush: isize) -> i32;
    fn DrawTextW(hdc: isize, text: *const u16, count: i32, rc: *mut WinRect, format: u32) -> i32;
    fn SetTimer(hwnd: isize, id: usize, elapse: u32, func: *const c_void) -> usize;
    fn OpenClipboard(hwnd: isize) -> i32;
    fn EmptyClipboard() -> i32;
    fn SetClipboardData(format: u32, mem: isize) -> isize;
    fn CloseClipboard() -> i32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, hdc: isize) -> i32;
    fn MessageBoxW(hwnd: isize, text: *const u16, caption: *const u16, flags: u32) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn CreateCompatibleDC(hdc: isize) -> isize;
    fn DeleteDC(hdc: isize) -> i32;
    fn CreateCompatibleBitmap(hdc: isize, w: i32, h: i32) -> isize;
    fn SelectObject(hdc: isize, obj: isize) -> isize;
    fn DeleteObject(obj: isize) -> i32;
    fn BitBlt(
        dst: isize,
        dx: i32,
        dy: i32,
        w: i32,
        h: i32,
        src: isize,
        sx: i32,
        sy: i32,
        rop: u32,
    ) -> i32;
    fn CreateSolidBrush(color: u32) -> isize;
    fn SetBkMode(hdc: isize, mode: i32) -> i32;
    fn SetTextColor(hdc: isize, color: u32) -> u32;
    fn CreateFontW(
        height: i32,
        width: i32,
        escapement: i32,
        orientation: i32,
        weight: i32,
        italic: u32,
        underline: u32,
        strikeout: u32,
        charset: u32,
        out_precision: u32,
        clip_precision: u32,
        quality: u32,
        pitch_and_family: u32,
        face_name: *const u16,
    ) -> isize;
    fn GetDeviceCaps(hdc: isize, index: i32) -> i32;
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
    fn GlobalAlloc(flags: u32, bytes: usize) -> isize;
    fn GlobalLock(mem: isize) -> *mut u8;
    fn GlobalUnlock(mem: isize) -> i32;
    fn GlobalFree(mem: isize) -> isize;
}

#[link(name = "ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}

#[link(name = "shell32")]
unsafe extern "system" {
    fn ShellExecuteW(
        hwnd: isize,
        operation: *const u16,
        file: *const u16,
        parameters: *const u16,
        directory: *const u16,
        show_cmd: i32,
    ) -> isize;
}

#[link(name = "dwmapi")]
unsafe extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attr: u32, val: *const c_void, size: u32) -> i32;
}

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

const COL_BG: u32 = rgb(13, 17, 23);
const COL_HEADER: u32 = rgb(18, 24, 33);
const COL_CARD: u32 = rgb(22, 28, 38);
const COL_CARD_HOVER: u32 = rgb(29, 37, 50);
const COL_CARD_SEL: u32 = rgb(21, 43, 45);
const COL_BORDER: u32 = rgb(44, 54, 70);
const COL_MINT: u32 = rgb(148, 228, 211);
const COL_MINT_DARK: u32 = rgb(36, 138, 118);
const COL_EMERALD: u32 = rgb(28, 176, 122);
const COL_EMERALD_HOVER: u32 = rgb(42, 198, 140);
const COL_BLUE: u32 = rgb(37, 99, 235);
const COL_BLUE_HOVER: u32 = rgb(59, 130, 246);
const COL_BTN_SEC: u32 = rgb(33, 40, 52);
const COL_BTN_SEC_HOVER: u32 = rgb(48, 58, 74);
const COL_DANGER_HOVER: u32 = rgb(155, 44, 44);
const COL_TEXT: u32 = rgb(240, 246, 252);
const COL_MUTED: u32 = rgb(139, 148, 158);
const COL_WARN: u32 = rgb(245, 166, 35);
const COL_ERR: u32 = rgb(248, 81, 73);

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn copy_to_clipboard(hwnd: isize, text: &str) -> bool {
    let utf16 = wide(text);
    let bytes = utf16.len() * 2;
    unsafe {
        let mem = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if mem == 0 {
            return false;
        }
        let dst = GlobalLock(mem);
        if dst.is_null() {
            GlobalFree(mem);
            return false;
        }
        ptr::copy_nonoverlapping(utf16.as_ptr().cast::<u8>(), dst, bytes);
        GlobalUnlock(mem);
        if OpenClipboard(hwnd) == 0 {
            GlobalFree(mem);
            return false;
        }
        let copied = EmptyClipboard() != 0 && SetClipboardData(CF_UNICODETEXT, mem) != 0;
        CloseClipboard();
        if !copied {
            GlobalFree(mem);
        }
        copied
    }
}

fn open_browser(url: &str) -> bool {
    let op = wide("open");
    let target = wide(url);
    unsafe {
        // Shell handlers may need a single-threaded COM apartment on this worker.
        const COINIT_APARTMENTTHREADED: u32 = 2;
        if CoInitializeEx(ptr::null_mut(), COINIT_APARTMENTTHREADED) < 0 {
            return false;
        }
        let result = ShellExecuteW(0, op.as_ptr(), target.as_ptr(), ptr::null(), ptr::null(), 1);
        CoUninitialize();
        result > 32
    }
}

fn copy_and_open_link(hwnd: isize, code: &str, uri: &str) -> (bool, bool) {
    (copy_to_clipboard(hwnd, code), open_browser(uri))
}

enum WorkerMsg {
    DevicePrompt(DeviceCodePrompt),
    PromptLinkResult { copied: bool, opened: bool },
    BrowserApproved,
    AuthSuccess(AuthenticatedAccount),
    AccountSaved(AccountStore),
    AuthError(String),
    DumpProgress { done: usize, total: usize, stage: String },
    MissingJarAlert,
    LaunchSuccess(String),
    LaunchError(String),
}

#[derive(Clone, Debug)]
enum ResourceUiState {
    Ready(PathBuf),
    NeedsDump(PathBuf),
    Dumping { done: usize, total: usize, stage: String },
    Missing,
}

fn detect_resource_state() -> ResourceUiState {
    if let Some((_, jar_path)) = resource_dump::find_appdata_189_jar() {
        let dest = ms_auth::project_root().join(".local").join("resources");
        if resource_dump::is_dump_ready(&jar_path, &dest) {
            ResourceUiState::Ready(jar_path)
        } else {
            ResourceUiState::NeedsDump(jar_path)
        }
    } else {
        ResourceUiState::Missing
    }
}

fn show_missing_jar_alert(hwnd: isize) {
    const MB_OK_ICONWARNING: u32 = 0x0000_0030;
    let body = wide(resource_dump::MISSING_JAR_POPUP);
    let title = wide("Ferrum 1.8.9 — 리소스 확인");
    unsafe {
        MessageBoxW(hwnd, body.as_ptr(), title.as_ptr(), MB_OK_ICONWARNING);
    }
}

struct WorkerEvent {
    attempt: Option<Arc<AtomicBool>>,
    message: WorkerMsg,
}

fn post_event(tx: &Sender<WorkerEvent>, hwnd: isize, attempt: Option<&Arc<AtomicBool>>, message: WorkerMsg) {
    if tx.send(WorkerEvent { attempt: attempt.cloned(), message }).is_ok() {
        unsafe { PostMessageW(hwnd, WM_APP_UPDATE, 0, 0); }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HitTarget {
    AddMicrosoft,
    OfflineMode,
    RemoveAccount,
    CopyOpenLink,
    CancelAuth,
    SelectAccount(usize),
    RefreshResources,
    Launch,
}

struct HitBox {
    rect: WinRect,
    target: HitTarget,
    enabled: bool,
}

struct Fonts {
    title: isize,
    subtitle: isize,
    section: isize,
    body: isize,
    body_bold: isize,
    small: isize,
    code: isize,
    launch: isize,
}

impl Fonts {
    fn create(scale: f32) -> Self {
        let mk = |px: i32, weight: i32, face: &str| -> isize {
            let f = wide(face);
            let h = -((px as f32 * scale).round() as i32);
            unsafe {
                CreateFontW(
                    h,
                    0,
                    0,
                    0,
                    weight,
                    0,
                    0,
                    0,
                    1,
                    0,
                    0,
                    5, // CLEARTYPE_QUALITY
                    0,
                    f.as_ptr(),
                )
            }
        };
        Self {
            title: mk(23, 700, "Malgun Gothic"),
            subtitle: mk(13, 400, "Malgun Gothic"),
            section: mk(14, 700, "Malgun Gothic"),
            body: mk(14, 400, "Malgun Gothic"),
            body_bold: mk(15, 700, "Malgun Gothic"),
            small: mk(12, 400, "Malgun Gothic"),
            code: mk(22, 700, "Consolas"),
            launch: mk(17, 700, "Malgun Gothic"),
        }
    }
}

struct AppState {
    hwnd: isize,
    scale: f32,
    fonts: Option<Fonts>,
    store_path: PathBuf,
    store: AccountStore,
    resource_state: ResourceUiState,
    last_res_probe: Instant,
    mouse: Point,
    scroll: usize,
    status: String,
    status_color: u32,
    active_prompt: Option<(DeviceCodePrompt, Instant)>,
    auth_cancel: Option<Arc<AtomicBool>>,
    saving_account: bool,
    launching: bool,
    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
    hitboxes: Vec<HitBox>,
}

static STATE: OnceLock<Mutex<AppState>> = OnceLock::new();

impl AppState {
    fn new(scale: f32, store_path: PathBuf, store: AccountStore) -> Self {
        let resource_state = detect_resource_state();
        let (status, status_color) = if matches!(resource_state, ResourceUiState::Missing) {
            (resource_dump::MISSING_JAR_NOTICE.to_owned(), COL_ERR)
        } else if let Some(acc) = store.selected() {
            (
                format!("준비 완료 — '{}' Microsoft 계정으로 멀티플레이어 실행 가능", acc.username),
                COL_MINT,
            )
        } else {
            (
                "Microsoft 계정을 추가하거나 LAUNCH를 눌러 게임을 실행하세요.".into(),
                COL_MINT,
            )
        };
        let (tx, rx) = mpsc::channel();
        Self {
            hwnd: 0,
            scale,
            fonts: None,
            store_path,
            store,
            resource_state,
            last_res_probe: Instant::now(),
            mouse: Point { x: -1, y: -1 },
            scroll: 0,
            status,
            status_color,
            active_prompt: None,
            auth_cancel: None,
            saving_account: false,
            launching: false,
            tx,
            rx,
            hitboxes: Vec::new(),
        }
    }

    fn poll_resource_state_if_idle(&mut self) {
        if self.launching || self.last_res_probe.elapsed() < Duration::from_secs(2) {
            return;
        }
        self.last_res_probe = Instant::now();
        let was_missing = matches!(self.resource_state, ResourceUiState::Missing);
        if was_missing {
            let next = detect_resource_state();
            if !matches!(next, ResourceUiState::Missing) {
                self.resource_state = next;
                self.status = "✓ %APPDATA%\\.minecraft 에서 1.8.9.jar 감지됨! LAUNCH를 눌러 시작하세요.".into();
                self.status_color = COL_MINT;
            }
        }
    }

    fn s(&self, v: i32) -> i32 {
        (v as f32 * self.scale).round() as i32
    }

    fn locate_game_binary() -> Option<PathBuf> {
        let root = ms_auth::project_root();
        let mut candidates = vec![
            root.join("target").join("release").join("ferrum189.exe"),
            root.join("ferrum189.exe"),
            root.join("target").join("debug").join("ferrum189.exe"),
        ];
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.insert(0, dir.join("target").join("release").join("ferrum189.exe"));
                candidates.insert(1, dir.join("ferrum189.exe"));
            }
        }
        candidates.into_iter().find(|p| p.is_file())
    }

    fn present_prompt_link(
        &mut self,
        present: impl FnOnce(isize, &str, &str) -> (bool, bool) + Send + 'static,
    ) {
        let Some((prompt, _)) = &self.active_prompt else { return };
        let Some(attempt) = self.auth_cancel.clone() else { return };
        let code = prompt.user_code.clone();
        let uri = prompt.verification_uri.clone();
        let tx = self.tx.clone();
        let hwnd = self.hwnd;
        self.status = "인증 페이지를 여는 중입니다. 위 주소와 코드로 직접 로그인할 수도 있습니다.".into();
        self.status_color = COL_MINT;
        // Clipboard ownership changes and shell extensions can synchronously send window
        // messages. Let the window thread finish dispatch before either can need it.
        std::thread::spawn(move || {
            if attempt.load(Ordering::Relaxed) { return; }
            let (copied, opened) = present(hwnd, &code, &uri);
            post_event(&tx, hwnd, Some(&attempt), WorkerMsg::PromptLinkResult { copied, opened });
        });
    }

    fn drain_worker(&mut self) {
        while let Ok(event) = self.rx.try_recv() {
            if let Some(attempt) = &event.attempt {
                if !self.auth_cancel.as_ref().is_some_and(|current| Arc::ptr_eq(current, attempt)) {
                    continue;
                }
            }
            match event.message {
                WorkerMsg::DevicePrompt(prompt) => {
                    self.active_prompt = Some((prompt, Instant::now()));
                    self.present_prompt_link(copy_and_open_link);
                }
                WorkerMsg::PromptLinkResult { copied, opened } => {
                    if self.active_prompt.is_some() {
                        self.status = match (copied, opened) {
                            (true, true) => "열린 브라우저에 복사된 코드를 붙여 넣고 로그인하세요.",
                            (false, true) => "코드를 복사하지 못했습니다. 열린 브라우저에 위 코드를 직접 입력하세요.",
                            (true, false) => "브라우저를 열지 못했습니다. 위 인증 주소를 직접 열고 복사된 코드를 붙여 넣으세요.",
                            (false, false) => "위 인증 주소를 직접 열고 표시된 코드를 입력하세요.",
                        }.into();
                        self.status_color = if copied && opened { COL_MINT } else { COL_WARN };
                    }
                }
                WorkerMsg::BrowserApproved => {
                    self.active_prompt = None;
                    self.status = "Microsoft 승인 완료. Xbox와 Minecraft 프로필을 확인하는 중…".into();
                    self.status_color = COL_WARN;
                }
                WorkerMsg::AuthSuccess(account) => {
                    let Some(attempt) = event.attempt else { continue; };
                    self.active_prompt = None;
                    self.saving_account = true;
                    self.status = "인증 완료. 이 PC에 계정을 저장하는 중…".into();
                    self.status_color = COL_WARN;
                    let path = self.store_path.clone();
                    let tx = self.tx.clone();
                    let hwnd = self.hwnd;
                    std::thread::spawn(move || {
                        let save = || -> anyhow::Result<AccountStore> {
                            let mut store = AccountStore::load_from(&path)?;
                            store.upsert_authenticated(&account)?;
                            store.save_to(&path)?;
                            Ok(store)
                        };
                        let message = match save() {
                            Ok(store) => WorkerMsg::AccountSaved(store),
                            Err(e) => WorkerMsg::AuthError(format!("계정 저장 오류: {e}")),
                        };
                        post_event(&tx, hwnd, Some(&attempt), message);
                    });
                }
                WorkerMsg::AccountSaved(store) => {
                    self.saving_account = false;
                    self.auth_cancel = None;
                    self.status = format!("Microsoft 계정 '{}'을 추가하고 선택했습니다.",
                        store.selected().map_or("", |a| a.username.as_str()));
                    self.store = store;
                    self.status_color = COL_MINT;
                }
                WorkerMsg::AuthError(err) => {
                    self.saving_account = false;
                    self.active_prompt = None;
                    self.auth_cancel = None;
                    self.status = format!("로그인 실패: {err}");
                    self.status_color = COL_ERR;
                }
                WorkerMsg::DumpProgress { done, total, stage } => {
                    let pct = (done.saturating_mul(100) / total.max(1)).min(100);
                    self.status = format!("{stage} ({pct}%)");
                    self.status_color = COL_MINT;
                    self.resource_state = ResourceUiState::Dumping { done, total, stage };
                }
                WorkerMsg::MissingJarAlert => {
                    self.launching = false;
                    self.resource_state = ResourceUiState::Missing;
                    self.status = resource_dump::MISSING_JAR_NOTICE.into();
                    self.status_color = COL_ERR;
                }
                WorkerMsg::LaunchSuccess(msg) => {
                    self.launching = false;
                    self.resource_state = detect_resource_state();
                    self.store = AccountStore::load_from(&self.store_path).unwrap_or_default();
                    self.status = msg;
                    self.status_color = COL_MINT;
                }
                WorkerMsg::LaunchError(err) => {
                    self.launching = false;
                    self.resource_state = detect_resource_state();
                    self.status = format!("실행 실패: {err}");
                    self.status_color = COL_ERR;
                }
            }
        }
    }

    fn start_add_microsoft(&mut self) {
        if self.auth_cancel.is_some() || self.launching {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.auth_cancel = Some(cancel.clone());
        self.status = "Microsoft 기기 인증 코드를 요청하는 중…".into();
        self.status_color = COL_WARN;
        let tx = self.tx.clone();
        let hwnd = self.hwnd;
        std::thread::spawn(move || {
            let post = |msg| post_event(&tx, hwnd, Some(&cancel), msg);
            let prompt = match ms_auth::start_device_code_flow() {
                Ok(prompt) => prompt,
                Err(e) => { post(WorkerMsg::AuthError(format!("{e:#}"))); return; }
            };
            if cancel.load(Ordering::Relaxed) { return; }
            let mut interval = Duration::from_secs(prompt.interval_secs.max(3));
            let issued = Instant::now();
            let lifetime = Duration::from_secs(prompt.expires_in_secs);
            let code = prompt.device_code.clone();
            post(WorkerMsg::DevicePrompt(prompt));
            loop {
                let wait = interval.min(lifetime.saturating_sub(issued.elapsed()));
                let last_poll = Instant::now();
                while last_poll.elapsed() < wait {
                    if cancel.load(Ordering::Relaxed) { return; }
                    std::thread::sleep(Duration::from_millis(200).min(wait.saturating_sub(last_poll.elapsed())));
                }
                if cancel.load(Ordering::Relaxed) { return; }
                if issued.elapsed() >= lifetime {
                    post(WorkerMsg::AuthError("인증 코드가 만료되었습니다. 다시 시도해 주세요.".into()));
                    return;
                }
                match ms_auth::poll_device_code_with_progress(&code, || post(WorkerMsg::BrowserApproved)) {
                    Ok(PollOutcome::Pending) => {}
                    Ok(PollOutcome::SlowDown) => interval = interval.saturating_add(Duration::from_secs(5)),
                    Ok(PollOutcome::Authorized(account)) => { post(WorkerMsg::AuthSuccess(account)); return; }
                    Err(e) => { post(WorkerMsg::AuthError(format!("{e:#}"))); return; }
                }
            }
        });
    }

    fn start_launch(&mut self) -> bool {
        if self.launching || self.auth_cancel.is_some() {
            return false;
        }
        let Some(exe_path) = Self::locate_game_binary() else {
            self.status = "ferrum189.exe 실행 파일을 찾을 수 없습니다 (target\\release\\ferrum189.exe 확인 필요).".into();
            self.status_color = COL_ERR;
            return false;
        };
        let Some((mc_dir, jar_src)) = resource_dump::find_appdata_189_jar() else {
            self.resource_state = ResourceUiState::Missing;
            self.status = resource_dump::MISSING_JAR_NOTICE.into();
            self.status_color = COL_ERR;
            return true;
        };

        let root = ms_auth::project_root();
        let dest_dir = root.join(".local").join("resources");
        let needs_dump = !resource_dump::is_dump_ready(&jar_src, &dest_dir);

        self.launching = true;
        if needs_dump {
            self.resource_state = ResourceUiState::Dumping {
                done: 0,
                total: 100,
                stage: "1.8.9.jar 리소스 덤프 준비 중...".into(),
            };
            self.status = "%APPDATA%\\.minecraft 의 1.8.9.jar 리소스를 덤프하는 중...".into();
        } else {
            self.resource_state = ResourceUiState::Ready(jar_src.clone());
            self.status = if self.store.selected().is_some() {
                "Microsoft 마인크래프트 세션 확인 및 Ferrum 1.8.9 실행 중...".into()
            } else {
                "오프라인 모드로 Ferrum 1.8.9 실행 중...".into()
            };
        }
        self.status_color = COL_WARN;

        let store_path = self.store_path.clone();
        let tx = self.tx.clone();
        let hwnd = self.hwnd;

        std::thread::spawn(move || {
            let dumped_jar = match resource_dump::ensure_dumped_resources(
                &mc_dir,
                &jar_src,
                &dest_dir,
                |done, total, stage| {
                    post_event(
                        &tx,
                        hwnd,
                        None,
                        WorkerMsg::DumpProgress {
                            done,
                            total,
                            stage: stage.to_owned(),
                        },
                    );
                },
            ) {
                Ok(path) => path,
                Err(e) => {
                    post_event(
                        &tx,
                        hwnd,
                        None,
                        WorkerMsg::LaunchError(format!("1.8.9 리소스 덤프 실패: {e:#}")),
                    );
                    return;
                }
            };

            let session = AccountStore::load_from(&store_path).and_then(|mut store| {
                let auth = store.ensure_fresh_selected(&store_path)?;
                Ok((store, auth))
            });
            let (store, auth) = match session {
                Ok(result) => result,
                Err(e) => {
                    post_event(&tx, hwnd, None, WorkerMsg::LaunchError(format!("{e:#}")));
                    return;
                }
            };

            let mut cmd = std::process::Command::new(&exe_path);
            cmd.current_dir(&root).arg("--jar").arg(&dumped_jar);
            let msg = if let Some(acc) = auth {
                cmd.arg("--username")
                    .arg(&acc.username)
                    .env("FERRUM189_MC_ACCESS_TOKEN", &acc.mc_access_token)
                    .env("FERRUM189_MC_PROFILE_ID", &acc.uuid)
                    .env("FERRUM189_MC_USERNAME", &acc.username);
                format!(
                    "✓ '{}' (Microsoft 정품 인증) 계정으로 Ferrum 1.8.9가 실행되었습니다!",
                    acc.username
                )
            } else {
                cmd.arg("--username")
                    .arg(&store.offline_username)
                    .env_remove("FERRUM189_MC_ACCESS_TOKEN")
                    .env_remove("FERRUM189_MC_PROFILE_ID")
                    .env_remove("FERRUM189_MC_USERNAME");
                format!(
                    "✓ 오프라인 모드('{}')로 Ferrum 1.8.9가 실행되었습니다.",
                    store.offline_username
                )
            };

            let result = match cmd.spawn() {
                Ok(_) => WorkerMsg::LaunchSuccess(msg),
                Err(e) => WorkerMsg::LaunchError(format!("{} 실행 실패: {e}", exe_path.display())),
            };
            post_event(&tx, hwnd, None, result);
        });
        false
    }

    fn on_click(&mut self, x: i32, y: i32) -> bool {
        let clicked = self
            .hitboxes
            .iter()
            .rev()
            .find(|h| h.enabled && h.rect.contains(x, y))
            .map(|h| h.target);
        let Some(target) = clicked else {
            return false;
        };
        match target {
            HitTarget::AddMicrosoft => self.start_add_microsoft(),
            HitTarget::OfflineMode => {
                self.store.selected_uuid = None;
                let _ = self.store.save_to(&self.store_path);
                self.status = "오프라인 모드로 전환되었습니다. 정품 서버 접속 시 Microsoft 계정을 클릭해 선택하세요.".into();
                self.status_color = COL_MUTED;
            }
            HitTarget::RemoveAccount => {
                if let Some(sel) = self.store.selected_uuid.clone() {
                    let removed_name = self
                        .store
                        .selected()
                        .map(|a| a.username.clone())
                        .unwrap_or_default();
                    self.store.remove_uuid(&sel);
                    let _ = self.store.save_to(&self.store_path);
                    self.status = format!("'{}' 계정이 목록에서 제거되었습니다.", removed_name);
                    self.status_color = COL_MUTED;
                }
            }
            HitTarget::CopyOpenLink => {
                self.present_prompt_link(copy_and_open_link);
            }
            HitTarget::CancelAuth => {
                if self.saving_account { return false; }
                if let Some(cancel) = self.auth_cancel.take() {
                    cancel.store(true, Ordering::Relaxed);
                }
                self.active_prompt = None;
                self.status = "Microsoft 계정 추가가 취소되었습니다.".into();
                self.status_color = COL_MUTED;
            }
            HitTarget::SelectAccount(idx) => {
                if let Some(acc) = self.store.accounts.get(idx) {
                    self.store.selected_uuid = Some(acc.uuid.clone());
                    let name = acc.username.clone();
                    let _ = self.store.save_to(&self.store_path);
                    self.status = format!(
                        "✓ '{}' 계정이 멀티플레이어 접속 계정으로 선택되었습니다.",
                        name
                    );
                    self.status_color = COL_MINT;
                }
            }
            HitTarget::RefreshResources => {
                self.resource_state = detect_resource_state();
                match &self.resource_state {
                    ResourceUiState::Ready(_) => {
                        self.status = "✓ 1.8.9.jar 및 로컬 덤프 리소스가 준비되어 있습니다.".into();
                        self.status_color = COL_MINT;
                    }
                    ResourceUiState::NeedsDump(_) => {
                        self.status = "✓ %APPDATA%\\.minecraft 에서 1.8.9.jar를 찾았습니다! LAUNCH 클릭 시 자동 덤프됩니다.".into();
                        self.status_color = COL_MINT;
                    }
                    ResourceUiState::Missing => {
                        self.status = resource_dump::MISSING_JAR_NOTICE.into();
                        self.status_color = COL_ERR;
                        return true;
                    }
                    ResourceUiState::Dumping { .. } => {}
                }
            }
            HitTarget::Launch => return self.start_launch(),
        }
        false
    }

    fn paint(&mut self, hdc: isize, width: i32, height: i32) {
        if self.fonts.is_none() {
            self.fonts = Some(Fonts::create(self.scale));
        }
        let fonts = self.fonts.as_ref().unwrap();
        self.hitboxes.clear();

        unsafe {
            SetBkMode(hdc, TRANSPARENT);
        }

        // Background
        fill(hdc, WinRect::new(0, 0, width, height), COL_BG);

        // Header banner
        let hdr_h = self.s(82);
        fill(hdc, WinRect::new(0, 0, width, hdr_h), COL_HEADER);
        fill(
            hdc,
            WinRect::new(0, hdr_h - self.s(2), width, self.s(2)),
            COL_MINT_DARK,
        );

        let pad = self.s(24);
        draw_text(
            hdc,
            fonts.title,
            COL_TEXT,
            WinRect::new(pad, self.s(16), width - pad * 2, self.s(30)),
            "FERRUM 1.8.9 CLIENT",
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );
        draw_text(
            hdc,
            fonts.subtitle,
            COL_MINT,
            WinRect::new(pad, self.s(46), width - pad * 2, self.s(22)),
            "Native Rust + Vulkan Multiplayer Boost Client  ·  Protocol 47 (1.8.9)",
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );

        // Active Account Badge in top-right of header
        let badge_w = self.s(250);
        let badge_r = WinRect::new(width - pad - badge_w, self.s(22), badge_w, self.s(38));
        fill(hdc, badge_r, COL_CARD_SEL);
        border(hdc, badge_r, COL_MINT_DARK);
        let active_str = if let Some(acc) = self.store.selected() {
            format!("선택됨: {} (MS 정품)", acc.username)
        } else {
            format!("오프라인: {}", self.store.offline_username)
        };
        draw_text(
            hdc,
            fonts.small,
            COL_MINT,
            badge_r.inset(self.s(10), 0),
            &active_str,
            DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );

        // Section Header + Action Buttons
        let sec_y = hdr_h + self.s(18);
        draw_text(
            hdc,
            fonts.section,
            COL_TEXT,
            WinRect::new(pad, sec_y, self.s(260), self.s(32)),
            "Microsoft 멀티플레이어 계정",
            DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
        );

        let add_btn = WinRect::new(width - pad - self.s(175), sec_y, self.s(175), self.s(34));
        let auth_busy = self.auth_cancel.is_some();
        let can_change_account = !auth_busy && !self.launching;
        let header_cancels = auth_busy && self.active_prompt.is_none() && !self.saving_account;
        let add_enabled = can_change_account || header_cancels;
        draw_button(
            hdc,
            fonts.body_bold,
            add_btn,
            if header_cancels { "인증 취소" } else { "+ Microsoft 계정 추가" },
            add_enabled,
            add_btn.contains(self.mouse.x, self.mouse.y),
            COL_BLUE,
            COL_BLUE_HOVER,
            COL_TEXT,
        );
        self.hitboxes.push(HitBox {
            rect: add_btn,
            target: if header_cancels { HitTarget::CancelAuth } else { HitTarget::AddMicrosoft },
            enabled: add_enabled,
        });

        let del_enabled = self.store.selected().is_some() && can_change_account;
        let del_btn = WinRect::new(add_btn.left - self.s(98), sec_y, self.s(90), self.s(34));
        draw_button(
            hdc,
            fonts.small,
            del_btn,
            "계정 삭제",
            del_enabled,
            del_btn.contains(self.mouse.x, self.mouse.y),
            COL_BTN_SEC,
            COL_DANGER_HOVER,
            COL_TEXT,
        );
        self.hitboxes.push(HitBox {
            rect: del_btn,
            target: HitTarget::RemoveAccount,
            enabled: del_enabled,
        });

        let off_enabled = self.store.selected().is_some() && can_change_account;
        let off_btn = WinRect::new(del_btn.left - self.s(106), sec_y, self.s(98), self.s(34));
        draw_button(
            hdc,
            fonts.small,
            off_btn,
            "오프라인 전환",
            off_enabled,
            off_btn.contains(self.mouse.x, self.mouse.y),
            COL_BTN_SEC,
            COL_BTN_SEC_HOVER,
            COL_TEXT,
        );
        self.hitboxes.push(HitBox {
            rect: off_btn,
            target: HitTarget::OfflineMode,
            enabled: off_enabled,
        });

        let mut list_top = sec_y + self.s(44);
        let footer_top = height - self.s(104);

        // Device Code Prompt Banner (when adding a Microsoft account)
        if let Some((prompt, _)) = &self.active_prompt {
            let banner_h = self.s(102);
            let banner = WinRect::new(pad, list_top, width - pad * 2, banner_h);
            fill(hdc, banner, COL_CARD_SEL);
            border(hdc, banner, COL_MINT);

            draw_text(
                hdc,
                fonts.body_bold,
                COL_MINT,
                WinRect::new(
                    banner.left + self.s(16),
                    banner.top + self.s(10),
                    self.s(420),
                    self.s(22),
                ),
                "브라우저에서 아래 인증 코드를 입력해 로그인하세요.",
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );

            let code_box = WinRect::new(
                banner.left + self.s(16),
                banner.top + self.s(38),
                self.s(185),
                self.s(48),
            );
            fill(hdc, code_box, COL_BG);
            border(hdc, code_box, COL_MINT);
            draw_text(
                hdc,
                fonts.code,
                COL_TEXT,
                code_box,
                &prompt.user_code,
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );

            draw_text(
                hdc,
                fonts.small,
                COL_MUTED,
                WinRect::new(
                    code_box.right + self.s(14),
                    banner.top + self.s(40),
                    self.s(280),
                    self.s(22),
                ),
                &format!("인증 주소: {}", prompt.verification_uri),
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );
            draw_text(
                hdc,
                fonts.small,
                COL_WARN,
                WinRect::new(
                    code_box.right + self.s(14),
                    banner.top + self.s(62),
                    self.s(280),
                    self.s(20),
                ),
                "승인 완료 시 자동으로 마인크래프트 계정이 등록됩니다...",
                DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
            );

            let copy_btn = WinRect::new(
                banner.right - self.s(178),
                banner.top + self.s(16),
                self.s(162),
                self.s(32),
            );
            draw_button(
                hdc,
                fonts.small,
                copy_btn,
                "코드 복사 & 링크 열기",
                true,
                copy_btn.contains(self.mouse.x, self.mouse.y),
                COL_BLUE,
                COL_BLUE_HOVER,
                COL_TEXT,
            );
            self.hitboxes.push(HitBox {
                rect: copy_btn,
                target: HitTarget::CopyOpenLink,
                enabled: true,
            });

            let cancel_btn = WinRect::new(
                banner.right - self.s(178),
                banner.top + self.s(54),
                self.s(162),
                self.s(30),
            );
            draw_button(
                hdc,
                fonts.small,
                cancel_btn,
                "인증 취소",
                true,
                cancel_btn.contains(self.mouse.x, self.mouse.y),
                COL_BTN_SEC,
                COL_DANGER_HOVER,
                COL_TEXT,
            );
            self.hitboxes.push(HitBox {
                rect: cancel_btn,
                target: HitTarget::CancelAuth,
                enabled: true,
            });

            list_top += banner_h + self.s(12);
        }

        // Resource Status Card geometry (above footer)
        let res_card_h = self.s(66);
        let res_card_top = footer_top - res_card_h - self.s(12);

        // Account List Area
        let list_rect = WinRect::new(
            pad,
            list_top,
            width - pad * 2,
            (res_card_top - self.s(12) - list_top).max(self.s(60)),
        );
        fill(hdc, list_rect, COL_HEADER);
        border(hdc, list_rect, COL_BORDER);

        if self.store.accounts.is_empty() {
            draw_text(
                hdc,
                fonts.body_bold,
                COL_TEXT,
                WinRect::new(
                    list_rect.left,
                    list_rect.top + (list_rect.bottom - list_rect.top) / 2 - self.s(22),
                    list_rect.right - list_rect.left,
                    self.s(24),
                ),
                "등록된 Microsoft 계정이 없습니다.",
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );
            draw_text(
                hdc,
                fonts.body,
                COL_MUTED,
                WinRect::new(
                    list_rect.left,
                    list_rect.top + (list_rect.bottom - list_rect.top) / 2 + self.s(4),
                    list_rect.right - list_rect.left,
                    self.s(22),
                ),
                "우측 상단의 [+ Microsoft 계정 추가] 버튼을 눌러 정품 멀티플레이어 계정을 연결하세요.",
                DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
            );
        } else {
            let row_h = self.s(58);
            let gap = self.s(8);
            let max_rows =
                ((list_rect.bottom - list_rect.top - self.s(16)) / (row_h + gap)).max(1) as usize;
            let max_scroll = self.store.accounts.len().saturating_sub(max_rows);
            let scroll = self.scroll.min(max_scroll);

            for (vis_i, idx) in (scroll..self.store.accounts.len())
                .take(max_rows)
                .enumerate()
            {
                let acc = &self.store.accounts[idx];
                let ry = list_rect.top + self.s(10) + vis_i as i32 * (row_h + gap);
                let card = WinRect::new(
                    list_rect.left + self.s(10),
                    ry,
                    (list_rect.right - list_rect.left) - self.s(20),
                    row_h,
                );
                let is_sel = self.store.selected_uuid.as_deref() == Some(acc.uuid.as_str());
                let is_hov = card.contains(self.mouse.x, self.mouse.y);
                fill(
                    hdc,
                    card,
                    if is_sel {
                        COL_CARD_SEL
                    } else if is_hov {
                        COL_CARD_HOVER
                    } else {
                        COL_CARD
                    },
                );
                border(hdc, card, if is_sel { COL_MINT } else { COL_BORDER });

                // Avatar tile
                let av = WinRect::new(
                    card.left + self.s(12),
                    card.top + self.s(10),
                    self.s(38),
                    self.s(38),
                );
                fill(hdc, av, if is_sel { COL_MINT_DARK } else { COL_BLUE });
                let initial: String = acc.username.chars().take(2).collect();
                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_TEXT,
                    av,
                    &initial,
                    DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                );

                // Username & UUID
                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_TEXT,
                    WinRect::new(
                        av.right + self.s(12),
                        card.top + self.s(8),
                        self.s(320),
                        self.s(22),
                    ),
                    &acc.username,
                    DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                );
                draw_text(
                    hdc,
                    fonts.small,
                    COL_MUTED,
                    WinRect::new(
                        av.right + self.s(12),
                        card.top + self.s(30),
                        self.s(360),
                        self.s(20),
                    ),
                    &format!("UUID: {}", ms_auth::dashed_uuid(&acc.uuid)),
                    DT_SINGLELINE | DT_VCENTER | DT_NOPREFIX,
                );

                // Status pill on right
                let pill_w = self.s(132);
                let pill = WinRect::new(
                    card.right - pill_w - self.s(14),
                    card.top + self.s(15),
                    pill_w,
                    self.s(28),
                );
                if is_sel {
                    fill(hdc, pill, COL_MINT_DARK);
                    draw_text(
                        hdc,
                        fonts.small,
                        COL_TEXT,
                        pill,
                        "✓ 활성 멀티 계정",
                        DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                    );
                } else {
                    fill(hdc, pill, COL_BTN_SEC);
                    draw_text(
                        hdc,
                        fonts.small,
                        COL_MUTED,
                        pill,
                        "클릭하여 선택",
                        DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                    );
                }

                self.hitboxes.push(HitBox {
                    rect: card,
                    target: HitTarget::SelectAccount(idx),
                    enabled: can_change_account,
                });
            }
        }

        // Minecraft 1.8.9 Resource Status & Auto-Dump Card
        let res_card = WinRect::new(pad, res_card_top, width - pad * 2, res_card_h);
        match &self.resource_state {
            ResourceUiState::Ready(jar_path) => {
                fill(hdc, res_card, COL_CARD_SEL);
                border(hdc, res_card, COL_MINT_DARK);

                let tag = WinRect::new(
                    res_card.left + self.s(12),
                    res_card.top + self.s(14),
                    self.s(64),
                    self.s(38),
                );
                fill(hdc, tag, COL_MINT_DARK);
                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_TEXT,
                    tag,
                    "1.8.9",
                    DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                );

                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_MINT,
                    WinRect::new(
                        tag.right + self.s(12),
                        res_card.top + self.s(10),
                        res_card.right - tag.right - self.s(124),
                        self.s(22),
                    ),
                    "✓ Minecraft 1.8.9 리소스 덤프 완료 (.local/resources)",
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                draw_text(
                    hdc,
                    fonts.small,
                    COL_MUTED,
                    WinRect::new(
                        tag.right + self.s(12),
                        res_card.top + self.s(34),
                        res_card.right - tag.right - self.s(124),
                        self.s(20),
                    ),
                    &format!("원본 확인됨: {}", jar_path.display()),
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );

                let ref_btn = WinRect::new(
                    res_card.right - self.s(102),
                    res_card.top + self.s(17),
                    self.s(88),
                    self.s(32),
                );
                draw_button(
                    hdc,
                    fonts.small,
                    ref_btn,
                    "다시 확인",
                    can_change_account,
                    ref_btn.contains(self.mouse.x, self.mouse.y),
                    COL_BTN_SEC,
                    COL_BTN_SEC_HOVER,
                    COL_TEXT,
                );
                self.hitboxes.push(HitBox {
                    rect: ref_btn,
                    target: HitTarget::RefreshResources,
                    enabled: can_change_account,
                });
            }
            ResourceUiState::NeedsDump(jar_path) => {
                fill(hdc, res_card, COL_CARD);
                border(hdc, res_card, COL_BLUE);

                let tag = WinRect::new(
                    res_card.left + self.s(12),
                    res_card.top + self.s(14),
                    self.s(64),
                    self.s(38),
                );
                fill(hdc, tag, COL_BLUE);
                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_TEXT,
                    tag,
                    "1.8.9",
                    DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                );

                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_TEXT,
                    WinRect::new(
                        tag.right + self.s(12),
                        res_card.top + self.s(10),
                        res_card.right - tag.right - self.s(124),
                        self.s(22),
                    ),
                    "Minecraft 1.8.9 클라이언트 감지됨 — 첫 실행 시 자동 덤프",
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                draw_text(
                    hdc,
                    fonts.small,
                    COL_MUTED,
                    WinRect::new(
                        tag.right + self.s(12),
                        res_card.top + self.s(34),
                        res_card.right - tag.right - self.s(124),
                        self.s(20),
                    ),
                    &format!("감지 경로: {}", jar_path.display()),
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );

                let ref_btn = WinRect::new(
                    res_card.right - self.s(102),
                    res_card.top + self.s(17),
                    self.s(88),
                    self.s(32),
                );
                draw_button(
                    hdc,
                    fonts.small,
                    ref_btn,
                    "다시 확인",
                    can_change_account,
                    ref_btn.contains(self.mouse.x, self.mouse.y),
                    COL_BTN_SEC,
                    COL_BTN_SEC_HOVER,
                    COL_TEXT,
                );
                self.hitboxes.push(HitBox {
                    rect: ref_btn,
                    target: HitTarget::RefreshResources,
                    enabled: can_change_account,
                });
            }
            ResourceUiState::Dumping { done, total, stage } => {
                fill(hdc, res_card, COL_CARD_SEL);
                border(hdc, res_card, COL_MINT);

                let pct = ((*done).saturating_mul(100) / (*total).max(1)).min(100);
                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_MINT,
                    WinRect::new(
                        res_card.left + self.s(16),
                        res_card.top + self.s(10),
                        res_card.right - res_card.left - self.s(32),
                        self.s(22),
                    ),
                    &format!("{stage}  ({pct}% · {done}/{total})"),
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );

                let bar_bg = WinRect::new(
                    res_card.left + self.s(16),
                    res_card.top + self.s(38),
                    res_card.right - res_card.left - self.s(32),
                    self.s(14),
                );
                fill(hdc, bar_bg, COL_BG);
                border(hdc, bar_bg, COL_MINT_DARK);
                let inner_w = (bar_bg.right - bar_bg.left - self.s(2)).max(0);
                let fill_w = ((inner_w as usize).saturating_mul(*done) / (*total).max(1)) as i32;
                if fill_w > 0 {
                    fill(
                        hdc,
                        WinRect::new(
                            bar_bg.left + self.s(1),
                            bar_bg.top + self.s(1),
                            fill_w.min(inner_w),
                            (bar_bg.bottom - bar_bg.top - self.s(2)).max(1),
                        ),
                        COL_EMERALD,
                    );
                }
            }
            ResourceUiState::Missing => {
                fill(hdc, res_card, rgb(42, 20, 24));
                border(hdc, res_card, COL_ERR);

                let tag = WinRect::new(
                    res_card.left + self.s(12),
                    res_card.top + self.s(14),
                    self.s(64),
                    self.s(38),
                );
                fill(hdc, tag, COL_ERR);
                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_TEXT,
                    tag,
                    "필수",
                    DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
                );

                draw_text(
                    hdc,
                    fonts.body_bold,
                    COL_ERR,
                    WinRect::new(
                        tag.right + self.s(12),
                        res_card.top + self.s(10),
                        res_card.right - tag.right - self.s(124),
                        self.s(22),
                    ),
                    resource_dump::MISSING_JAR_NOTICE,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );
                draw_text(
                    hdc,
                    fonts.small,
                    COL_TEXT,
                    WinRect::new(
                        tag.right + self.s(12),
                        res_card.top + self.s(34),
                        res_card.right - tag.right - self.s(124),
                        self.s(20),
                    ),
                    "찾는 위치: %APPDATA%\\.minecraft\\versions\\1.8.9\\1.8.9.jar",
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
                );

                let ref_btn = WinRect::new(
                    res_card.right - self.s(102),
                    res_card.top + self.s(17),
                    self.s(88),
                    self.s(32),
                );
                draw_button(
                    hdc,
                    fonts.small,
                    ref_btn,
                    "다시 확인",
                    can_change_account,
                    ref_btn.contains(self.mouse.x, self.mouse.y),
                    COL_BTN_SEC,
                    COL_BTN_SEC_HOVER,
                    COL_TEXT,
                );
                self.hitboxes.push(HitBox {
                    rect: ref_btn,
                    target: HitTarget::RefreshResources,
                    enabled: can_change_account,
                });
            }
        }

        // Bottom Footer Bar + Prominent LAUNCH Button
        fill(
            hdc,
            WinRect::new(0, footer_top, width, height - footer_top),
            COL_HEADER,
        );
        fill(
            hdc,
            WinRect::new(0, footer_top, width, self.s(1)),
            COL_BORDER,
        );

        let launch_w = self.s(240);
        let launch_h = self.s(54);
        let launch_btn = WinRect::new(
            width - pad - launch_w,
            footer_top + self.s(24),
            launch_w,
            launch_h,
        );
        let launch_label = if self.launching {
            "실행 준비 중..."
        } else {
            "LAUNCH  (게임 실행)"
        };
        draw_button(
            hdc,
            fonts.launch,
            launch_btn,
            launch_label,
            can_change_account,
            launch_btn.contains(self.mouse.x, self.mouse.y),
            COL_EMERALD,
            COL_EMERALD_HOVER,
            COL_TEXT,
        );
        self.hitboxes.push(HitBox {
            rect: launch_btn,
            target: HitTarget::Launch,
            enabled: can_change_account,
        });

        // Status text on bottom-left
        let status_rc = WinRect::new(
            pad,
            footer_top + self.s(18),
            launch_btn.left - pad - self.s(16),
            self.s(34),
        );
        draw_text(
            hdc,
            fonts.body,
            self.status_color,
            status_rc,
            &self.status,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
        let sub_hint = if let Some(acc) = self.store.selected() {
            format!(
                "LAUNCH 클릭 시 '{}' Microsoft 세션으로 정품 멀티플레이어 서버에 로그인합니다.",
                acc.username
            )
        } else {
            "Microsoft 계정을 선택하지 않으면 로컬/오프라인 모드로 실행됩니다.".into()
        };
        draw_text(
            hdc,
            fonts.small,
            COL_MUTED,
            WinRect::new(
                pad,
                footer_top + self.s(52),
                launch_btn.left - pad - self.s(16),
                self.s(24),
            ),
            &sub_hint,
            DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
        );
    }
}

fn fill(hdc: isize, rc: WinRect, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FillRect(hdc, &rc, brush);
        DeleteObject(brush);
    }
}

fn border(hdc: isize, rc: WinRect, color: u32) {
    unsafe {
        let brush = CreateSolidBrush(color);
        FrameRect(hdc, &rc, brush);
        DeleteObject(brush);
    }
}

fn draw_text(hdc: isize, font: isize, color: u32, mut rc: WinRect, text: &str, flags: u32) {
    let w = wide(text);
    unsafe {
        let old = SelectObject(hdc, font);
        SetTextColor(hdc, color);
        DrawTextW(hdc, w.as_ptr(), (w.len() - 1) as i32, &mut rc, flags);
        SelectObject(hdc, old);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_button(
    hdc: isize,
    font: isize,
    rc: WinRect,
    label: &str,
    enabled: bool,
    hovered: bool,
    bg: u32,
    bg_hover: u32,
    fg: u32,
) {
    let fill_col = if !enabled {
        rgb(26, 31, 40)
    } else if hovered {
        bg_hover
    } else {
        bg
    };
    fill(hdc, rc, fill_col);
    if enabled && hovered {
        border(hdc, rc, COL_MINT);
    } else {
        border(hdc, rc, COL_BORDER);
    }
    draw_text(
        hdc,
        font,
        if enabled { fg } else { COL_MUTED },
        rc,
        label,
        DT_SINGLELINE | DT_VCENTER | DT_CENTER | DT_NOPREFIX,
    );
}

unsafe extern "system" fn wnd_proc(
    hwnd: isize,
    msg: u32,
    w_param: usize,
    l_param: isize,
) -> isize {
    match msg {
        WM_ERASEBKGND => 1,
        WM_APP_UPDATE | WM_TIMER => {
            if let Some(lock) = STATE.get() {
                if let Ok(mut st) = lock.lock() {
                    st.drain_worker();
                    if msg == WM_TIMER {
                        st.poll_resource_state_if_idle();
                    }
                }
            }
            unsafe {
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_MOUSEMOVE => {
            let x = (l_param & 0xFFFF) as i16 as i32;
            let y = ((l_param >> 16) & 0xFFFF) as i16 as i32;
            if let Some(lock) = STATE.get() {
                if let Ok(mut st) = lock.lock() {
                    if st.mouse.x != x || st.mouse.y != y {
                        st.mouse = Point { x, y };
                        unsafe {
                            InvalidateRect(hwnd, ptr::null(), 0);
                        }
                    }
                }
            }
            0
        }
        WM_SETCURSOR => {
            if (l_param & 0xFFFF) == 1 {
                let on_btn = STATE.get().and_then(|l| l.lock().ok()).is_some_and(|st| {
                    st.hitboxes
                        .iter()
                        .any(|h| h.enabled && h.rect.contains(st.mouse.x, st.mouse.y))
                });
                let id = if on_btn { IDC_HAND } else { IDC_ARROW };
                unsafe {
                    SetCursor(LoadCursorW(0, id as *const u16));
                }
                return 1;
            }
            unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) }
        }
        WM_LBUTTONDOWN => {
            let x = (l_param & 0xFFFF) as i16 as i32;
            let y = ((l_param >> 16) & 0xFFFF) as i16 as i32;
            let mut alert_missing_jar = false;
            if let Some(lock) = STATE.get() {
                if let Ok(mut st) = lock.lock() {
                    st.mouse = Point { x, y };
                    alert_missing_jar = st.on_click(x, y);
                }
            }
            unsafe {
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            if alert_missing_jar {
                show_missing_jar_alert(hwnd);
            }
            0
        }
        WM_LBUTTONUP => 0,
        WM_MOUSEWHEEL => {
            let delta = ((w_param >> 16) & 0xFFFF) as i16;
            if let Some(lock) = STATE.get() {
                if let Ok(mut st) = lock.lock() {
                    if delta < 0 {
                        st.scroll = st.scroll.saturating_add(1);
                    } else if delta > 0 {
                        st.scroll = st.scroll.saturating_sub(1);
                    }
                }
            }
            unsafe {
                InvalidateRect(hwnd, ptr::null(), 0);
            }
            0
        }
        WM_PAINT => {
            let mut ps: PaintStruct = unsafe { std::mem::zeroed() };
            let hdc = unsafe { BeginPaint(hwnd, &mut ps) };
            let mut rc = WinRect::default();
            unsafe {
                GetClientRect(hwnd, &mut rc);
            }
            let w = (rc.right - rc.left).max(1);
            let h = (rc.bottom - rc.top).max(1);
            let mem_dc = unsafe { CreateCompatibleDC(hdc) };
            let mem_bmp = unsafe { CreateCompatibleBitmap(hdc, w, h) };
            let old_bmp = unsafe { SelectObject(mem_dc, mem_bmp) };

            if let Some(lock) = STATE.get() {
                if let Ok(mut st) = lock.lock() {
                    st.paint(mem_dc, w, h);
                }
            }

            unsafe {
                BitBlt(hdc, 0, 0, w, h, mem_dc, 0, 0, SRCCOPY);
                SelectObject(mem_dc, old_bmp);
                DeleteObject(mem_bmp);
                DeleteDC(mem_dc);
                EndPaint(hwnd, &ps);
            }
            0
        }
        WM_DESTROY => {
            if let Some(lock) = STATE.get() {
                if let Ok(mut st) = lock.lock() {
                    if let Some(cancel) = st.auth_cancel.take() {
                        cancel.store(true, Ordering::Relaxed);
                    }
                }
            }
            unsafe {
                PostQuitMessage(0);
            }
            0
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, w_param, l_param) },
    }
}

#[cfg(test)]
fn test_app_state(path: PathBuf) -> AppState {
    AppState::new(1.0, path, AccountStore::default())
}

#[cfg(test)]
mod prompt_link_tests {
    use super::*;

    #[test]
    fn blocked_shell_can_reenter_launcher_without_stalling_prompt_updates() {
        let state = Arc::new(Mutex::new(test_app_state(PathBuf::new())));
        let shell_state = state.clone();
        let (reentered, observed) = mpsc::channel();
        let (resume_shell, stalled) = mpsc::channel();
        let window_thread = std::thread::current().id();
        {
            let mut ui = state.lock().unwrap();
            ui.auth_cancel = Some(Arc::new(AtomicBool::new(false)));
            ui.active_prompt = Some((DeviceCodePrompt {
                user_code: "test".into(),
                device_code: String::new(),
                verification_uri: "https://example.invalid".into(),
                interval_secs: 5,
                expires_in_secs: 30,
            }, Instant::now()));
            ui.present_prompt_link(move |_, _, _| {
                // A shell or clipboard handler may request the very same window state.
                assert_ne!(std::thread::current().id(), window_thread);
                let has_prompt = shell_state.lock().unwrap().active_prompt.is_some();
                reentered.send(has_prompt).unwrap();
                stalled.recv_timeout(Duration::from_secs(5)).unwrap();
                (false, true)
            });
        }
        assert!(observed.recv_timeout(Duration::from_secs(5)).unwrap());
        // While the external handler is still blocked, a timer can drain pending updates.
        state.lock().unwrap().drain_worker();
        resume_shell.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let mut ui = state.lock().unwrap();
            ui.drain_worker();
            if ui.status_color == COL_WARN {
                assert!(ui.status.contains("직접 입력"));
                return;
            }
            assert!(Instant::now() < deadline, "shell result was not delivered");
            drop(ui);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn main() {
    unsafe {
        SetProcessDPIAware();
    }
    let screen_dc = unsafe { GetDC(0) };
    let dpi = unsafe { GetDeviceCaps(screen_dc, 88) }.max(96); // LOGPIXELSX = 88
    unsafe {
        ReleaseDC(0, screen_dc);
    }
    let scale = dpi as f32 / 96.0;

    let store_path = ms_auth::default_store_path();
    let store = AccountStore::load_from(&store_path).unwrap_or_default();
    let instance = unsafe { GetModuleHandleW(ptr::null()) };
    let class_name = wide("Ferrum189LauncherWnd");
    let title = wide("Ferrum 1.8.9 클라이언트 런처");

    let wc = WndClassExW {
        cb_size: std::mem::size_of::<WndClassExW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfn_wnd_proc: wnd_proc,
        cb_cls_extra: 0,
        cb_wnd_extra: 0,
        h_instance: instance,
        h_icon: 0,
        h_cursor: unsafe { LoadCursorW(0, IDC_ARROW as *const u16) },
        hbr_background: 0,
        lpsz_menu_name: ptr::null(),
        lpsz_class_name: class_name.as_ptr(),
        h_icon_sm: 0,
    };
    unsafe {
        RegisterClassExW(&wc);
    }

    let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_VISIBLE;
    let client_w = (780.0 * scale).round() as i32;
    let client_h = (600.0 * scale).round() as i32;
    let mut win_rc = WinRect::new(0, 0, client_w, client_h);
    unsafe {
        AdjustWindowRectEx(&mut win_rc, style, 0, 0);
    }
    let win_w = win_rc.right - win_rc.left;
    let win_h = win_rc.bottom - win_rc.top;
    let sx = ((unsafe { GetSystemMetrics(0) } - win_w) / 2).max(20);
    let sy = ((unsafe { GetSystemMetrics(1) } - win_h) / 2).max(20);

    let _ = STATE.set(Mutex::new(AppState::new(scale, store_path, store)));

    let hwnd = unsafe {
        CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            sx,
            sy,
            win_w,
            win_h,
            0,
            0,
            instance,
            ptr::null_mut(),
        )
    };
    if hwnd == 0 {
        return;
    }

    if let Some(lock) = STATE.get() {
        if let Ok(mut st) = lock.lock() {
            st.hwnd = hwnd;
        }
    }

    let dark: i32 = 1;
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, 20, (&dark as *const i32).cast(), 4);
        SetTimer(hwnd, 1, 500, ptr::null());
        InvalidateRect(hwnd, ptr::null(), 0);
    }

    let mut msg: Msg = unsafe { std::mem::zeroed() };
    unsafe {
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

#[cfg(test)]
mod account_add_tests {
    use super::*;

    fn fixture() -> AuthenticatedAccount {
        AuthenticatedAccount {
            uuid: "0123456789abcdef0123456789abcdef".into(),
            username: "TestPlayer".into(),
            mc_access_token: "synthetic-mc-access".into(),
            ms_refresh_token: "synthetic-ms-refresh".into(),
            expires_at_epoch_secs: ms_auth::now_epoch_secs() + 3600,
        }
    }

    #[test]
    fn canceled_attempt_cannot_create_account_or_interrupt_retry() {
        let path = std::env::temp_dir().join(format!("ferrum-canceled-{}.json", rand::random::<u128>()));
        let mut state = test_app_state(path.clone());
        let old = Arc::new(AtomicBool::new(true));
        let retry = Arc::new(AtomicBool::new(false));
        state.auth_cancel = Some(retry.clone());
        post_event(&state.tx, 0, Some(&old), WorkerMsg::AuthSuccess(fixture()));
        post_event(&state.tx, 0, Some(&old), WorkerMsg::AuthError("stale".into()));
        state.drain_worker();
        assert!(Arc::ptr_eq(state.auth_cancel.as_ref().unwrap(), &retry));
        assert!(!state.saving_account && state.store.accounts.is_empty() && !path.exists());
        post_event(&state.tx, 0, Some(&retry), WorkerMsg::BrowserApproved);
        state.drain_worker();
        assert!(state.status.contains("Minecraft"));
    }

    #[test]
    fn successful_auth_returns_to_ready_only_after_background_save() {
        let path = std::env::temp_dir().join(format!("ferrum-auth-success-{}.json", rand::random::<u128>()));
        let mut state = test_app_state(path.clone());
        let current = Arc::new(AtomicBool::new(false));
        state.auth_cancel = Some(current.clone());
        post_event(&state.tx, 0, Some(&current), WorkerMsg::AuthSuccess(fixture()));
        let deadline = Instant::now() + Duration::from_secs(8);
        while state.auth_cancel.is_some() {
            state.drain_worker();
            assert!(Instant::now() < deadline, "account save did not finish");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(state.store.selected().unwrap().username, "TestPlayer");
        let mut persisted = AccountStore::load_from(&path).unwrap();
        let account = persisted.ensure_fresh_selected(&path).unwrap().unwrap();
        assert_eq!(account.username, "TestPlayer");
        assert_eq!(account.mc_access_token, fixture().mc_access_token);
        std::fs::remove_file(path).unwrap();
    }
}
