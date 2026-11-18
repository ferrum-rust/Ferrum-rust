use super::*;
use crate::{
    container::{Container, Kind},
    inventory::{ClickAction, SlotRef},
    ui::Screen,
};
pub(crate) const REMOTE_TRADER: u64 = u64::MAX;
fn crafting_result(sim: &Simulation, width: usize) -> Result<Option<Slot>> {
    crate::inventory::recipe(&sim.inventory.crafting, width)
        .map(|r| to_slot(&r.output))
        .transpose()
}

impl world_events::Window {
    fn update_anvil(&mut self,creative:bool) {
        if self.kind!="minecraft:anvil" {return}
        let plan=crate::vanilla::anvil::calculate(self.container.slots[0].as_ref(),self.container.slots[1].as_ref(),&self.anvil_name,creative);
        self.container.slots[2]=plan.output;
        self.container.anvil_materials=plan.materials;
        self.properties.insert(0,plan.cost.clamp(i32::from(i16::MIN),i32::from(i16::MAX)) as i16);
    }
    pub fn screen(&self) -> Screen {
        match self.kind.as_str() {
            "minecraft:furnace" => Screen::Furnace,
            "minecraft:crafting_table" => Screen::Crafting,
            "minecraft:villager" => Screen::Trading,
            _ => Screen::Chest,
        }
    }
    fn wire(&self, s: SlotRef) -> Option<usize> {
        match s {
            SlotRef::Player(i @ 0..=8) => Some(self.count + 27 + i),
            SlotRef::Player(i @ 9..=35) => Some(self.count + i - 9),
            SlotRef::Container(i) if i < self.count => Some(i),
            SlotRef::Craft(i) if self.screen() == Screen::Crafting && i < 9 => Some(i + 1),
            SlotRef::CraftOutput if self.screen() == Screen::Crafting => Some(0),
            _ => None,
        }
    }
    fn player_index(&self, n: usize) -> Option<usize> {
        match n.checked_sub(self.count)? {
            offset @ 0..=26 => Some(offset + 9),
            offset @ 27..=35 => Some(offset - 27),
            _ => None,
        }
    }
    pub fn view(&self) -> Container {
        let mut c = self.container.clone();
        let property = |k| self.properties.get(&k).copied().unwrap_or(0).max(0) as u32;
        c.burn = property(0);
        c.brew_time=self.properties.get(&0).copied().unwrap_or(0);
        c.burn_total = property(1);
        c.cook = property(2)
            .saturating_mul(200)
            .checked_div(property(3))
            .unwrap_or(0);
        c
    }
}
impl Remote {
    pub fn enchant_choice(&mut self,choice:u8,sim:&Simulation)->Result<()>{
        if let Some(w)=self.extra.window.as_ref().filter(|w|w.kind=="minecraft:enchanting_table" && choice<3){
            let cost=w.properties.get(&i16::from(choice)).copied().unwrap_or(0);
            let lapis=w.container.slots[1].as_ref().filter(|s|s.id==351 && s.meta==4).map_or(0,|s|s.count);
            if cost>0 && w.container.slots[0].is_some() && (sim.mode==Mode::Creative || sim.xp.level>=i32::from(cost) && lapis>choice){
                let id = w.id;
                self.queue_tick_command(Command::Enchant(id,choice))?;
            }
        }
        Ok(())
    }
    pub fn rename_anvil(&mut self,name:String)->Result<()>{
        if let Some(w)=self.extra.window.as_mut().filter(|w|w.kind=="minecraft:anvil" && w.container.slots[0].is_some()) {
            w.anvil_name.clone_from(&name);
            w.update_anvil(self.game_mode==1);
            self.queue_tick_command(Command::WindowPayload(crate::network::special_payload::Payload::Anvil(name)))?;
        }
        Ok(())
    }
    pub fn confirm_beacon(&mut self,primary:i16,secondary:i16)->Result<bool>{
        if let Some(w)=self.extra.window.as_ref().filter(|w|w.kind=="minecraft:beacon"){
            let level=w.properties.get(&0).copied().unwrap_or(0);
            let valid=matches!(primary,1|3) && level>=1 || matches!(primary,8|11) && level>=2 || primary==5 && level>=3;
            if valid && (secondary==0||level>=4 && (secondary==10 || secondary==primary)) && w.container.slots.first().is_some_and(Option::is_some){
                self.queue_tick_command(Command::WindowPayload(crate::network::special_payload::Payload::Beacon(primary.into(),secondary.into())))?;
                return Ok(true);
            }
        }
        Ok(false)
    }
    pub fn open_window(
        &mut self,
        id: u8,
        kind: String,
        title: String,
        slots: u8,
        sim: &mut Simulation,
    ) {
        let count = match kind.as_str() {
            "minecraft:furnace" | "minecraft:villager" | "minecraft:anvil" => 3,
            "minecraft:crafting_table" => 10,
            "minecraft:enchanting_table" => 2,
            "minecraft:brewing_stand" => 4,
            "minecraft:beacon" => 1,
            _ => usize::from(slots),
        };
        let mut container = Container::new(if kind == "minecraft:furnace" {
            Kind::Furnace
        } else {
            Kind::Chest
        });
        container.slots.resize(count, None);
        container.special=match kind.as_str(){
            "minecraft:brewing_stand"=>crate::container::Special::Brewing,
            "minecraft:hopper"=>crate::container::Special::Hopper,
            "minecraft:dispenser"|"minecraft:dropper"=>crate::container::Special::Dispenser,
            "EntityHorse"=>crate::container::Special::Horse{armor:true},
            "minecraft:anvil"=>crate::container::Special::Anvil,
            "minecraft:beacon"=>crate::container::Special::Beacon,
            "minecraft:enchanting_table"=>crate::container::Special::Enchant,
            _=>crate::container::Special::None,
        };
        self.extra.window = Some(world_events::Window {
            id,
            kind,
            title,
            container,
            count,
            slots: vec![None; count + 36],
            properties: Default::default(),
            anvil_name:String::new(),
            offers: vec![],
        });
        let w = self.extra.window.as_mut().unwrap();
        for i in 0..36 {
            let n = w.wire(SlotRef::Player(i)).unwrap();
            w.slots[n].clone_from(&self.slots[wire_index(i).unwrap()]);
        }
        if w.screen() == Screen::Crafting {
            sim.inventory.crafting.fill(None);
        }
        sim.trades.remove(&REMOTE_TRADER);
        sim.trading_with = None;
        self.extra.opens = Some(true);
        self.inventory_blocked = false;
        self.click_id = 0;
        self.cursor = None;
        sim.inventory.cursor = None;
        if self.extra.window.as_ref().unwrap().screen() == Screen::Trading {
            sim.trades.insert(
                REMOTE_TRADER,
                crate::villagers::Trader {
                    offers: vec![],
                    ..Default::default()
                },
            );
            sim.trading_with = Some(REMOTE_TRADER);
            sim.merchant_window = Default::default();
        }
    }
    pub(super) fn reset_window(&mut self, sim: &mut Simulation) -> Result<()> {
        let open = self.extra.window.take();
        sim.trading_with = None;
        sim.trades.remove(&REMOTE_TRADER);
        sim.merchant_window = Default::default();
        sim.inventory.cursor = None;
        self.cursor = None;
        self.inventory_blocked = false;
        sim.inventory.crafting.fill(None);
        if open.is_none() {
            self.slots[0..5].fill(None);
        } else {
            for i in 0..4 {
                sim.inventory.crafting[i] =
                    self.slots[i + 1].as_ref().map(from_slot).transpose()?;
            }
        }
        Ok(())
    }
    fn personal_slot(&mut self, n: usize, item: Option<Slot>, sim: &mut Simulation) -> Result<()> {
        if n >= 45 {
            return Ok(());
        }
        if let Some(i) = inventory_index(n) {
            let mut next = item.as_ref().map(from_slot).transpose()?;
            if let (Some(prev), Some(inc)) = (&sim.inventory.slots[i], &mut next) {
                if prev == inc {
                    inc.identity = prev.identity;
                }
            }
            if i == sim.inventory.selected && sim.inventory.slots[i] != next {
                self.using_item = false;
                self.use_ticks = 0;
            }
            sim.inventory.slots[i] = next;
            if let Some(w) = self.extra.window.as_mut() {
                if let Some(k) = w.wire(SlotRef::Player(i)) {
                    w.slots[k].clone_from(&item);
                }
            }
        } else if (1..=4).contains(&n)
            && self
                .extra
                .window
                .as_ref()
                .is_none_or(|w| w.screen() != Screen::Crafting)
        {
            sim.inventory.crafting[n - 1] = item.as_ref().map(from_slot).transpose()?;
            self.slots[0] = crafting_result(sim, 2)?;
        }
        self.slots[n] = item;
        Ok(())
    }
    fn open_slot(
        &mut self,
        id: u8,
        n: usize,
        item: Option<Slot>,
        sim: &mut Simulation,
    ) -> Result<()> {
        let Some(w) = self
            .extra
            .window
            .as_mut()
            .filter(|w| w.id == id && n < w.slots.len())
        else {
            return Ok(());
        };
        if let Some(i) = w.player_index(n) {
            let mut next = item.as_ref().map(from_slot).transpose()?;
            if let (Some(prev), Some(inc)) = (&sim.inventory.slots[i], &mut next) {
                if prev == inc {
                    inc.identity = prev.identity;
                }
            }
            if i == sim.inventory.selected && sim.inventory.slots[i] != next {
                self.using_item = false;
                self.use_ticks = 0;
            }
            sim.inventory.slots[i] = next;
            self.slots[wire_index(i).unwrap()].clone_from(&item);
        } else if w.screen() == Screen::Crafting && (1..=9).contains(&n) {
            sim.inventory.crafting[n - 1] = item.as_ref().map(from_slot).transpose()?;
            w.slots[0] = crafting_result(sim, 3)?;
        } else if w.screen() == Screen::Trading {
            if let Some(t) = sim.trades.get_mut(&REMOTE_TRADER) {
                let stack = item.as_ref().map(from_slot).transpose()?;
                if n < 2 {
                    sim.merchant_window.set_input(n, stack, t);
                } else if n == 2 {
                    sim.merchant_window.output = stack;
                }
            }
        }
        if n < w.count && matches!(w.screen(), Screen::Chest | Screen::Furnace) {
            w.container.slots[n] = item.as_ref().map(from_slot).transpose()?;
            if n<2 { w.update_anvil(self.game_mode==1); }
        }
        w.slots[n] = item;
        Ok(())
    }
    pub(super) fn window_items(
        &mut self,
        id: u8,
        items: Vec<Option<Slot>>,
        sim: &mut Simulation,
    ) -> Result<()> {
        if id != 0 && self.extra.window.as_ref().is_none_or(|w| w.id != id) {
            return Ok(());
        }
        for (n, item) in items.into_iter().enumerate() {
            if id == 0 {
                self.personal_slot(n, item, sim)?
            } else {
                self.open_slot(id, n, item, sim)?
            }
        }
        if id == self.extra.window.as_ref().map_or(0, |w| w.id) {
            self.inventory_blocked = false;
        }
        Ok(())
    }
    pub(super) fn set_window_slot(
        &mut self,
        id: i8,
        n: i16,
        item: Option<Slot>,
        sim: &mut Simulation,
    ) -> Result<()> {
        if id == -1 {
            sim.inventory.cursor = item.as_ref().map(from_slot).transpose()?;
            self.cursor = item;
            return Ok(());
        }
        let Ok(n) = usize::try_from(n) else {
            return Ok(());
        };
        if id == 0 {
            if self.extra.window.is_none() || (36..45).contains(&n) {
                self.personal_slot(n, item, sim)?
            }
        } else if id > 0 {
            self.open_slot(id as u8, n, item, sim)?;
        }
        Ok(())
    }
    pub fn sync_window(&mut self, sim: &mut Simulation) -> Result<()> {
        let Some(w) = self.extra.window.as_ref() else {
            return Ok(());
        };
        if w.screen() == Screen::Trading {
            if let Some(t) = sim.trades.get_mut(&REMOTE_TRADER) {
                t.offers = w
                    .offers
                    .iter()
                    .map(|o| {
                        Ok(crate::villagers::Offer {
                            cost: from_slot(&o.buy)?,
                            output: from_slot(&o.sell)?,
                            second: o.second.as_ref().map(from_slot).transpose()?,
                            uses: if o.disabled { o.limit } else { o.uses },
                            max_uses: o.limit,
                            reward_exp: false,
                            supported: true,
                        })
                    })
                    .collect::<Result<_>>()?;
                for i in 0..2 {
                    sim.merchant_window.set_input(
                        i,
                        w.slots
                            .get(i)
                            .and_then(Option::as_ref)
                            .map(from_slot)
                            .transpose()?,
                        t,
                    );
                }
                sim.merchant_window.output = w
                    .slots
                    .get(2)
                    .and_then(Option::as_ref)
                    .map(from_slot)
                    .transpose()?;
            }
        }
        Ok(())
    }
    pub fn current_window_view(&self) -> Option<Container> {
        Some(self.extra.window.as_ref()?.view())
    }
    pub fn window_click(
        &mut self,
        slot: SlotRef,
        action: ClickAction,
        sim: &mut Simulation,
    ) -> Result<()> {
        if !self.ready || self.inventory_blocked {
            return Ok(());
        }
        let w = self.extra.window.as_ref();
        let id = w.map_or(0, |w| w.id);
        let wire = |s: SlotRef| -> Option<i16> {
            if s == SlotRef::Outside {
                return Some(-999);
            }
            if let Some(w) = w {
                w.wire(s).and_then(|n| i16::try_from(n).ok())
            } else {
                match s {
                    SlotRef::Player(i) => wire_index(i).map(|v| v as i16),
                    SlotRef::Craft(i) if i < 4 => Some(i as i16 + 1),
                    SlotRef::CraftOutput => Some(0),
                    _ => None,
                }
            }
        };
        let Some(number) = wire(slot) else {
            return Ok(());
        };
        let previous = if number < 0
            || !matches!(action, ClickAction::Pickup { .. } | ClickAction::QuickMove)
        {
            None
        } else if let Some(w)=w.filter(|w|w.kind=="minecraft:anvil" && number==2){
            w.container.slots[2].as_ref().map(to_slot).transpose()?
        } else {
            w.map_or(&self.slots, |w| &w.slots)
                .get(number as usize)
                .cloned()
                .flatten()
        };
        let (button, mode, mut clicked) = match &action {
            ClickAction::Pickup { right } => (*right as u8, 0, previous),
            ClickAction::QuickMove => (0, 1, previous),
            ClickAction::Hotbar(i) if *i < 9 => (*i as u8, 2, None),
            ClickAction::Clone => (2, 3, None),
            ClickAction::Throw { all } => (*all as u8, 4, None),
            ClickAction::Collect { reverse } => (*reverse as u8, 6, None),
            ClickAction::Drag { mode, slots } if *mode < 3 => {
                let mut seq = vec![(-999, 4 * mode)];
                seq.extend(
                    slots
                        .iter()
                        .filter_map(|s| wire(*s).map(|n| (n, 1 + 4 * mode))),
                );
                seq.push((-999, 2 + 4 * mode));
                for (n, b) in seq {
                    let counter = if id == 0 {
                        &mut self.personal_click_id
                    } else {
                        &mut self.click_id
                    };
                    *counter = counter.wrapping_add(1);
                    let action = *counter;
                    self.queue_tick_command(Command::ClickWindow {
                        window_id: id,
                        slot: n,
                        button: b,
                        mode: 5,
                        action,
                        clicked: None,
                    })?;
                }
                (0, 5, None)
            }
            _ => return Ok(()),
        };
        let w = self.extra.window.as_ref();
        let is_trade = w.is_some_and(|w| w.screen() == Screen::Trading);
        let crafting_width = if w.is_some_and(|w| w.screen() == Screen::Crafting) {
            3
        } else {
            2
        };
        let mut view = if w.is_some_and(|w| matches!(w.screen(), Screen::Chest | Screen::Furnace)) {
            self.current_window_view()
        } else {
            None
        };
        let player_before = sim.inventory.slots.clone();
        let creative = sim.mode == Mode::Creative;
        if let (Some(c),Some(w))=(view.as_mut(),w){
            if c.special==crate::container::Special::Anvil{
                let cost=i32::from(w.properties.get(&0).copied().unwrap_or(0));
                c.anvil_locked=cost<=0 || !creative && sim.xp.level<cost;
            }
        }
        let transferred = if is_trade {
            sim.trades.get_mut(&REMOTE_TRADER).is_some_and(|t| {
                crate::container::predict_merchant_click(
                    &mut sim.inventory,
                    &mut sim.merchant_window,
                    t,
                    slot,
                    action,
                    creative,
                )
                .1
            })
        } else {
            crate::container::predict_window_click(
                &mut sim.inventory,
                view.as_mut(),
                slot,
                action,
                creative,
                crafting_width,
            )
            .1
        };
        self.cursor = sim.inventory.cursor.as_ref().map(to_slot).transpose()?;
        let as_wire = |v: Option<&Stack>| -> Result<Option<Slot>> { v.map(to_slot).transpose() };
        if let Some(w) = self.extra.window.as_mut() {
            if let Some(c) = view {
                let inputs_changed=w.kind=="minecraft:anvil" && w.container.slots[..2]!=c.slots[..2];
                for (n, value) in c.slots.iter().enumerate() {
                    if w.container.slots[n] != *value {
                        w.slots[n] = as_wire(value.as_ref())?;
                    }
                }
                w.container = c;
                if inputs_changed {w.update_anvil(creative);}
            }
            if is_trade {
                for (s, v) in w.slots.iter_mut().zip(
                    sim.merchant_window
                        .inputs()
                        .iter()
                        .chain([&sim.merchant_window.output]),
                ) {
                    *s = as_wire(v.as_ref())?;
                }
            }
            if crafting_width == 3 {
                w.slots[0] = crafting_result(sim, 3)?;
                for i in 0..9 {
                    if let Some(s) = w.slots.get_mut(i + 1) {
                        *s = as_wire(sim.inventory.crafting.get(i).and_then(Option::as_ref))?;
                    }
                }
            }
        }
        for (i, v) in sim.inventory.slots.iter().enumerate() {
            if player_before[i] != *v {
                let raw = as_wire(v.as_ref())?;
                if let Some(w) = self.extra.window.as_mut() {
                    if let Some(n) = w.wire(SlotRef::Player(i)) {
                        w.slots[n].clone_from(&raw);
                    }
                }
                self.slots[wire_index(i).unwrap()] = raw;
            }
        }
        if id == 0 {
            for i in 0..4 {
                self.slots[i + 1] = as_wire(sim.inventory.crafting[i].as_ref())?;
            }
            self.slots[0] = crafting_result(sim, 2)?;
        }
        if mode == 1 && !transferred {
            clicked = None;
        }
        if mode != 5 {
            let counter = if id == 0 {
                &mut self.personal_click_id
            } else {
                &mut self.click_id
            };
            *counter = counter.wrapping_add(1);
            let action = *counter;
            self.queue_tick_command(Command::ClickWindow {
                window_id: id,
                slot: number,
                button,
                mode,
                action,
                clicked,
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "remote_window_oracle_tests.rs"]
mod oracle_tests;
