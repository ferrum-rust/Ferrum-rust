use super::*;
use crate::{
    gameplay::{Mode, Outcome},
    inventory::{ClickAction, Inventory, SlotRef, Stack},
    protocol::Slot,
    settings::Settings,
    vanilla::{
        mining,
        movement::{self, Aabb},
        ray_trace::{Hit, Kind},
    },
};
use glam::{DVec3, IVec3};
use serde::{Deserialize, Serialize};
#[path = "lan_guest_commands.rs"]
mod commands;

pub(super) const FIRST_GUEST: i32 = 0x60000000;
fn eye_height(sneaking: bool) -> f32 {
    if sneaking {
        1.62f32 - 0.08
    } else {
        1.62
    }
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct SavedGuest {
    inventory: Inventory,
    exhaustion: f32,
    position: Option<[f64; 3]>,
    random: Option<u64>,
    look: [f32; 2],
    sneaking: bool,
    profile: commands::Profile,
}
impl SavedGuest {
    pub(super) fn record_stat(&mut self, key: &str) {
        crate::gameplay::local_commands::award_pickup(&mut self.profile.statistics, key);
    }
    pub fn disconnected(&mut self, sim: &mut Simulation) {
        let p = DVec3::from(self.position.unwrap_or([0., 64., 0.]));
        let mut rng = crate::vanilla::random::JavaRandom::from_state(self.random.unwrap_or(189));
        for stack in self
            .inventory
            .cursor
            .take()
            .into_iter()
            .chain(self.inventory.crafting.iter_mut().filter_map(Option::take))
        {
            sim.throw_stack_from(
                stack,
                p,
                eye_height(self.sneaking),
                self.look,
                &mut rng,
                None,
            );
        }
        self.random = Some(rng.state());
    }
}
#[derive(Clone, PartialEq)]
pub(super) struct Actor {
    pub id: i32,
    pub uuid: [u8; 16],
    pub name: String,
    pub feet: DVec3,
    pub look: [f32; 2],
    pub ground: bool,
    pub bed: Option<IVec3>,
    pub flags: u8,
    pub mode: u8,
    pub hand: Option<Slot>,
}
impl Actor {
    pub fn new(id: i32, uuid: [u8; 16], name: &str, feet: [f64; 3]) -> Self {
        Self {
            id,
            uuid,
            name: name.to_owned(),
            feet: feet.into(),
            look: [0.; 2],
            ground: false,
            bed: None,
            flags: 0,
            mode: 0,
            hand: None,
        }
    }
}
pub(super) enum Action {
    Move(Option<DVec3>, Option<[f32; 2]>, bool),
    Dig(u8, IVec3),
    Place(IVec3, u8, [u8; 3]),
    Select(usize),
    Click(u8, i16, u8, i16, u8, Option<Slot>),
    Close,
    Confirm(u8, i16),
    Swing,
    CreativeSlot(i16, Option<Slot>),
    Flight(bool),
    Sneak(bool),
    RequestStats,
    OpenInventory,
    Command(String),
    Complete(String),
}
pub(super) struct Guest {
    pub actor: Actor,
    pub inputs: VecDeque<Action>,
    pub replies: VecDeque<(i32, Writer)>,
    mine: Option<(IVec3, u16, u64, bool)>,
    pub inventory: Vec<Option<Slot>>,
    pub cursor: Option<Slot>,
    pub revision: u64,
    pending: Option<i16>,
    drag: Option<(u8, Vec<SlotRef>)>,
    flying: bool,
    sneaking: bool,
    drop_delay: u16,
}
pub(super) struct PickupEvents {
    pub online: std::collections::HashSet<String>,
    pub collected: Vec<(u64, i32)>,
    pub stats: Vec<(String, String)>,
    pub sounds: Vec<(DVec3, f32, f32)>,
}
impl Guest {
    pub fn new(actor: Actor) -> Self {
        let flying = actor.mode == 3;
        Self {
            flying,
            sneaking: false,
            drop_delay: 0,
            actor,
            inputs: Default::default(),
            replies: Default::default(),
            mine: None,
            inventory: vec![],
            cursor: None,
            revision: 0,
            pending: None,
            drag: None,
        }
    }
    pub(super) fn send(&mut self, id: i32, w: Writer) {
        if self.replies.len() < 256 {
            self.replies.push_back((id, w));
        }
    }
    fn correction(&mut self, world: &World, p: IVec3) {
        let mut w = Writer::new();
        w.position(p.to_array()).varint(i32::from(world.get(p)));
        self.send(0x23, w);
    }
    fn warp(&mut self) {
        let p = self.actor.feet;
        let mut w = Writer::new();
        w.f64(p.x)
            .f64(p.y)
            .f64(p.z)
            .f32(self.actor.look[0])
            .f32(self.actor.look[1])
            .u8(0);
        self.send(8, w);
    }
    fn reach(&self, world: &World, sim: &Simulation, p: IVec3, place: bool) -> bool {
        let eye = self.actor.feet + if place { DVec3::ZERO } else { DVec3::Y * 1.5 };
        (0..256).contains(&p.y)
            && world.chunks.contains_key(&(p.x >> 4, p.z >> 4))
            && (p.as_dvec3() + DVec3::splat(0.5)).distance_squared(eye)
                < if place { 64. } else { 36.00000000001 }
            && sim
                .world_border
                .contains(p, crate::vanilla::click_border::now())
    }
    fn relative(&self, world: &World, bag: &Inventory, s: u16) -> f32 {
        mining::increment(
            s,
            bag.slots[bag.selected].as_ref(),
            mining::hardness(s),
            mining::Conditions {
                grounded: self.actor.ground,
                eyes_in_water: movement::eyes_in_water(world, self.actor.feet, false),
                aqua_affinity: bag.armor_enchantment(6) > 0,
                ..Default::default()
            },
        )
    }
    fn refresh(&mut self, inv: &Inventory) {
        let mut slots = vec![None; 45];
        let wire = |s: &Stack| crate::remote::to_slot(s).ok();
        for (i, s) in inv.slots.iter().enumerate() {
            if let Some(n) = crate::remote::wire_index(i) {
                slots[n] = s.as_ref().and_then(wire);
            }
        }
        for i in 0..4 {
            slots[1 + i] = inv.crafting[i].as_ref().and_then(wire);
        }
        slots[0] = crate::inventory::recipe(&inv.crafting, 2).and_then(|r| wire(&r.output));
        self.actor.hand = inv.held().as_ref().and_then(wire);
        let cursor = inv.cursor.as_ref().and_then(wire);
        if self.inventory != slots || self.cursor != cursor {
            self.inventory = slots;
            self.cursor = cursor;
            self.revision += 1;
        }
    }
    fn click(
        &mut self,
        sim: &mut Simulation,
        window: u8,
        n: i16,
        button: u8,
        transaction: i16,
        mode: u8,
        claimed: Option<Slot>,
    ) {
        if self.pending.is_some() {
            return;
        }
        let reference = match n {
            -999 => Some(SlotRef::Outside),
            0 => Some(SlotRef::CraftOutput),
            1..=4 => Some(SlotRef::Craft((n - 1) as usize)),
            5..=8 => Some(SlotRef::Player((44 - n) as usize)),
            9..=35 => Some(SlotRef::Player(n as usize)),
            36..=44 => Some(SlotRef::Player((n - 36) as usize)),
            _ => None,
        };
        let mut accepted = false;
        if let (0, Some(slot)) = (window, reference) {
            let action = match (mode, button) {
                (0, 0 | 1) => Some(ClickAction::Pickup { right: button == 1 }),
                (1, 0 | 1) => Some(ClickAction::QuickMove),
                (2, 0..=8) => Some(ClickAction::Hotbar(button as usize)),
                (3, 2) if self.actor.mode == 1 => Some(ClickAction::Clone),
                (4, 0 | 1) => Some(ClickAction::Throw { all: button == 1 }),
                (6, 0 | 1) => Some(ClickAction::Collect {
                    reverse: button == 1,
                }),
                (5, 0 | 4 | 8) if n == -999 && (button < 8 || self.actor.mode == 1) => {
                    self.drag = Some((button / 4, vec![]));
                    accepted = claimed.is_none();
                    None
                }
                (5, 1 | 5 | 9) if n >= 0 => {
                    if let Some((kind, slots)) = &mut self.drag {
                        if *kind == button / 4 && slots.len() < 45 && !slots.contains(&slot) {
                            slots.push(slot)
                        }
                    }
                    accepted = claimed.is_none();
                    None
                }
                (5, 2 | 6 | 10) if n == -999 => self
                    .drag
                    .take()
                    .filter(|(kind, _)| *kind == button / 4)
                    .map(|(mode, slots)| ClickAction::Drag { mode, slots }),
                _ => None,
            };
            if let Some(action) = action {
                let prior = match slot {
                    SlotRef::Player(i) => sim.inventory.slots[i].clone(),
                    SlotRef::Craft(i) => sim.inventory.crafting[i].clone(),
                    SlotRef::CraftOutput => {
                        crate::inventory::recipe(&sim.inventory.crafting, 2).map(|r| r.output)
                    }
                    _ => None,
                };
                let (drops, transferred) = crate::container::predict_window_click(
                    &mut sim.inventory,
                    None,
                    slot,
                    action,
                    self.actor.mode == 1,
                    2,
                );
                let prediction = if mode == 0 || mode == 1 && transferred {
                    prior
                } else {
                    None
                };
                accepted = prediction
                    .as_ref()
                    .and_then(|s| crate::remote::to_slot(s).ok())
                    == claimed;
                self.drop_items(sim, drops);
            }
        }
        let mut w = Writer::new();
        w.u8(window).i16(transaction).bool(accepted);
        if !accepted && window == 0 {
            self.pending = Some(transaction);
        }
        self.send(0x32, w);
        self.revision += 1;
    }
    fn drop_items(&self, sim: &mut Simulation, items: Vec<Stack>) {
        for s in items {
            self.throw_stack(sim, s, false, false);
        }
    }
    fn throw_stack(
        &self,
        sim: &mut Simulation,
        stack: Stack,
        q: bool,
        creative_slot: bool,
    ) -> bool {
        let mut rng = crate::vanilla::random::JavaRandom::from_state(sim.player_random_state);
        let item = sim.throw_stack_from(
            stack,
            self.actor.feet,
            eye_height(self.sneaking),
            self.actor.look,
            &mut rng,
            q.then_some(self.actor.name.as_str()),
        );
        let spawned = item.is_some();
        if let Some(item) = item.filter(|_| creative_slot) {
            item.age = 4800;
        }
        sim.player_random_state = rng.state();
        spawned
    }
    fn close_inventory(&mut self, sim: &mut Simulation) {
        let items = sim
            .inventory
            .cursor
            .take()
            .into_iter()
            .chain(sim.inventory.crafting.iter_mut().filter_map(Option::take))
            .collect();
        self.drop_items(sim, items);
        self.drag = None;
    }
    fn collect_at_previous_feet(
        &self,
        sim: &mut Simulation,
        saved: &SavedGuest,
        pickups: &mut PickupEvents,
        _host: &Player,
        out: &mut Outcome,
    ) -> bool {
        let mut dirty = false;
        let previous = sim
            .server_scene
            .actors
            .get(&self.actor.id)
            .map_or(self.actor.feet, |a| a.feet.as_dvec3());
        if self.actor.mode != 3 && saved.profile.alive() {
            let area = Aabb::player(self.actor.feet).expand(DVec3::new(1., 0.5, 1.));
            for item in &mut sim.drops {
                if !area.intersects(item.bounds) {
                    continue;
                }
                let online = item
                    .thrower
                    .as_ref()
                    .is_some_and(|name| pickups.online.contains(name));
                if let Ok(take) = item.pickup(
                    &mut sim.inventory,
                    &self.actor.name,
                    self.actor.mode == 1,
                    online,
                ) {
                    dirty |= take.removed_count != 0;
                    if let Some(count) = take.announced_count {
                        pickups.collected.push((item.id, self.actor.id));
                        sim.pickup_fx.item_count(
                            item,
                            count,
                            crate::gameplay::item_pickup_fx::Collector::Actor {
                                id: self.actor.id,
                                previous,
                                feet: self.actor.feet,
                            },
                        );
                    }
                    pickups.stats.extend(take.stats);
                    if let Some((volume, pitch)) = take.sound {
                        pickups.sounds.push((self.actor.feet, volume, pitch));
                        out.world_sounds
                            .push(("random.pop", self.actor.feet, volume, pitch));
                    }
                }
            }
        }
        sim.drops.retain(|d| !d.dead);
        dirty
    }
    pub fn tick(
        &mut self,
        world: &mut World,
        sim: &mut Simulation,
        settings: &Settings,
        resources: &crate::resources::Resources,
        options: &Options,
        allow_commands: bool,
        out: &mut Outcome,
        host: &Player,
        saved: &mut SavedGuest,
        pickups: &mut PickupEvents,
    ) {
        use std::mem::{replace, swap};
        if self.inventory.is_empty() {
            if let Some(at) = saved.position.filter(|p| {
                p.iter().all(|n| n.is_finite()) && world.contains(DVec3::from(*p).as_vec3())
            }) {
                self.actor.feet = at.into();
                self.warp();
            }
            saved.inventory.sanitize();
        }
        swap(&mut sim.inventory, &mut saved.inventory);
        swap(&mut sim.exhaustion, &mut saved.exhaustion);
        let host_mode = replace(
            &mut sim.mode,
            if self.actor.mode == 1 {
                Mode::Creative
            } else {
                Mode::Survival
            },
        );
        let host_game_type = sim.commands.player_mode.replace(self.actor.mode);
        self.drop_delay = self.drop_delay.saturating_sub(1);
        let host_delay = sim.place_ticks;
        let host_rand = replace(&mut sim.player_random_state, saved.random.unwrap_or(189));
        let mut player = Player::new(self.actor.feet.as_vec3());
        let mut allowance = 10.;
        let mut dirty = self.inventory.is_empty();
        while let Some(event) = self.inputs.pop_front() {
            player.set_pose_f64(
                self.actor.feet.to_array(),
                self.actor.look[0],
                self.actor.look[1],
                self.actor.ground,
            );
            player.allow_flying = matches!(self.actor.mode, 1 | 3);
            player.flying = self.flying;
            match event {
                Action::RequestStats => {
                    let mut packet = Writer::new();
                    packet.varint(saved.profile.statistics.len() as i32);
                    for (key, value) in &saved.profile.statistics {
                        packet.string(key).varint(*value);
                    }
                    self.send(0x37, packet);
                }
                Action::OpenInventory => saved.record_stat("achievement.openInventory"),
                Action::Command(input) => {
                    self.command(
                        &input,
                        false,
                        world,
                        sim,
                        &mut player,
                        resources,
                        options,
                        allow_commands,
                        out,
                        &mut saved.profile,
                    );
                    dirty = true;
                }
                Action::Complete(input) => {
                    self.command(
                        &input,
                        true,
                        world,
                        sim,
                        &mut player,
                        resources,
                        options,
                        allow_commands,
                        out,
                        &mut saved.profile,
                    );
                }
                Action::Flight(requested) => {
                    self.flying = self.actor.mode == 3 || self.actor.mode == 1 && requested
                }
                Action::CreativeSlot(index, item) => {
                    if self.actor.mode != 1 {
                        continue;
                    }
                    if item.as_ref().is_some_and(|s| {
                        s.damage < 0
                            || !(1..=64).contains(&s.count)
                            || crate::vanilla::item_entity::facts(s.id as u16).is_none()
                    }) {
                        continue;
                    }
                    let Ok(stack) = item.as_ref().map(crate::remote::from_slot).transpose() else {
                        continue;
                    };
                    if let Some(slot) = usize::try_from(index)
                        .ok()
                        .and_then(crate::remote::inventory_index)
                    {
                        sim.inventory.slots[slot] = stack;
                        dirty = true;
                    } else if (1..=4).contains(&index) {
                        sim.inventory.crafting[index as usize - 1] = stack;
                        dirty = true;
                    } else if index < 0 && self.drop_delay < 200 {
                        if let Some(stack) = stack {
                            self.drop_delay += 20;
                            self.throw_stack(sim, stack, false, true);
                        }
                    }
                }
                Action::Move(point, look, ground) => {
                    // NetHandlerPlayServer.processPlayer calls onUpdateEntity before
                    // applying this C03 packet's new coordinates to EntityPlayerMP.
                    dirty |= self.collect_at_previous_feet(sim, saved, pickups, host, out);
                    if let Some(point) = point {
                        let delta = point - self.actor.feet;
                        let dist = delta.length();
                        if dist > allowance || !world.contains(point.as_vec3()) {
                            self.warp();
                            continue;
                        }
                        let moved = movement::collision::move_body(
                            world,
                            Aabb::player(self.actor.feet),
                            delta,
                            self.actor.ground,
                            false,
                        );
                        if self.actor.mode != 3 && (moved.delta - delta).length_squared() > 0.0625 {
                            self.warp();
                            continue;
                        }
                        allowance -= dist;
                        self.actor.feet = point;
                    }
                    if let Some(angles) = look {
                        self.actor.look = angles;
                    }
                    self.actor.ground = ground;
                }
                Action::Sneak(value) => {
                    self.sneaking = value;
                    self.actor.flags = (self.actor.flags & !2) | (u8::from(value) << 1);
                }
                Action::Dig(kind, p) => {
                    if self.actor.mode == 3 {
                        self.correction(world, p);
                        continue;
                    }
                    if kind == 1 {
                        self.mine = None;
                        continue;
                    }
                    if matches!(kind, 3 | 4) {
                        if let Some(item) =
                            sim.inventory.take_selected(if kind == 3 { 64 } else { 1 })
                        {
                            if self.throw_stack(sim, item, true, false) {
                                let stat = saved
                                    .profile
                                    .statistics
                                    .entry("stat.drop".into())
                                    .or_default();
                                *stat = stat.wrapping_add(1);
                            }
                            dirty = true;
                        }
                        continue;
                    }
                    if !self.reach(world, sim, p, false) {
                        self.correction(world, p);
                        self.mine = None;
                        continue;
                    }
                    let state = world.get(p);
                    let held = sim.inventory.held();
                    if !crate::vanilla::click_border::can_edit(
                        self.actor.mode,
                        state >> 4,
                        held.as_ref(),
                    ) || self.actor.mode == 1
                        && held
                            .as_ref()
                            .is_some_and(|s| matches!(s.id, 267 | 268 | 272 | 276 | 283))
                    {
                        self.correction(world, p);
                        self.mine = None;
                        continue;
                    }
                    let inc = if self.actor.mode == 1 {
                        1.
                    } else {
                        self.relative(world, &sim.inventory, state)
                    };
                    if state == 0 || matches!(state >> 4, 8..=11) || inc <= 0. {
                        self.correction(world, p);
                        self.mine = None;
                        continue;
                    }
                    if kind == 0 {
                        self.mine = Some((p, state, sim.total_ticks, false));
                    }
                    if let Some((target, original, start, pending)) = self.mine {
                        if target == p && original == state {
                            let progress = (sim.total_ticks.saturating_sub(start) + 1) as f32 * inc;
                            if (kind == 0 && inc >= 1.) || (kind == 2 && progress >= 0.7) {
                                sim.destroy_picked_block(world, p, settings, out);
                                self.mine = None;
                                dirty = true;
                            } else if kind == 2 && !pending {
                                self.mine = Some((p, state, start, true));
                                self.correction(world, p);
                            }
                        } else {
                            self.correction(world, p);
                        }
                    } else {
                        self.correction(world, p);
                    }
                }
                Action::Place(p, face, offset) => {
                    if face >= 6 && face != 255 {
                        continue;
                    }
                    if face < 6 {
                        let hit = Hit {
                            kind: Kind::Block,
                            position: p,
                            face,
                            point: p.as_dvec3() + DVec3::from_array(offset.map(|n| f64::from(n) / 16.)),
                        };
                        let adj = hit.adjacent();
                        if self.reach(world, sim, p, true)
                            && crate::vanilla::click_border::can_place_on(
                                self.actor.mode,
                                world.get(p) >> 4,
                                sim.inventory.held().as_ref(),
                            )
                            && world.get(p) != 0
                            && (0..256).contains(&adj.y)
                            && sim
                                .world_border
                                .contains(adj, crate::vanilla::click_border::now())
                        {
                            if (!player.sneaking() || sim.inventory.held().is_none()) && sim.activate_circuit(world,p,out) {
                                dirty = true;
                            } else if let Some(item) = sim.inventory.held() {
                                if !host.precise_bounds().intersects(Aabb {
                                    min: adj.as_dvec3(),
                                    max: adj.as_dvec3() + DVec3::ONE,
                                }) {
                                    let before = out.changed.len();
                                    if sim.place_held_block(world, &player, &item, hit, out) {
                                        if let Some(&first) = out.changed.get(before) {
                                            if crate::vanilla::rail_events::is_rail(world.get(first)) {
                                                crate::gameplay::block_support::on_rail_added(
                                                    sim, world, first, out,
                                                );
                                            }
                                        }
                                    }
                                    dirty = true;
                                }
                            }
                        }
                        self.correction(world, p);
                        self.correction(world, adj);
                    } else if self.actor.mode != 3 {
                        if let Some(item) = sim.inventory.held() {
                            if item.id == 111 {
                                let origin = player.precise_eye(1.0);
                                let look = player.direction().as_dvec3();
                                if let Some(hit) = crate::vanilla::ray_trace::trace(
                                    world,
                                    origin,
                                    origin + look * 5.0,
                                    crate::vanilla::ray_trace::Options::item(true),
                                ) {
                                    let water_pos = hit.position;
                                    let above = water_pos + IVec3::Y;
                                    let water_state = world.get(water_pos);
                                    if hit.kind == Kind::Block
                                        && self.reach(world, sim, water_pos, true)
                                        && crate::vanilla::click_border::can_place_on(
                                            self.actor.mode,
                                            water_state >> 4,
                                            Some(&item),
                                        )
                                        && matches!(water_state >> 4, 8 | 9)
                                        && (water_state & 15) == 0
                                        && world.get(above) == 0
                                        && (0..256).contains(&above.y)
                                        && sim
                                            .world_border
                                            .contains(above, crate::vanilla::click_border::now())
                                        && !host.precise_bounds().intersects(Aabb {
                                            min: above.as_dvec3(),
                                            max: above.as_dvec3() + DVec3::ONE,
                                        })
                                    {
                                        if world.set(above, 111 << 4) {
                                            out.changed.push(above);
                                        }
                                        if self.actor.mode != 1 {
                                            sim.inventory.take_selected(1);
                                        }
                                        self.correction(world, above);
                                        dirty = true;
                                    }
                                }
                            }
                            if !dirty {
                                self.revision = self.revision.wrapping_add(1);
                            }
                        }
                    }
                }
                Action::Select(i) => {
                    sim.inventory.selected = i;
                    dirty = true;
                }
                Action::Click(w, n, b, t, m, s) => {
                    if self.actor.mode != 3 {
                        self.click(sim, w, n, b, t, m, s);
                    } else {
                        self.revision += 1;
                    }
                    dirty = true;
                }
                Action::Close => {
                    self.close_inventory(sim);
                    dirty = true;
                }
                Action::Confirm(0, tx) if self.pending == Some(tx) => self.pending = None,
                Action::Confirm(..) => (),
                Action::Swing => (),
            }
        }
        if let Some((p, state, start, true)) = self.mine {
            if !self.reach(world, sim, p, false) || world.get(p) != state {
                self.mine = None;
            } else if (sim.total_ticks.saturating_sub(start) + 1) as f32
                * self.relative(world, &sim.inventory, state)
                >= 1.
            {
                sim.destroy_picked_block(world, p, settings, out);
                self.mine = None;
                dirty = true;
            }
        }
        if dirty {
            self.refresh(&sim.inventory);
        }
        saved.position = Some(self.actor.feet.to_array());
        saved.look = self.actor.look;
        saved.sneaking = self.sneaking;
        saved.random = Some(replace(&mut sim.player_random_state, host_rand));
        sim.mode = host_mode;
        sim.commands.player_mode = host_game_type;
        sim.place_ticks = host_delay;
        swap(&mut sim.exhaustion, &mut saved.exhaustion);
        swap(&mut sim.inventory, &mut saved.inventory);
    }
}
