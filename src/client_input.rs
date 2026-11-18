use super::*;
use crate::keybindings::Binding;
use crate::ui::Rect;

pub(super) fn menu_cursor(position: [f64; 2], size: [u32; 2], scale: f32) -> [f32; 2] {
    let [width, height] = size.map(|v| v.max(1) as f64);
    let logical_width = (width / f64::from(scale)).ceil();
    let logical_height = (height / f64::from(scale)).ceil();
    // LWJGL reports integer pixels from the bottom left. GuiScreen scales those
    // integer coordinates before reversing the Y axis.
    let x = (position[0].floor() * logical_width / width).floor();
    let y = ((height - 1. - position[1].floor()) * logical_height / height).floor();
    [x as f32, (logical_height - 1. - y) as f32]
}

#[derive(Clone, Copy)]
pub(super) enum Drag {
    Sensitivity(Rect),
    Option(ui::options::Slider, Rect),
    List { y: f32, scroll: i32, factor: f32 },
}
#[derive(Default)]
pub(super) struct SmoothCamera {
    pub enabled: bool,
    pub pending: [f32; 2],
    previous: f32,
    filters: [[f32; 3]; 2],
    output: [f32; 2],
}
impl SmoothCamera {
    pub fn reset_filter(&mut self) {
        *self = Self {
            enabled: self.enabled,
            ..Default::default()
        };
    }
    pub fn tick(&mut self, sensitivity: f32) {
        if !self.enabled {
            return;
        }
        self.previous = 0.;
        let scale = (sensitivity * 0.6 + 0.2).powi(3) * 8. * 0.05;
        for i in 0..2 {
            let [acc, applied, last] = &mut self.filters[i];
            *acc += std::mem::take(&mut self.pending[i]);
            let mut delta = (*acc - *applied) * scale;
            *last += (delta - *last) * 0.5;
            if (delta > 0. && delta > *last) || (delta < 0. && delta < *last) {
                delta = *last;
            }
            *applied += delta;
            self.output[i] = delta;
        }
    }
    pub fn sample(&mut self, partial: f32) -> [f32; 2] {
        let delta = (partial - self.previous).max(0.);
        self.previous = partial;
        self.output.map(|v| v * delta)
    }
}

fn movement_key(binding: Binding) -> Option<KeyCode> {
    Some(match binding {
        Binding::Forward => KeyCode::KeyW,
        Binding::Back => KeyCode::KeyS,
        Binding::Left => KeyCode::KeyA,
        Binding::Right => KeyCode::KeyD,
        Binding::Jump => KeyCode::Space,
        Binding::Sneak => KeyCode::ShiftLeft,
        Binding::Sprint => KeyCode::ControlLeft,
        _ => return None,
    })
}

