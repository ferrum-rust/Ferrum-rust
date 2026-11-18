use super::*;
use crate::inventory::Inventory;
use crate::keybindings::{self, Binding};
use ui::creative::{self as menu, Hit};

impl App {
    fn creative_hit(&self) -> Option<Hit> {
        self.ui_frame
            .buttons
            .iter()
            .rev()
            .find(|b| b.rect.contains(self.gui.cursor))
            .and_then(|b| {
                if let Action::Creative(hit) = b.action {
                    Some(hit)
                } else {
                    None
                }
            })
    }
    fn creative_select(&mut self, tab: usize) {
        self.inventory_drag = None;
        self.gui.creative.select(
            tab,
            &self.resources,
            &self.settings.language,
            self.gui.advanced,
        );
    }
    pub(super) fn creative_text(&mut self, text: &str) {
        if self.gui.creative.tab != menu::SEARCH
            || self.control_down()
            || !text.chars().any(|c| c >= ' ' && c != '\u{7f}')
        {
            return;
        }
        if std::mem::take(&mut self.gui.creative.clear_search) {
            self.gui.creative.search = Default::default();
        }
        self.gui.creative.search.insert(text, 15);
        self.gui
            .creative
            .refresh(&self.resources, &self.settings.language, self.gui.advanced);
    }
    fn creative_apply(
        &mut self,
        change: impl FnOnce(&mut Inventory) -> Vec<Stack>,
        clear_all: bool,
    ) -> Result<()> {
        let Some(g) = &mut self.game else {
            return Ok(());
        };
        let previous = g.sim.inventory.slots.clone();
        let dropped = change(&mut g.sim.inventory);
        if let Some(remote) = &mut g.remote {
            if clear_all {
                for wire in 0..45 {
                    remote.creative_slot(wire, None)?;
                }
            } else {
                for (i, (before, after)) in previous.iter().zip(&g.sim.inventory.slots).enumerate()
                {
                    if before != after {
                        if let Some(wire) = crate::remote::wire_index(i) {
                            remote.creative_slot(wire as i16, after.as_ref())?;
                        }
                    }
                }
            }
            remote.creative_cursor(g.sim.inventory.cursor.as_ref())?;
            for stack in &dropped {
                remote.creative_slot(-1, Some(stack))?;
            }
        } else {
            for stack in dropped {
                g.sim.throw_local_stack(&g.player,stack,gameplay::ThrowSource::CreativeWindow);
            }
        }
        Ok(())
    }
    pub(super) fn creative_player_click(
        &mut self,
        slot: SlotRef,
        action: ClickAction,
    ) -> Result<()> {
        let tab = self.gui.creative.tab;
        self.gui.creative.clear_search = true;
        self.creative_apply(|inv| menu::player_click(inv, tab, slot, action), false)
    }
    fn creative_cell(
        &mut self,
        cell: Option<usize>,
        action: ClickAction,
        right: bool,
    ) -> Result<()> {
        self.gui.creative.clear_search = true;
        self.creative_apply(
            |inv| {
                menu::catalog_click(inv, cell, action, right)
                    .into_iter()
                    .collect()
            },
            false,
        )
    }
    pub(super) fn creative_mouse(&mut self, button: MouseButton, pressed: bool) -> Result<bool> {
        let hit = self.creative_hit();
        if !pressed && button == MouseButton::Left {
            self.gui.creative.dragging = false;
            if let Some(Hit::Tab(tab)) = hit {
                self.creative_select(tab);
                return Ok(true);
            }
        }
        let Some(hit) = hit else { return Ok(false) };
        if !pressed {
            return Ok(self.inventory_drag.is_none());
        }
        let pick = self.settings.bindings.code(Binding::Pick) == keybindings::mouse_code(button);
        let right = button == MouseButton::Right;
        match hit {
            Hit::Tab(_) => {
                if right {
                    self.creative_player_click(
                        SlotRef::Outside,
                        ClickAction::Pickup { right: true },
                    )?;
                }
                return Ok(true);
            }
            Hit::Cell(i) => {
                if button == MouseButton::Left || right {
                    self.creative_cell(i, self.pickup_action(right), right)?;
                } else if pick {
                    self.creative_cell(i, ClickAction::Clone, false)?;
                }
            }
            Hit::Scroll if button == MouseButton::Left => {
                self.gui.creative.dragging = self.gui.creative.list().len() > 45;
                self.gui
                    .creative
                    .drag(self.gui.cursor[1], self.ui_frame.size[1]);
            }
            Hit::Bin if matches!(button, MouseButton::Left | MouseButton::Right) || pick => {
                let all = matches!(self.pickup_action(right), ClickAction::QuickMove);
                self.creative_apply(
                    |inv| {
                        menu::discard(inv, all);
                        vec![]
                    },
                    all,
                )?;
            }
            Hit::Search | Hit::Scroll | Hit::Bin => {}
        }
        Ok(true)
    }
    pub(super) fn creative_key(&mut self, key: KeyCode, text: Option<&str>) -> Result<bool> {
        let binding =
            keybindings::key_code(key).and_then(|code| self.settings.bindings.action_for(code));
        let hit = self.creative_hit();
        let cell = if let Some(Hit::Cell(i)) = hit {
            Some(i)
        } else {
            None
        };
        let slot = self.hovered_slot();
        let search = self.gui.creative.tab == menu::SEARCH;
        if !search && binding == Some(Binding::Chat) {
            self.creative_select(menu::SEARCH);
            return Ok(true);
        }
        if search && std::mem::take(&mut self.gui.creative.clear_search) {
            self.gui.creative.search = Default::default();
            self.gui
                .creative
                .refresh(&self.resources, &self.settings.language, self.gui.advanced);
        }
        if let Some(h) = binding.and_then(Binding::hotbar) {
            if self
                .game
                .as_ref()
                .is_some_and(|g| g.sim.inventory.cursor.is_none())
            {
                if let Some(i) = cell {
                    self.creative_cell(i, ClickAction::Hotbar(h), false)?;
                    return Ok(true);
                }
                if let Some(target) = slot {
                    self.creative_player_click(target, ClickAction::Hotbar(h))?;
                    return Ok(true);
                }
            }
            if !search {
                return Ok(true);
            }
        }
        if search {
            let ctrl = self.control_down();
            let shift =
                self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight);
            if matches!(
                key,
                KeyCode::Delete
                    | KeyCode::Backspace
                    | KeyCode::Home
                    | KeyCode::End
                    | KeyCode::ArrowLeft
                    | KeyCode::ArrowRight
            ) || (ctrl
                && matches!(
                    key,
                    KeyCode::KeyA | KeyCode::KeyC | KeyCode::KeyX | KeyCode::KeyV
                ))
            {
                self.gui.creative.search.key(key, shift, ctrl, 15);
                self.gui.creative.refresh(
                    &self.resources,
                    &self.settings.language,
                    self.gui.advanced,
                );
                return Ok(true);
            }
            if let Some(text) = text.filter(|v| v.chars().any(|c| c >= ' ' && c != '\u{7f}')) {
                self.creative_text(text);
                return Ok(true);
            }
        }
        if let Some(i) = cell {
            match binding {
                Some(Binding::Drop) => {
                    self.creative_cell(
                        i,
                        ClickAction::Throw {
                            all: self.control_down(),
                        },
                        false,
                    )?;
                    return Ok(true);
                }
                Some(Binding::Pick) => {
                    self.creative_cell(i, ClickAction::Clone, false)?;
                    return Ok(true);
                }
                _ => {}
            }
        } else if binding == Some(Binding::Pick) {
            if let Some(target) = slot {
                self.creative_player_click(target, ClickAction::Clone)?;
                return Ok(true);
            }
        }
        Ok(false)
    }
}