impl App {
    pub(super) fn send_anvil_name(&mut self)->Result<()> {
        if let (Some(r),Some(name))=(self.game.as_mut().and_then(|g|g.remote.as_mut()),self.gui.special.rename()){r.rename_anvil(name)?;}
        Ok(())
    }
    pub(super) fn open_chat(&mut self, command: bool) {
        if self.settings.menu.chat_visibility == 2 {
            return;
        }
        self.gui.chat.open(command);
        self.open(Screen::Chat);
    }
    pub(super) fn send_chat_message(&mut self,message:String)->Result<()> {
        let mut units=0;
        let message:String=message.chars().take_while(|ch|{units+=ch.len_utf16();units<=100}).collect();
        if message.chars().any(|c|c<' ' || c=='\u{7f}' || c=='§') {return Ok(())}
        if !message.is_empty() {
            if let Some(game) = self.game.as_mut() {
                if let Some(remote) = &game.remote {
                    remote.client.send(crate::network::Command::Chat(message))?;
                } else if message.starts_with('/') {
                    let output = gameplay::local_commands::execute(&mut game.sim,&mut game.player,&mut game.world,&self.resources,&game.options,&message);
                    self.gui.spectator.set_spectator(game.player.is_spectator());
                    for p in output.changed {game.mark(p);}
                    if !output.sounds.is_empty() {
                        let eye = glam::DVec3::from_array(game.player.precise_position()) + glam::DVec3::Y * 1.62;
                        self.audio.listener(eye, game.player.yaw);
                        for (name,pos,volume,pitch) in output.sounds {self.audio.play_world(&self.resources,&name,volume,pitch,pos);}
                    }
                    for reply in output.messages {
                        self.gui.chat.receive(&reply.to_string(),1,&self.resources,&self.settings.language);
                    }
                } else if self.settings.menu.chat_visibility == 0 {
                    if let Some(host)=&game.sim.lan {host.host_say(&game.options.username,&message);}
                    self.gui.chat.push(format!("<{}> {}", game.options.username, message), false);
                } else {
                    self.gui.chat.push(
                        self.resources
                            .tr(&self.settings.language, "chat.cannotSend"),
                        true,
                    );
                }
            }
        }
        Ok(())
    }
    fn show_stream_unavailable(&mut self) {
        self.open(Screen::OptionPage(ui::options::Page::Broadcast));
    }
    pub(super) fn visit_link(url: &str) -> Result<()> {
        anyhow::ensure!(
            (url.starts_with("https://") || url.starts_with("http://"))
                && !url.contains(char::is_control),
            "Unsupported link"
        );
        #[cfg(windows)]
        std::process::Command::new("rundll32.exe")
            .args(["url.dll,FileProtocolHandler", url])
            .spawn()?;
        Ok(())
    }
    pub(super) fn keyboard_input(
        &mut self,
        physical: PhysicalKey,
        pressed: bool,
        repeat: bool,
        text: Option<&str>,
    ) -> Result<()> {
        if pressed && self.gui.screen==Screen::Creative {
            if let PhysicalKey::Code(key)=physical {if self.creative_key(key,text)?{return Ok(())}}
        }
        let previous_screen = self.gui.screen;
        let key_code = match physical {
            PhysicalKey::Code(key) => crate::keybindings::key_code(key),
            _ => None,
        };
        // LWJGL Keyboard.KEY_NONE events carry the character instead of a scancode.
        let character_code = || {
            text?
                .encode_utf16()
                .next()
                .filter(|&c| c > 0)
                .map(|c| i32::from(c) + 256)
        };
        if pressed
            && !repeat
            && self.gui.screen == Screen::Controls
            && self.gui.editing_binding.is_some()
        {
            let assigned = if physical == PhysicalKey::Code(KeyCode::Escape) {
                Some(0)
            } else {
                key_code.or_else(character_code)
            };
            if let Some(code) = assigned {
                self.finish_rebinding(code)?;
            }
            return Ok(());
        }
        if key_code.is_none() && !repeat {
            let binding = if pressed {
                character_code()
                    .and_then(|code| self.settings.bindings.action_for(code))
                    .inspect(|binding| {
                        self.character_keys.insert(physical, *binding);
                    })
            } else {
                // Key-up events normally carry no text.
                self.character_keys.remove(&physical)
            };
            if let Some(binding) = binding {
                self.bound_input(binding, pressed)?;
            }
        }
        if let PhysicalKey::Code(key) = physical {
            self.key(key, pressed, repeat)?;
        }
        if pressed && previous_screen == self.gui.screen {
            if let Some(text) = text {
                self.text(text);
            }
        }
        Ok(())
    }
    pub(super) fn screen_key(&mut self, key: KeyCode) -> Result<bool> {
        let shift =
            self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
        let ctrl = self.control_down();
        if self.gui.screen == Screen::Video {
            let alt =
                self.keys.contains(&KeyCode::AltLeft) || self.keys.contains(&KeyCode::AltRight);
            let effect =
                ui::sodium_video::key_down(&mut self.gui, &self.settings, key, shift, ctrl, alt);
            self.apply_video_effect(effect)?;
            return Ok(true);
        }
        if self.gui.screen == Screen::Create {
            if let Some(action) = self.gui.create.key(key,shift,ctrl) { self.action(action,false)?; }
            return Ok(true);
        }
        if matches!(self.gui.screen,Screen::Worlds|Screen::RenameWorld|Screen::DeleteWorld) {
            if let Some(action) = self.gui.world_list.key(self.gui.screen,key,shift,ctrl) {
                self.action(action,false)?;
                return Ok(true);
            }
            if self.gui.screen == Screen::RenameWorld {return Ok(true);}
        }
        if self.gui.screen == Screen::Multiplayer {
            if let Some(cmd) = self
                .gui
                .multiplayer
                .key(key, shift, ctrl, self.ui_frame.size[1])
            {
                self.action(Action::Server(cmd), false)?;
                return Ok(true);
            }
        }
        if !matches!(self.gui.screen, Screen::Chat | Screen::Sleeping) {
            return Ok(false);
        }
        let sleeping = self.gui.screen == Screen::Sleeping;
        let chat = &mut self.gui.chat;
        if key != KeyCode::Tab {chat.cancel_completion();}
        match key {
            KeyCode::Escape if sleeping => self.action(Action::LeaveBed, false)?,
            KeyCode::Escape => self.open(Screen::Playing),
            KeyCode::Enter | KeyCode::NumpadEnter => {
                let message = chat.submit();
                self.send_chat_message(message)?;
                if !sleeping { self.open(Screen::Playing); }
            }
            KeyCode::ArrowUp => chat.recall(-1),
            KeyCode::ArrowDown => chat.recall(1),
            KeyCode::Tab => {
                if let (Some(prefix),Some(game))=(chat.complete(),self.game.as_mut()) {
                    if let Some(remote)=&game.remote {
                        let target=remote.pick(&game.world,&game.player,&game.sim).and_then(|h|h.block()).map(|h|h.position.to_array());
                        remote.client.send(crate::network::Command::TabComplete{input:prefix,target})?;
                    } else {
                        chat.complete_with(gameplay::local_commands::completions(&mut game.sim,&mut game.player,&mut game.world,&self.resources,&game.options,&prefix));
                    }
                }
            }
            KeyCode::PageUp | KeyCode::PageDown => {
                let shown=((self.settings.menu.chat_height_focused*160.+20.).floor()/9.) as isize;
                chat.wheel(if key==KeyCode::PageUp {shown-1} else {1-shown});
            }
            KeyCode::ArrowLeft | KeyCode::ArrowRight => {
                chat.move_by(key == KeyCode::ArrowRight, shift, ctrl)
            }
            KeyCode::Backspace | KeyCode::Delete => chat.delete(key == KeyCode::Delete, ctrl),
            KeyCode::Home | KeyCode::End => {
                chat.caret = if key == KeyCode::Home {
                    0
                } else {
                    chat.input.len()
                };
                if !shift {
                    chat.anchor = chat.caret
                }
            }
            KeyCode::KeyA if ctrl => {
                chat.caret = chat.input.len();
                chat.anchor = 0;
            }
            KeyCode::KeyC if ctrl => crate::clipboard::set(chat.selected()),
            KeyCode::KeyX if ctrl => {
                crate::clipboard::set(chat.selected());
                chat.insert("");
            }
            KeyCode::KeyV if ctrl => chat.insert(&crate::clipboard::get()),
            _ => return Ok(false),
        }
        Ok(true)
    }
    pub(super) fn control_action(&mut self, cmd: crate::keybindings::ControlAction) -> Result<()> {
        use crate::keybindings::ControlAction as C;
        match cmd {
            C::Edit(b) => {
                self.gui.editing_binding = Some(b);
                return Ok(());
            }
            C::Reset(b) => self.settings.bindings.reset(b),
            C::ResetAll => self.settings.bindings.reset_all(),
            C::InvertMouse => self.settings.invert_mouse = !self.settings.invert_mouse,
            C::Touchscreen => self.settings.touchscreen = !self.settings.touchscreen,
            C::Sensitivity => return Ok(()),
        }
        self.settings.save_to(&self.settings_path)
    }
    pub(super) fn begin_controls_scroll(&mut self) {
        let list = crate::keybindings::ControlsList::new(self.ui_frame.size);
        if let Some(factor) = list.drag_factor(self.gui.cursor) {
            self.drag_control = Some(Drag::List {
                y: self.gui.cursor[1],
                scroll: self.gui.scroll,
                factor,
            });
        }
    }
    pub(super) fn begin_menu_drag(&mut self, action: &Action) -> Result<()> {
        let rect = self
            .ui_frame
            .buttons
            .iter()
            .rev()
            .find(|b| b.enabled && b.rect.contains(self.gui.cursor))
            .map(|b| b.rect);
        if let Some(rect) = rect {
            self.drag_control = match action {
                Action::Option(ui::options::Command::Slider(slider)) => {
                    Some(Drag::Option(*slider, rect))
                }
                Action::Controls(crate::keybindings::ControlAction::Sensitivity) => {
                    Some(Drag::Sensitivity(rect))
                }
                _ => None,
            };
        }
        self.drag_menu()
    }
    pub(super) fn drag_menu(&mut self) -> Result<()> {
        if ui::options::drag_scroll(&mut self.gui, &self.resources, self.ui_frame.size) {
            return Ok(());
        }
        if self.gui.screen == Screen::Multiplayer {
            self.gui
                .multiplayer
                .drag(self.gui.cursor, self.ui_frame.size);
        }
        match self.drag_control {
            Some(Drag::Sensitivity(r)) => {
                self.settings.sensitivity =
                    ((self.gui.cursor[0] - r.x - 4.) / (r.w - 8.)).clamp(0., 1.)
            }
            Some(Drag::Option(slider, r)) => {
                slider.set(&mut self.settings, r, self.gui.cursor[0]);
            }
            Some(Drag::List { y, scroll, factor }) => {
                self.gui.scroll = crate::keybindings::ControlsList::new(self.ui_frame.size)
                    .clamp(scroll as f32 + (self.gui.cursor[1] - y) * factor)
            }
            None => {}
        }
        Ok(())
    }
    pub(super) fn option_action(&mut self, cmd: ui::options::Command) -> Result<()> {
        use ui::options::Effect;
        if matches!(cmd, ui::options::Command::Slider(_)) {
            return Ok(());
        }
        match ui::options::activate(cmd, &mut self.gui, &mut self.settings) {
            Effect::None => {}
            Effect::CycleDifficulty => {
                if let Some(g) = &mut self.game {
                    if !g.sim.hardcore && !g.sim.difficulty_locked && g.remote.is_none() {
                        self.settings.difficulty = (self.settings.difficulty + 1) % 4;
                        g.sim.world_difficulty = Some(self.settings.difficulty);
                    }
                }
            }
            Effect::LockDifficulty => {
                if let Some(g) = &mut self.game {
                    if g.remote.is_none() {
                        g.sim.difficulty_locked = true;
                        g.sim.world_difficulty = Some(self.settings.difficulty);
                        g.save()?;
                    }
                }
            }
            Effect::ReloadPacks(names) => {
                if let Err(error) = self.reload_packs(&names) {
                    self.gui.status = format!(
                        "{}: {error}",
                        self.resources
                            .tr(&self.settings.language, "resourcePack.title")
                    );
                    return Ok(());
                }
                self.settings.menu.resource_packs = names;
                self.open(Screen::Options);
            }
            Effect::OpenPackFolder => {
                std::fs::create_dir_all("resourcepacks")?;
                #[cfg(windows)]
                std::process::Command::new("explorer.exe")
                    .arg(std::fs::canonicalize("resourcepacks")?)
                    .spawn()?;
            }
            Effect::OpenStreamHelp => Self::visit_link("https://account.mojang.com/migrate")?,
            Effect::CycleShader => {
                if let Some(r) = &mut self.renderer {
                    if let Err(error) = r.cycle_shader(&self.resources, self.game.is_some()) {
                        self.gui.status = error.to_string();
                    }
                }
            }
        }
        self.send_client_settings()?;
        self.settings.save_to(&self.settings_path)
    }
    pub(super) fn send_client_settings(&self) -> Result<()> {
        if let Some(remote) = self.game.as_ref().and_then(|g| g.remote.as_ref()) {
            remote
                .client
                .send(crate::network::Command::Settings((&self.settings).into()))?;
        }
        Ok(())
    }
    pub(super) fn reload_packs(&mut self, names: &[String]) -> Result<()> {
        anyhow::ensure!(!self.options.benchmark_surface_waits,
            "Resource-pack reload is unavailable during the explicit surface-wait benchmark; it would replace the collector and its associated Renderer/device, discarding its records");
        anyhow::ensure!(!self.options.benchmark_gpu_timestamps,
            "Resource-pack reload is unavailable during the explicit GPU timestamp benchmark; it would replace the captured device and discard its records");
        let stack = match &self.server_pack{
            Some(path)=>crate::resource_packs::Stack::with_server(names,path)?,
            None=>crate::resource_packs::Stack::load(names)?,
        };
        let resources = Arc::new(Resources::load_with_packs(stack.clone())?);
        let assets = Arc::new(Assets::load_with_packs(
            self.options.jar.as_ref().unwrap(),
            &stack,
        )?);
        let mut renderer = pollster::block_on(Renderer::new_with_gpu_stage_diagnostics(
            self.window.clone(),
            &assets,
            &resources,
            &self.settings,
            self.options.benchmark_adapter.as_deref(),
            self.options.benchmark_near_arena,
            self.options.benchmark_capture_transparency || self.options.benchmark_capture_near_opaque
                || self.options.benchmark_capture_uploads || self.options.benchmark_near_opaque_runs,
            self.options.benchmark_gpu_timestamps,
            self.options.benchmark_gpu_stages,
        ))?;
        renderer.configure_pre_acquire_visibility(self.options.benchmark_pre_acquire_visibility);
        renderer.configure_packed_frustum(self.options.benchmark_packed_frustum);
        renderer.configure_covered_lod_culling(self.options.benchmark_cull_covered_lod);
        renderer.configure_surface_wait_trace(self.options.benchmark_surface_waits)?;
        renderer.configure_transparency_capture(self.options.benchmark_capture_transparency);
        renderer.configure_near_opaque_shadow(self.options.benchmark_capture_near_opaque);
        renderer.configure_upload_capture(self.options.benchmark_capture_uploads,Some(self.started));
        renderer.configure_arena_upload_capture(self.options.benchmark_capture_arena_uploads,Some(self.started));
        renderer.configure_near_opaque_runs(self.options.benchmark_near_opaque_runs);
        if self.options.benchmark {
            renderer.configure_slow_frame_threshold(self.options.benchmark_slow_frame_ms.unwrap_or_default());
        }
        renderer.configure_transparency_sort(self.options.benchmark_transparency_sort.unwrap_or_default());
        renderer.configure_transparent_staging(self.options.benchmark_transparency_upload.unwrap_or_default())?;
        renderer.configure_translucent_mdi(self.options.benchmark_transparency_draw.unwrap_or_default())?;
        if let Some(previous) = &self.renderer {
            renderer.reload_shader_from(previous, &resources)?;
        }
        if let Some(game) = &mut self.game {
            // Let an in-flight lighting owner return; its old-pack meshes are
            // rejected by the asset stamp when the result arrives.
            game.uploads.clear();
            game.prepared.clear();
            game.dirty.extend(game.world.chunks.keys());
        }
        self.settings.locale_unicode = resources.unicode(&self.settings.language);
        self.resources = resources;
        self.assets = assets;
        self.renderer = Some(renderer);
        self.audio.clear_resources();
        Ok(())
    }
    pub(super) fn control_down(&self) -> bool {
        self.keys.contains(&KeyCode::ControlLeft) || self.keys.contains(&KeyCode::ControlRight)
    }
    pub(super) fn finish_rebinding(&mut self, code: i32) -> Result<()> {
        if let Some(binding) = self.gui.editing_binding.take() {
            self.settings.bindings.set(binding, code);
            self.settings.save_to(&self.settings_path)?;
        }
        Ok(())
    }
    pub(super) fn activate_spectator_slot(&mut self, slot: Option<usize>) -> Result<()> {
        let Some(game) = self.game.as_ref() else { return Ok(()) };
        self.gui.spectator.set_spectator(true);
        if let Some(target) = self.gui.spectator.activate(slot, &game.sim.presentation) {
            if let Some(remote) = &game.remote {
                remote.client.send(crate::network::Command::Spectate(target))?;
            }
        }
        Ok(())
    }
    pub(super) fn world_wheel(&mut self, y: f32) {
        if y == 0. { return; }
        let direction = y.signum();
        if matches!(self.gui.screen, Screen::Chat | Screen::Sleeping) {
            let shift = self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
            self.gui.chat.wheel(direction as isize * if shift { 1 } else { 7 });
            return;
        }
        let Some(game) = &mut self.game else { return };
        if game.player.is_spectator() {
            self.gui.spectator.set_spectator(true);
            if self.gui.spectator.active() {
                self.gui.spectator.wheel(direction, &game.sim.presentation);
            } else {
                game.player.fly_speed = (game.player.fly_speed + direction * 0.005).clamp(0., 0.2);
            }
        } else {
            game.sim.inventory.selected =
                (game.sim.inventory.selected as i32 - direction as i32).rem_euclid(9) as usize;
        }
    }
    pub(super) fn bound_input(&mut self, binding: Binding, pressed: bool) -> Result<()> {
        if self.gui.screen == Screen::Playing {
            if self.game.as_ref().is_some_and(|g| g.player.is_spectator()) {
                if let Some(slot) = binding.hotbar() {
                    if pressed {
                        self.activate_spectator_slot(Some(slot))?;
                    }
                    return Ok(());
                }
                if matches!(binding, Binding::Pick | Binding::Drop) {
                    return Ok(());
                }
            }
            if let Some(key) = movement_key(binding) {
                if let Some(game) = &mut self.game {
                    if pressed {
                        game.player.keys.insert(key);
                    } else {
                        game.player.keys.remove(&key);
                    }
                }
                return Ok(());
            }
            if matches!(binding, Binding::Use | Binding::Attack | Binding::Pick) {
                return self.game_click(binding, pressed);
            }
        }
        if binding == Binding::PlayerList {
            self.gui.chat.tab_open = pressed
                && self.gui.screen == Screen::Playing
                && self.game.as_ref().is_some_and(|g| g.remote.is_some());
            return Ok(());
        }
        if !pressed {
            return Ok(());
        }
        match binding {
            Binding::Fullscreen => self.action(Action::Setting(Setting::Fullscreen), false)?,
            Binding::Screenshot => self.screenshot_pending = true,
            Binding::Perspective if self.gui.screen == Screen::Playing => {
                self.gui.perspective = (self.gui.perspective + 1) % 3
            }
            Binding::SmoothCamera if self.gui.screen == Screen::Playing => {
                self.smooth_camera.enabled = !self.smooth_camera.enabled;
                self.smooth_camera.reset_filter();
            }
            Binding::Chat | Binding::Command if self.gui.screen == Screen::Playing => {
                self.open_chat(binding == Binding::Command);
            }
            Binding::StreamStartStop => {
                self.show_stream_unavailable()
            }
            Binding::Inventory => {
                if self.gui.screen == Screen::Playing {
                    if let Some(remote)=self.game.as_mut().and_then(|g|g.remote.as_mut()) {
                        if remote.riding_any_horse() {
                            remote.queue_tick_command(crate::network::Command::EntityAction{action:crate::network::EntityAction::HorseInventory,parameter:0})?;
                            return Ok(());
                        }
                        remote.queue_tick_command(crate::network::Command::InventoryAchievement)?;
                    } else if let Some(g)=&mut self.game {
                        if let Some(id) = g.sim.ridden_horse {
                            let _ = g.sim.request_horse_window(id);
                            return Ok(());
                        }
                        g.sim.statistics.entry("achievement.openInventory".into()).or_insert(1);
                    }
                    let creative = self
                        .game
                        .as_ref()
                        .is_some_and(|g| g.sim.mode == Mode::Creative && g.sim.commands.player_mode!=Some(3));
                    self.open(if creative {
                        Screen::Creative
                    } else {
                        Screen::Inventory
                    });
                } else if matches!(
                    self.gui.screen,
                    Screen::Inventory
                        | Screen::Achievements
                        | Screen::Crafting
                        | Screen::Creative
                        | Screen::Chest
                        | Screen::Furnace
                        | Screen::Trading
                ) {
                    self.open(Screen::Playing);
                }
            }
            Binding::Drop if self.gui.screen == Screen::Playing => {
                let all = self.control_down();
                if let Some(g) = &mut self.game {
                    if let Some(remote) = &mut g.remote {
                        remote.queue_drop(all);
                        return Ok(());
                    }
                    g.sim.throw_selected(&g.player, all);
                }
            }
            Binding::Drop if self.has_inventory_screen() => {
                if let Some(slot) = self.hovered_slot() {
                    self.window_click(
                        slot,
                        ClickAction::Throw {
                            all: self.control_down(),
                        },
                    )?;
                }
            }

            _ => {
                if let Some(slot) = binding.hotbar() {
                    if self.gui.screen == Screen::Playing {
                        if let Some(game) = &mut self.game {
                            game.sim.inventory.selected = slot;
                        }
                    } else if self.has_inventory_screen() {
                        if let Some(target) = self.hovered_slot() {
                            self.window_click(target, ClickAction::Hotbar(slot))?;
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn game_click(&mut self, binding: Binding, pressed: bool) -> Result<()> {
        match binding {
            Binding::Attack => {
                self.left = pressed;
                if pressed {
                    if let Some(g) = &mut self.game {
                        g.sim.click_controller.queue_press();
                    }
                }
            }

            Binding::Use => {
                self.right = pressed;
                if pressed {
                    if let Some(remote) = self.game.as_mut().and_then(|g| g.remote.as_mut()) {
                        remote.queue_use_press();
                        return Ok(());
                    }
                }
                if pressed && self.game.as_ref().is_some_and(|g| g.remote.is_none()) {
                    use crate::vanilla::entity_pick::Hit;
                    let picked = self
                        .game
                        .as_ref()
                        .and_then(|g| g.sim.pick(&g.world, &g.player));
                    let merchant = self.game.as_ref().and_then(|g| match picked? {
                        Hit::Entity {
                            id: gameplay::LocalTarget::Mob(id),
                            pointed: true,
                            ..
                        } if g.sim.mobs.iter().any(|m| {
                            m.id == id
                                && m.kind == gameplay::MobKind::Villager
                                && m.health > 0.
                                && !g.sim.is_baby(id)
                        }) =>
                        {
                            Some(id)
                        }
                        _ => None,
                    });
                    let spawn_egg = self
                        .game
                        .as_ref()
                        .is_some_and(|g| g.sim.inventory.held().is_some_and(|s| s.id == 383));
                    if let Some(id) = merchant.filter(|_| !spawn_egg) {
                        let g = self.game.as_mut().unwrap();
                        if g.sim.begin_trading(id, &g.options.username)? {
                            self.open(Screen::Trading);
                            self.gui.trader = Some(id);
                            self.gui.trade_index = 0;
                        }
                        self.right = false;
                    }
                    let target = self.game.as_ref().and_then(|g| {
                        picked
                            .and_then(|h| h.block())
                            .map(|hit| (hit.position, g.world.get(hit.position) >> 4))
                    });
                    let pig_boost_priority = self.game.as_ref().is_some_and(|g| {
                        g.remote.is_none()
                            && g.sim.ridden_pig.is_some()
                            && g.sim.pig_boost_ticks == 0
                            && g.sim.inventory.held().is_some_and(|s| s.id == 398)
                    });
                    if let Some((pos, id)) = target.filter(|_| {
                        merchant.is_none()
                            && !pig_boost_priority
                            && !self.game.as_ref().is_some_and(|g| g.player.sneaking() && g.sim.inventory.held().is_some())
                    }) {
                        match id {
                            26 if self.game.as_ref().is_some_and(|g| g.sim.dimension == 0) => {
                                let g = self.game.as_mut().unwrap();
                                let (result, changed) =
                                    g.sim.try_bed(&mut g.world, &mut g.player, pos);
                                if let Some(p) = changed {
                                    g.mark(p);
                                }
                                if let Some(key) = result.notice() {
                                    g.sim.sleep_notice = Some((key, 100));
                                }
                                self.right = false;
                                if result == crate::vanilla::sleep::Attempt::Asleep {
                                    self.open(Screen::Sleeping);
                                }
                            }
                            64 | 71 | 193..=197 => {
                                let g = self.game.as_mut().unwrap();
                                let mut out = gameplay::Outcome::default();
                                if g.sim.commands.player_mode != Some(3) {
                                    g.sim.activate_circuit(&mut g.world, pos, &mut out);
                                    for p in out.changed {
                                        g.sim.block_ticks.notify(&g.world, p);
                                        g.mark(p);
                                    }
                                    self.audio.listener(g.player.precise_eye(1.0), g.player.yaw);
                                    for (sound, volume) in out.sounds {
                                        self.audio.play(&self.resources, sound, volume);
                                    }
                                    for (sound, volume, pitch) in out.pitched_sounds {
                                        self.audio.play_pitched(&self.resources, sound, volume, pitch);
                                    }
                                    for (sound, source, volume, pitch) in out.world_sounds {
                                        self.audio.play_world(&self.resources, sound, volume, pitch, source);
                                    }
                                }
                                self.right = false;
                            }
                            96 | 107 | 183..=187 => {
                                let g = self.game.as_mut().unwrap();
                                let state = g.world.get(pos) ^ 4;
                                if g.world.set(pos, state) {
                                    g.sim.block_ticks.notify(&g.world, pos);
                                    g.mark(pos);
                                }
                                self.right = false;
                            }
                            58 => self.open(Screen::Crafting),
                            54 | 146 | 61 | 62 | 154 => {
                                if gameplay::container_parts(
                                    &self.game.as_ref().unwrap().world,
                                    pos,
                                )
                                .is_none()
                                {
                                    self.right = false;
                                    return Ok(());
                                }
                                let g = self.game.as_mut().unwrap();
                                g.sim.live_container(&g.world, pos);
                                self.open(if matches!(id, 54 | 146 | 154) {
                                    Screen::Chest
                                } else {
                                    Screen::Furnace
                                });
                                self.gui.container = Some(pos);
                            }
                            122 => {}
                            _ => {}
                        }
                    }
                    if self.right && self.gui.screen == Screen::Playing {
                        let book=self.game.as_ref().and_then(|g| {
                            let mode=g.sim.commands.player_mode.unwrap_or(u8::from(g.sim.mode==gameplay::Mode::Creative));
                            if mode==3 || g.sim.click_controller.hitting || target.is_some_and(|(p,_)|ui::server_book::block_consumes(&g.world,p,mode,g.player.sneaking())) { return None; }
                            let mut book=ui::server_book::Book::from_stack(g.sim.inventory.held().as_ref()?)?;
                            book.author.clone_from(&g.options.username);
                            Some(book)
                        });
                        if let Some(book)=book {
                            self.gui.book=book;
                            self.open(Screen::Book);
                            self.right=false;
                        }
                    }
                    if self.right && self.gui.screen == Screen::Playing {
                        self.local_right_clicks = self.local_right_clicks.saturating_add(1);
                    }
                }
            }
            Binding::Pick if pressed => {
                if let Some(g) = &mut self.game {
                    let ctrl = self.keys.contains(&KeyCode::ControlLeft)
                        || self.keys.contains(&KeyCode::ControlRight);
                    if let Some(remote) = &mut g.remote {
                        remote.middle_click(&g.world, &g.player, &mut g.sim, ctrl)?;
                    } else {
                        g.sim.middle_click(&g.world, &g.player, ctrl);
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
