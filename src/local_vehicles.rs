//! Authoritative 1.8.9 singleplayer Minecarts (328/342/343/407/408), Boats (333),
//! Detector Rail (28) power & comparator state, ridden Pig direct travel, and vehicle rendering.
use super::{
    ai, block_support, drops_local::tile_drops_enabled, hoppers::insert_into_slots, species_ai,
    JavaRandom, LocalTarget, MobKind, Mode, Outcome, Particle, Player, Settings, Simulation, World,
};
use crate::{
    container::{self, Container, Kind, Special},
    inventory::{stack_limit, ClickAction, SlotRef, Stack},
    models::{
        self,
        vehicle::{self as vehicle_model, Actor as RenderActor, Damage as RenderDamage},
        Dynamic, ModelContext,
    },
    vanilla::{
        drops::DropNbt,
        entity_pick, item_nbt, material,
        mineshaft::ChestMinecart,
        movement::{self, collision, Aabb, Travel},
        notes::SIDES,
        rail_events, ray_trace,
        runtime::stack_from_loot,
    },
};
use glam::{DVec3, IVec3, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use winit::keyboard::KeyCode;

const RAIL_MATRIX: [[[i32; 3]; 2]; 10] = [
    [[0, 0, -1], [0, 0, 1]],
    [[-1, 0, 0], [1, 0, 0]],
    [[-1, -1, 0], [1, 0, 0]],
    [[-1, 0, 0], [1, -1, 0]],
    [[0, 0, -1], [0, -1, 1]],
    [[0, -1, -1], [0, 0, 1]],
    [[0, 0, 1], [1, 0, 0]],
    [[0, 0, 1], [-1, 0, 0]],
    [[0, 0, -1], [-1, 0, 0]],
    [[0, 0, -1], [1, 0, 0]],
];

fn default_true() -> bool {
    true
}
fn default_neg_one() -> i32 {
    -1
}
fn default_boat_speed() -> f64 {
    0.07
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum MinecartKind {
    Rideable,
    Chest,
    Furnace,
    Tnt,
    Hopper,
}

impl MinecartKind {
    pub fn from_item(id: u16) -> Option<Self> {
        match id {
            328 => Some(Self::Rideable),
            342 => Some(Self::Chest),
            343 => Some(Self::Furnace),
            407 => Some(Self::Tnt),
            408 => Some(Self::Hopper),
            _ => None,
        }
    }
    pub fn item_id(self) -> u16 {
        match self {
            Self::Rideable => 328,
            Self::Chest => 342,
            Self::Furnace => 343,
            Self::Tnt => 407,
            Self::Hopper => 408,
        }
    }
    pub fn variant_id(self) -> i32 {
        match self {
            Self::Rideable => 0,
            Self::Chest => 1,
            Self::Furnace => 2,
            Self::Tnt => 3,
            Self::Hopper => 5,
        }
    }
    pub fn slot_count(self) -> usize {
        match self {
            Self::Chest => 27,
            Self::Hopper => 5,
            _ => 0,
        }
    }
    pub fn display(self, fuel: i32) -> Option<(u16, i32)> {
        match self {
            Self::Rideable => None,
            Self::Chest => Some((54 << 4 | 2, 8)),
            Self::Furnace => Some(((if fuel > 0 { 62 } else { 61 }) << 4 | 2, 6)),
            Self::Tnt => Some((46 << 4, 6)),
            Self::Hopper => Some((154 << 4, 1)),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocalMinecart {
    pub id: u64,
    pub kind: MinecartKind,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub yaw: f32,
    pub prev_yaw: f32,
    pub pitch: f32,
    pub prev_pitch: f32,
    pub on_ground: bool,
    pub in_reverse: bool,
    pub hit_ticks: i32,
    pub hit_dir: i32,
    pub damage: f32,
    #[serde(default)]
    pub slots: Vec<Option<Stack>>,
    #[serde(default)]
    pub transfer_cooldown: i32,
    #[serde(default = "default_true")]
    pub hopper_enabled: bool,
    #[serde(default)]
    pub fuel: i32,
    #[serde(default)]
    pub push_x: f64,
    #[serde(default)]
    pub push_z: f64,
    #[serde(default = "default_neg_one")]
    pub fuse: i32,
    #[serde(default)]
    pub fall_distance: f32,
    #[serde(default)]
    pub custom_name: Option<String>,
}

impl LocalMinecart {
    pub fn new(id: u64, kind: MinecartKind, position: DVec3) -> Self {
        Self {
            id,
            kind,
            position,
            previous: position,
            velocity: DVec3::ZERO,
            yaw: 0.0,
            prev_yaw: 0.0,
            pitch: 0.0,
            prev_pitch: 0.0,
            on_ground: false,
            in_reverse: false,
            hit_ticks: 0,
            hit_dir: 1,
            damage: 0.0,
            slots: vec![None; kind.slot_count()],
            transfer_cooldown: -1,
            hopper_enabled: true,
            fuel: 0,
            push_x: 0.0,
            push_z: 0.0,
            fuse: -1,
            fall_distance: 0.0,
            custom_name: None,
        }
    }
    pub fn bounds(&self) -> Aabb {
        entity_pick::body(self.position, 0.98, 0.7)
    }
    fn max_speed(&self) -> f64 {
        if self.kind == MinecartKind::Furnace {
            0.2
        } else {
            0.4
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LocalBoat {
    pub id: u64,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub yaw: f32,
    pub prev_yaw: f32,
    pub pitch: f32,
    pub prev_pitch: f32,
    #[serde(default = "default_boat_speed")]
    pub speed_multiplier: f64,
    pub grounded: bool,
    pub hit_ticks: i32,
    pub hit_dir: i32,
    pub damage: f32,
}

impl LocalBoat {
    pub fn new(id: u64, position: DVec3, yaw: f32) -> Self {
        Self {
            id,
            position,
            previous: position,
            velocity: DVec3::ZERO,
            yaw,
            prev_yaw: yaw,
            pitch: 0.0,
            prev_pitch: 0.0,
            speed_multiplier: 0.07,
            grounded: false,
            hit_ticks: 0,
            hit_dir: 1,
            damage: 0.0,
        }
    }
    pub fn bounds(&self) -> Aabb {
        entity_pick::body(self.position, 1.5, 0.6)
    }
}

#[inline]
fn rail_shape_of(state: u16) -> usize {
    let raw = if state >> 4 == 66 {
        state & 15
    } else {
        state & 7
    };
    usize::from(raw.min(9))
}

fn snap_to_rail(world: &World, pos: DVec3) -> Option<DVec3> {
    let mut rpos = pos.floor().as_ivec3();
    if rail_events::is_rail(world.get(rpos - IVec3::Y)) {
        rpos.y -= 1;
    }
    let state = world.get(rpos);
    if !rail_events::is_rail(state) {
        return None;
    }
    let ends = RAIL_MATRIX[rail_shape_of(state)];
    let x0 = f64::from(rpos.x) + 0.5 + f64::from(ends[0][0]) * 0.5;
    let y0 = f64::from(rpos.y) + 0.0625 + f64::from(ends[0][1]) * 0.5;
    let z0 = f64::from(rpos.z) + 0.5 + f64::from(ends[0][2]) * 0.5;
    let x1 = f64::from(rpos.x) + 0.5 + f64::from(ends[1][0]) * 0.5;
    let y1 = f64::from(rpos.y) + 0.0625 + f64::from(ends[1][1]) * 0.5;
    let z1 = f64::from(rpos.z) + 0.5 + f64::from(ends[1][2]) * 0.5;
    let dx = x1 - x0;
    let dy = (y1 - y0) * 2.0;
    let dz = z1 - z0;
    let t = if dx == 0.0 {
        pos.z - f64::from(rpos.z)
    } else if dz == 0.0 {
        pos.x - f64::from(rpos.x)
    } else {
        ((pos.x - x0) * dx + (pos.z - z0) * dz) * 2.0
    };
    let x = x0 + dx * t;
    let mut y = y0 + dy * t;
    let z = z0 + dz * t;
    if dy < 0.0 {
        y += 1.0;
    } else if dy > 0.0 {
        y += 0.5;
    }
    Some(DVec3::new(x, y, z))
}

fn detector_box(pos: IVec3) -> Aabb {
    let min = pos.as_dvec3() + DVec3::new(f64::from(0.2f32), 0.0, f64::from(0.2f32));
    let max = pos.as_dvec3() + DVec3::new(f64::from(0.8f32), f64::from(0.8f32), f64::from(0.8f32));
    Aabb { min, max }
}

fn wrap_degrees_f32(angle: f32) -> f32 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

fn wrap_degrees_f64(angle: f64) -> f64 {
    (angle + 180.0).rem_euclid(360.0) - 180.0
}

fn safe_dismount_feet(world: &World, anchor: DVec3) -> DVec3 {
    for y in [0.0, 1.0, -1.0] {
        for (x, z) in [
            (1.0, 0.0),
            (-1.0, 0.0),
            (0.0, 1.0),
            (0.0, -1.0),
            (1.0, 1.0),
            (-1.0, 1.0),
            (1.0, -1.0),
            (-1.0, -1.0),
        ] {
            let candidate = anchor + DVec3::new(x, y, z);
            let bounds = Aabb::player(candidate);
            if collision::colliders(world, bounds).is_empty()
                && !collision::colliders(world, bounds.offset(-DVec3::Y * 0.1)).is_empty()
            {
                return candidate;
            }
        }
    }
    for y in [1.0, 0.5, 1.6, 0.0] {
        let candidate = anchor + DVec3::Y * y;
        if collision::colliders(world, Aabb::player(candidate)).is_empty() {
            return candidate;
        }
    }
    anchor + DVec3::Y
}

fn stack_custom_name(stack: &Stack) -> Option<String> {
    let map = stack.nbt.as_deref().and_then(item_nbt::compound)?;
    let display = map.get("display").and_then(item_nbt::compound)?;
    match display.get("Name") {
        Some(DropNbt::String(name)) if !name.is_empty() => Some(name.clone()),
        _ => None,
    }
}

fn with_custom_name(stack: Stack, name: &str) -> Stack {
    stack.with_nbt(DropNbt::Compound(BTreeMap::from([(
        "display".into(),
        DropNbt::Compound(BTreeMap::from([(
            "Name".into(),
            DropNbt::String(name.to_owned()),
        )])),
    )])))
}

fn ray_hit_t(origin: DVec3, end: DVec3, seg_len: f64, bounds: Aabb) -> Option<f64> {
    if origin.cmpgt(bounds.min).all() && origin.cmplt(bounds.max).all() {
        return Some(0.0);
    }
    let raw = [
        bounds.min.x,
        bounds.min.y,
        bounds.min.z,
        bounds.max.x,
        bounds.max.y,
        bounds.max.z,
    ];
    let (_, point) = ray_trace::intercept(raw, origin, end)?;
    Some(origin.distance(point) / seg_len)
}

impl Simulation {
    fn entity_drops_enabled(&self) -> bool {
        self.world_commands
            .rules
            .get("doEntityDrops")
            .is_none_or(|v| !v.eq_ignore_ascii_case("false"))
    }

    fn alloc_vehicle_id(&mut self) -> u64 {
        self.next_id = self.next_id.wrapping_add(1).max(1);
        while self.minecarts.iter().any(|c| c.id == self.next_id)
            || self.boats.iter().any(|b| b.id == self.next_id)
            || self.mobs.iter().any(|m| m.id == self.next_id)
        {
            self.next_id = self.next_id.wrapping_add(1).max(1);
        }
        self.next_id
    }

    pub(crate) fn is_riding_local(&self) -> bool {
        self.ridden_horse.is_some()
            || self.ridden_pig.is_some()
            || self.ridden_minecart.is_some()
            || self.ridden_boat.is_some()
    }

    pub(crate) fn clear_local_mounts(&mut self, player: &mut Player) {
        self.ridden_horse = None;
        self.ridden_pig = None;
        self.ridden_minecart = None;
        self.ridden_boat = None;
        self.horse_rider = Default::default();
        self.pig_boost_ticks = 0;
        self.pig_boost_total = 0;
        self.pig_speed = 0.0;
        player.riding = false;
        player.third_person.riding = false;
        player.third_person.mount_yaw = None;
    }

    pub(crate) fn seat_vehicle_rider(&self, player: &mut Player) -> bool {
        if let Some(id) = self.ridden_minecart {
            if let Some(cart) = self.minecarts.iter().find(|c| c.id == id) {
                let seat = cart.position - DVec3::Y * 0.35;
                player.set_position_f64(seat.to_array());
                player.set_velocity_f64([0.0; 3]);
                player.jumped = false;
                player.riding = true;
                player.third_person.riding = true;
                player.third_person.mount_yaw = None;
                return true;
            }
        }
        if let Some(id) = self.ridden_boat {
            if let Some(boat) = self.boats.iter().find(|b| b.id == id) {
                let angle = f64::from(boat.yaw) * std::f64::consts::PI / 180.0;
                let seat = boat.position + DVec3::new(angle.cos() * 0.4, -0.65, angle.sin() * 0.4);
                player.set_position_f64(seat.to_array());
                player.set_velocity_f64([0.0; 3]);
                player.jumped = false;
                player.riding = true;
                player.third_person.riding = true;
                player.third_person.mount_yaw = None;
                return true;
            }
        }
        false
    }

    pub(crate) fn adopt_mineshaft_cart(&mut self, cart: ChestMinecart) {
        let pos = DVec3::from_array(cart.position);
        if self
            .minecarts
            .iter()
            .any(|c| c.kind == MinecartKind::Chest && c.position.distance_squared(pos) < 0.01)
        {
            return;
        }
        let id = self.alloc_vehicle_id();
        let mut entity = LocalMinecart::new(id, MinecartKind::Chest, pos);
        for (i, loot) in cart.slots.iter().enumerate().take(27) {
            entity.slots[i] = loot.as_ref().map(stack_from_loot);
        }
        self.minecarts.push(entity);
    }

    pub(crate) fn try_place_minecart_on_rail(
        &mut self,
        world: &mut World,
        _player: &Player,
        hit_pos: IVec3,
        item_id: u16,
        out: &mut Outcome,
    ) -> bool {
        let Some(kind) = MinecartKind::from_item(item_id) else {
            return false;
        };
        let state = world.get(hit_pos);
        if !rail_events::is_rail(state) {
            return false;
        }
        let shape = rail_shape_of(state);
        let lift = if (2..=5).contains(&shape) { 0.5 } else { 0.0 };
        let pos = DVec3::new(
            f64::from(hit_pos.x) + 0.5,
            f64::from(hit_pos.y) + 0.0625 + lift,
            f64::from(hit_pos.z) + 0.5,
        );
        let id = self.alloc_vehicle_id();
        let mut cart = LocalMinecart::new(id, kind, pos);
        if let Some(held) = self.inventory.held() {
            if let Some(name) = stack_custom_name(&held) {
                cart.custom_name = Some(name);
            }
        }
        self.minecarts.push(cart);
        if self.mode != Mode::Creative {
            self.inventory.take_selected(1);
        }
        if state >> 4 == 28 {
            self.update_detector_rail(world, hit_pos, out);
        }
        true
    }

    pub(crate) fn try_place_boat(
        &mut self,
        world: &World,
        player: &Player,
        _out: &mut Outcome,
    ) -> bool {
        let eye = player.precise_eye(1.0);
        let look = player.direction().as_dvec3();
        let Some(hit) = ray_trace::trace(world, eye, eye + look * 5.0, ray_trace::Options::item(true))
        else {
            return false;
        };
        if hit.kind != ray_trace::Kind::Block {
            return false;
        }
        let sweep = player.precise_bounds().sweep(look * 5.0).expand(DVec3::ONE);
        let eye_inside = |box_aabb: Aabb, border: f32| {
            sweep.intersects(box_aabb) && {
                let b = box_aabb.expand(DVec3::splat(f64::from(border)));
                eye.cmpgt(b.min).all() && eye.cmplt(b.max).all()
            }
        };
        for mob in &self.mobs {
            if mob.health <= 0.0 {
                continue;
            }
            let scale = if self.is_baby(mob.id) { 0.5 } else { 1.0 };
            let (w, h) = mob.kind.adult_size(self.command_entities.mobs.get(&mob.id));
            if eye_inside(entity_pick::body(mob.position.as_dvec3(), w * scale, h * scale), 0.1) {
                return false;
            }
        }
        for cart in &self.minecarts {
            if eye_inside(cart.bounds(), 0.1) {
                return false;
            }
        }
        for boat in &self.boats {
            if eye_inside(boat.bounds(), 0.1) {
                return false;
            }
        }
        let mut p = hit.position;
        if world.get(p) >> 4 == 78 {
            p.y -= 1;
        }
        let spawn_pos = DVec3::new(
            f64::from(p.x as f32 + 0.5),
            f64::from(p.y as f32 + 1.0),
            f64::from(p.z as f32 + 0.5),
        );
        let yaw = (player.java_rotation()[0] - 90.0).rem_euclid(360.0);
        let boat = LocalBoat::new(0, spawn_pos, yaw);
        let check_box = boat.bounds().expand(DVec3::splat(-0.1));
        if !collision::colliders(world, check_box).is_empty() {
            return false;
        }
        let id = self.alloc_vehicle_id();
        self.boats.push(LocalBoat::new(id, spawn_pos, yaw));
        if self.mode != Mode::Creative {
            self.inventory.take_selected(1);
        }
        true
    }

    pub(crate) fn pick_local_vehicle(
        &self,
        origin: DVec3,
        end: DVec3,
        best_t: f64,
    ) -> Option<(f64, LocalTarget)> {
        let seg_len = (end - origin).length();
        if seg_len <= 1e-9 {
            return None;
        }
        let mut best = best_t;
        let mut chosen = None;
        for cart in &self.minecarts {
            if self.ridden_minecart == Some(cart.id) {
                continue;
            }
            if let Some(t) = ray_hit_t(origin, end, seg_len, cart.bounds()) {
                if t < best {
                    best = t;
                    chosen = Some((t, LocalTarget::Minecart(cart.id)));
                }
            }
        }
        for boat in &self.boats {
            if self.ridden_boat == Some(boat.id) {
                continue;
            }
            let expanded = boat.bounds().expand(DVec3::splat(f64::from(0.1f32)));
            if let Some(t) = ray_hit_t(origin, end, seg_len, expanded) {
                if t < best {
                    best = t;
                    chosen = Some((t, LocalTarget::Boat(boat.id)));
                }
            }
        }
        chosen
    }

    pub(crate) fn interact_minecart(
        &mut self,
        id: u64,
        _world: &mut World,
        player: &mut Player,
        _out: &mut Outcome,
    ) -> bool {
        let Some(idx) = self.minecarts.iter().position(|c| c.id == id) else {
            return false;
        };
        match self.minecarts[idx].kind {
            MinecartKind::Rideable => {
                if player.sneaking() {
                    return false;
                }
                if self.ridden_minecart == Some(id) {
                    return true;
                }
                self.clear_local_mounts(player);
                self.ridden_minecart = Some(id);
                self.seat_vehicle_rider(player);
                true
            }
            MinecartKind::Chest | MinecartKind::Hopper => {
                self.minecart_window = Some(id);
                self.show_minecart_window = true;
                true
            }
            MinecartKind::Furnace => {
                if self.inventory.held().is_some_and(|s| s.id == 263) {
                    if self.mode != Mode::Creative {
                        self.inventory.take_selected(1);
                    }
                    let [px, _, pz] = player.precise_position();
                    let cart = &mut self.minecarts[idx];
                    cart.fuel = (cart.fuel + 3600).min(32000);
                    cart.push_x = cart.position.x - px;
                    cart.push_z = cart.position.z - pz;
                }
                true
            }
            MinecartKind::Tnt => false,
        }
    }

    pub(crate) fn interact_boat(&mut self, id: u64, _world: &World, player: &mut Player) -> bool {
        if !self.boats.iter().any(|b| b.id == id) || player.sneaking() {
            return false;
        }
        if self.ridden_boat == Some(id) {
            return true;
        }
        self.clear_local_mounts(player);
        self.ridden_boat = Some(id);
        self.seat_vehicle_rider(player);
        true
    }

    pub(crate) fn attack_minecart(
        &mut self,
        id: u64,
        world: &mut World,
        player: &mut Player,
        damage: f32,
        out: &mut Outcome,
    ) {
        let Some(idx) = self.minecarts.iter().position(|c| c.id == id) else {
            return;
        };
        let creative = self.mode == Mode::Creative;
        let has_name = self.minecarts[idx].custom_name.is_some();
        {
            let cart = &mut self.minecarts[idx];
            cart.hit_dir = if cart.hit_dir == 0 { -1 } else { -cart.hit_dir };
            cart.hit_ticks = 10;
            cart.damage += damage.max(0.5) * 10.0;
        }
        if creative || self.minecarts[idx].damage > 40.0 {
            let cart = self.minecarts.remove(idx);
            if self.ridden_minecart == Some(id) {
                self.dismount_from_vehicle(world, player, cart.position);
            }
            if self.minecart_window == Some(id) {
                self.minecart_window = None;
                self.show_minecart_window = false;
            }
            let horiz_sq = cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z;
            if (!creative || has_name) && self.entity_drops_enabled() {
                let mut base = Stack::new(328, 1);
                if let Some(name) = &cart.custom_name {
                    base = with_custom_name(base, name);
                }
                self.spawn_item_entity(base, cart.position, DVec3::ZERO, 10);
                match cart.kind {
                    MinecartKind::Rideable => {}
                    MinecartKind::Chest => {
                        self.spawn_item_entity(Stack::new(54, 1), cart.position, DVec3::ZERO, 10);
                    }
                    MinecartKind::Furnace => {
                        self.spawn_item_entity(Stack::new(61, 1), cart.position, DVec3::ZERO, 10);
                    }
                    MinecartKind::Tnt => {
                        if horiz_sq < 0.01 && cart.fuse < 0 {
                            self.spawn_item_entity(
                                Stack::new(46, 1),
                                cart.position,
                                DVec3::ZERO,
                                10,
                            );
                        }
                    }
                    MinecartKind::Hopper => {
                        self.spawn_item_entity(Stack::new(154, 1), cart.position, DVec3::ZERO, 10);
                    }
                }
                for stack in cart.slots.into_iter().flatten() {
                    self.spawn_item_entity(stack, cart.position, DVec3::ZERO, 10);
                }
            }
            if cart.kind == MinecartKind::Tnt && (horiz_sq >= 0.01 || cart.fuse >= 0) && !creative {
                self.explode_tnt_cart(world, player, cart.position, horiz_sq, out);
            }
            self.refresh_detector_rails_near(world, cart.position, out);
        }
    }

    pub(crate) fn attack_boat(
        &mut self,
        id: u64,
        player: &mut Player,
        damage: f32,
        _out: &mut Outcome,
    ) {
        let Some(idx) = self.boats.iter().position(|b| b.id == id) else {
            return;
        };
        let creative = self.mode == Mode::Creative;
        {
            let boat = &mut self.boats[idx];
            boat.hit_dir = if boat.hit_dir == 0 { -1 } else { -boat.hit_dir };
            boat.hit_ticks = 10;
            boat.damage += damage.max(0.5) * 10.0;
        }
        if creative || self.boats[idx].damage > 40.0 {
            let boat = self.boats.remove(idx);
            if self.ridden_boat == Some(id) {
                self.ridden_boat = None;
                player.riding = false;
                player.third_person.riding = false;
                player.third_person.mount_yaw = None;
                player.set_position_f64((boat.position + DVec3::Y * 0.6).to_array());
                player.set_velocity_f64([0.0; 3]);
            }
            if !creative && self.entity_drops_enabled() {
                self.spawn_item_entity(Stack::new(333, 1), boat.position, DVec3::ZERO, 10);
            }
        }
    }

    pub(crate) fn middle_pick_minecart(
        &self,
        id: u64,
    ) -> Option<crate::vanilla::middle_pick::Item> {
        let cart = self.minecarts.iter().find(|c| c.id == id)?;
        Some(crate::vanilla::middle_pick::Item {
            id: cart.kind.item_id(),
            damage: 0,
            match_damage: false,
        })
    }

    pub(crate) fn middle_pick_boat(&self, id: u64) -> Option<crate::vanilla::middle_pick::Item> {
        self.boats
            .iter()
            .any(|b| b.id == id)
            .then_some(crate::vanilla::middle_pick::Item {
                id: 333,
                damage: 0,
                match_damage: false,
            })
    }

    pub(crate) fn minecart_window_view(&self, player: &Player) -> Option<Container> {
        let id = self.minecart_window?;
        let cart = self.minecarts.iter().find(|c| c.id == id)?;
        let p = DVec3::from_array(player.precise_position());
        if p.distance_squared(cart.position) > 64.0 {
            return None;
        }
        let mut c = match cart.kind {
            MinecartKind::Chest => Container::new(Kind::Chest),
            MinecartKind::Hopper => {
                let mut h = Container::new(Kind::Chest);
                h.special = Special::Hopper;
                h
            }
            _ => return None,
        };
        c.slots = cart.slots.clone();
        c.slots.resize(cart.kind.slot_count(), None);
        Some(c)
    }

    pub(crate) fn minecart_window_click(
        &mut self,
        slot: SlotRef,
        action: ClickAction,
    ) -> Vec<Stack> {
        let Some(id) = self.minecart_window else {
            return Vec::new();
        };
        let Some(idx) = self.minecarts.iter().position(|c| c.id == id) else {
            self.minecart_window = None;
            return Vec::new();
        };
        let kind = self.minecarts[idx].kind;
        let mut c = match kind {
            MinecartKind::Chest => Container::new(Kind::Chest),
            MinecartKind::Hopper => {
                let mut h = Container::new(Kind::Chest);
                h.special = Special::Hopper;
                h
            }
            _ => return Vec::new(),
        };
        c.slots = self.minecarts[idx].slots.clone();
        c.slots.resize(kind.slot_count(), None);
        let dropped = container::window_click(
            &mut self.inventory,
            Some(&mut c),
            slot,
            action,
            self.mode == Mode::Creative,
            2,
        );
        self.minecarts[idx].slots = c.slots;
        dropped
    }

    pub(crate) fn update_detector_rail(&mut self, world: &mut World, pos: IVec3, out: &mut Outcome) {
        let state = world.get(pos);
        if state >> 4 != 28 {
            return;
        }
        let was_powered = state & 8 != 0;
        let box_area = detector_box(pos);
        let has_cart = self.minecarts.iter().any(|c| c.bounds().intersects(box_area));
        let next = if has_cart { state | 8 } else { state & !8 };
        if next != state && world.set(pos, next) {
            out.changed.push(pos);
            self.block_ticks.notify(world, pos);
            self.block_ticks.notify(world, pos - IVec3::Y);
            block_support::check_neighbors_support(self, world, pos, out);
            block_support::check_neighbors_support(self, world, pos - IVec3::Y, out);
            self.pulse_detector_redstone(world, pos, out);
        } else if has_cart && was_powered {
            // Re-evaluate comparator overrides even when bit 8 stays set (e.g. chest cart slot change).
            self.pulse_detector_redstone(world, pos, out);
        }
        if has_cart {
            self.block_ticks.schedule_block_update(pos, 28, 20, 0);
        }
    }

    fn pulse_detector_redstone(&mut self, world: &mut World, pos: IVec3, out: &mut Outcome) {
        let saved_moving = std::mem::take(&mut self.redstone.moving);
        let mut dummy = Player::new(Vec3::new(0.0, -1000.0, 0.0));
        dummy.sleeping = true;
        let mut seeds = vec![pos, pos - IVec3::Y];
        for side in SIDES {
            seeds.push(pos + side);
            seeds.push(pos - IVec3::Y + side);
        }
        self.update_circuits(world, &mut dummy, seeds, out);
        let mut newly_moving = std::mem::take(&mut self.redstone.moving);
        self.redstone.moving = saved_moving;
        self.redstone.moving.append(&mut newly_moving);
    }

    fn refresh_detector_rails_near(&mut self, world: &mut World, at: DVec3, out: &mut Outcome) {
        let base = at.floor().as_ivec3();
        for dy in -1..=1 {
            for dx in -1..=1 {
                for dz in -1..=1 {
                    let p = base + IVec3::new(dx, dy, dz);
                    if world.get(p) >> 4 == 28 {
                        self.update_detector_rail(world, p, out);
                    }
                }
            }
        }
    }

    pub(crate) fn detector_rail_comparator_signal(&self, pos: IVec3, state: u16) -> u8 {
        if state & 8 == 0 {
            return 0;
        }
        let area = detector_box(pos);
        let Some(cart) = self.minecarts.iter().find(|c| {
            matches!(c.kind, MinecartKind::Chest | MinecartKind::Hopper)
                && c.bounds().intersects(area)
        }) else {
            return 0;
        };
        if cart.slots.is_empty() {
            return 0;
        }
        let mut count_slots = 0usize;
        let mut fill = 0.0f32;
        for slot in &cart.slots {
            if let Some(stack) = slot {
                let max = f32::from(stack_limit(stack.id).max(1));
                fill += f32::from(stack.count) / max;
                count_slots += 1;
            }
        }
        if count_slots == 0 {
            return 0;
        }
        fill /= cart.slots.len() as f32;
        ((fill * 14.0).floor() as u8 + 1).min(15)
    }

    pub(crate) fn vehicle_solid_boxes(&self, exclude_ridden: bool) -> Vec<Aabb> {
        let mut out = Vec::with_capacity(
            self.boats.len() + if exclude_ridden { 0 } else { self.minecarts.len() },
        );
        for boat in &self.boats {
            if exclude_ridden && self.ridden_boat == Some(boat.id) {
                continue;
            }
            out.push(boat.bounds());
        }
        if !exclude_ridden {
            for cart in &self.minecarts {
                out.push(cart.bounds());
            }
        }
        out
    }

    pub(crate) fn vehicle_placement_boxes(&self) -> Vec<Aabb> {
        self.vehicle_solid_boxes(false)
    }

    fn dismount_from_vehicle(&mut self, world: &World, player: &mut Player, anchor: DVec3) {
        let feet = safe_dismount_feet(world, anchor);
        self.ridden_minecart = None;
        self.ridden_boat = None;
        player.riding = false;
        player.third_person.riding = false;
        player.third_person.mount_yaw = None;
        player.set_position_f64(feet.to_array());
        player.set_velocity_f64([0.0; 3]);
    }

    fn explode_tnt_cart(
        &mut self,
        world: &mut World,
        player: &mut Player,
        center: DVec3,
        horiz_speed_sq: f64,
        out: &mut Outcome,
    ) {
        let speed = horiz_speed_sq.sqrt().min(5.0);
        let mut rng = JavaRandom::from_state(self.xp_math_random);
        let power = (4.0 + rng.next_double() * 1.5 * speed) as f32;
        self.xp_math_random = rng.state();
        self.explode_excluding(
            world,
            player,
            &Settings::default(),
            center,
            power,
            true,
            false,
            None,
            out,
        );
    }

    pub(crate) fn tick_local_vehicles(
        &mut self,
        world: &mut World,
        player: &mut Player,
        out: &mut Outcome,
    ) {
        if let Some(id) = self.ridden_minecart {
            let dismount = player.sneaking() || player.is_spectator() || self.health <= 0.0;
            if let Some(pos) = self.minecarts.iter().find(|c| c.id == id).map(|c| c.position) {
                if dismount {
                    self.dismount_from_vehicle(world, player, pos);
                }
            } else {
                self.ridden_minecart = None;
                player.riding = false;
                player.third_person.riding = false;
                player.third_person.mount_yaw = None;
            }
        }
        if let Some(id) = self.ridden_boat {
            let dismount = player.sneaking() || player.is_spectator() || self.health <= 0.0;
            if let Some(pos) = self.boats.iter().find(|b| b.id == id).map(|b| b.position) {
                if dismount {
                    self.dismount_from_vehicle(world, player, pos);
                }
            } else {
                self.ridden_boat = None;
                player.riding = false;
                player.third_person.riding = false;
                player.third_person.mount_yaw = None;
            }
        }

        self.tick_minecarts(world, player, out);
        self.tick_boats(world, player, out);

        if self.ridden_minecart.is_some() || self.ridden_boat.is_some() {
            self.seat_vehicle_rider(player);
        }
    }

    fn tick_minecarts(&mut self, world: &mut World, player: &mut Player, out: &mut Outcome) {
        let rider_forward = f32::from(player.keys.contains(&KeyCode::KeyW))
            - f32::from(player.keys.contains(&KeyCode::KeyS));
        let rider_strafe = f32::from(player.keys.contains(&KeyCode::KeyA))
            - f32::from(player.keys.contains(&KeyCode::KeyD));
        let rider_yaw = player.java_rotation()[0];

        let mut i = 0;
        while i < self.minecarts.len() {
            if !world.contains(self.minecarts[i].position.as_vec3()) {
                i += 1;
                continue;
            }
            let cart_id = self.minecarts[i].id;
            let ridden = self.ridden_minecart == Some(cart_id);
            let prev_cell = self.minecarts[i].position.floor().as_ivec3();
            {
                let cart = &mut self.minecarts[i];
                if cart.hit_ticks > 0 {
                    cart.hit_ticks -= 1;
                }
                if cart.damage > 0.0 {
                    cart.damage = (cart.damage - 1.0).max(0.0);
                }
                cart.previous = cart.position;
                cart.prev_yaw = cart.yaw;
                cart.prev_pitch = cart.pitch;
            }
            if self.minecarts[i].position.y < -64.0 {
                let removed = self.minecarts.remove(i);
                if ridden {
                    self.dismount_from_vehicle(world, player, removed.position);
                }
                continue;
            }

            self.minecarts[i].velocity.y -= f64::from(0.04f32);
            let mut rpos = self.minecarts[i].position.floor().as_ivec3();
            if rail_events::is_rail(world.get(rpos - IVec3::Y)) {
                rpos.y -= 1;
            }
            let rstate = world.get(rpos);
            let clipped_horiz;
            let mut landed_fall = 0.0f32;
            let pre_horiz_sq = {
                let v = self.minecarts[i].velocity;
                v.x * v.x + v.z * v.z
            };

            if rail_events::is_rail(rstate) {
                clipped_horiz = self.step_minecart_on_rail(
                    world,
                    i,
                    rpos,
                    rstate,
                    ridden,
                    rider_forward,
                    rider_strafe,
                    rider_yaw,
                    out,
                );
                if rstate >> 4 == 157 {
                    let powered = rstate & 8 != 0;
                    match self.minecarts[i].kind {
                        MinecartKind::Rideable if powered && ridden => {
                            let pos = self.minecarts[i].position;
                            self.dismount_from_vehicle(world, player, pos);
                        }
                        MinecartKind::Tnt if powered && self.minecarts[i].fuse < 0 => {
                            self.minecarts[i].fuse = 80;
                            out.world_sounds.push((
                                "game.tnt.primed",
                                self.minecarts[i].position,
                                1.0,
                                1.0,
                            ));
                        }
                        MinecartKind::Hopper => {
                            self.minecarts[i].hopper_enabled = !powered;
                        }
                        _ => {}
                    }
                }
            } else {
                let (clipped, landed) = self.step_derailed_minecart(world, i);
                clipped_horiz = clipped;
                landed_fall = landed;
            }

            // Yaw / reverse update matching EntityMinecart.onUpdate
            {
                let cart = &mut self.minecarts[i];
                cart.pitch = 0.0;
                let dx = cart.previous.x - cart.position.x;
                let dz = cart.previous.z - cart.position.z;
                if dx * dx + dz * dz > 0.001 {
                    cart.yaw = (ai::motion::atan2(dz, dx) * 180.0 / std::f64::consts::PI) as f32;
                    if cart.in_reverse {
                        cart.yaw += 180.0;
                    }
                }
                let turn = wrap_degrees_f32(cart.yaw - cart.prev_yaw);
                if !(-170.0..170.0).contains(&turn) {
                    cart.yaw += 180.0;
                    cart.in_reverse = !cart.in_reverse;
                }
            }

            // Block collisions -> Detector Rail (28) activation
            let bounds = self.minecarts[i].bounds();
            let lo = (bounds.min + DVec3::splat(0.001)).floor().as_ivec3();
            let hi = (bounds.max - DVec3::splat(0.001)).floor().as_ivec3();
            for x in lo.x..=hi.x {
                for y in (lo.y - 1)..=hi.y {
                    for z in lo.z..=hi.z {
                        let bp = IVec3::new(x, y, z);
                        let st = world.get(bp);
                        if st >> 4 == 28 && st & 8 == 0 {
                            self.update_detector_rail(world, bp, out);
                        }
                    }
                }
            }
            if world.get(prev_cell) >> 4 == 28 && prev_cell != self.minecarts[i].position.floor().as_ivec3() {
                self.update_detector_rail(world, prev_cell, out);
            }

            // TNT Minecart (407) fuse & impact detonation
            if self.minecarts[i].kind == MinecartKind::Tnt {
                let mut explode = false;
                if self.minecarts[i].fuse > 0 {
                    self.minecarts[i].fuse -= 1;
                    let mut rng = JavaRandom::from_state(self.particle_random[2]);
                    if self.particles.len() < 4096 {
                        if let Some(p) = Particle::network_sprite(
                            11,
                            self.minecarts[i].position + DVec3::Y * 0.5,
                            DVec3::ZERO,
                            &mut rng,
                        ) {
                            self.particles.push(p);
                        }
                    }
                    self.particle_random[2] = rng.state();
                    if self.minecarts[i].fuse == 0 {
                        explode = true;
                    }
                } else if self.minecarts[i].fuse == 0 {
                    explode = true;
                }
                if (clipped_horiz && pre_horiz_sq >= 0.01) || landed_fall >= 3.0 {
                    explode = true;
                }
                if explode {
                    let removed = self.minecarts.remove(i);
                    if self.ridden_minecart == Some(removed.id) {
                        self.dismount_from_vehicle(world, player, removed.position);
                    }
                    let speed_sq = if landed_fall >= 3.0 {
                        f64::from((landed_fall * 5.0).min(5.0)).powi(2)
                    } else {
                        pre_horiz_sq
                    };
                    self.explode_tnt_cart(world, player, removed.position, speed_sq, out);
                    self.refresh_detector_rails_near(world, removed.position, out);
                    continue;
                }
            }

            // Hopper Minecart (408) suction & container pull
            if self.minecarts[i].kind == MinecartKind::Hopper && self.minecarts[i].hopper_enabled {
                let cur_cell = self.minecarts[i].position.floor().as_ivec3();
                if cur_cell == prev_cell {
                    self.minecarts[i].transfer_cooldown -= 1;
                } else {
                    self.minecarts[i].transfer_cooldown = 0;
                }
                if self.minecarts[i].transfer_cooldown <= 0 {
                    self.minecarts[i].transfer_cooldown = 0;
                    if self.tick_hopper_minecart_pull(world, i, out) {
                        self.minecarts[i].transfer_cooldown = 4;
                    }
                }
            }

            i += 1;
        }

        // Cart-cart & player-cart soft collisions
        let len = self.minecarts.len();
        for a in 0..len {
            for b in (a + 1)..len {
                if self.minecarts[a]
                    .bounds()
                    .expand(DVec3::new(0.2, 0.0, 0.2))
                    .intersects(self.minecarts[b].bounds())
                {
                    self.collide_minecart_pair(a, b);
                }
            }
            if self.ridden_minecart != Some(self.minecarts[a].id) && !player.is_spectator() {
                let pbounds = player.precise_bounds();
                if self.minecarts[a]
                    .bounds()
                    .expand(DVec3::new(0.2, 0.0, 0.2))
                    .intersects(pbounds)
                {
                    let [px, _, pz] = player.precise_position();
                    let mut dx = px - self.minecarts[a].position.x;
                    let mut dz = pz - self.minecarts[a].position.z;
                    let dist_sq = dx * dx + dz * dz;
                    if dist_sq >= 1e-4 {
                        let dist = dist_sq.sqrt();
                        dx /= dist;
                        dz /= dist;
                        let scale = (1.0 / dist).min(1.0) * 0.1 * 0.5;
                        self.minecarts[a].velocity.x -= dx * scale;
                        self.minecarts[a].velocity.z -= dz * scale;
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn step_minecart_on_rail(
        &mut self,
        world: &World,
        idx: usize,
        rpos: IVec3,
        rstate: u16,
        ridden: bool,
        rider_forward: f32,
        rider_strafe: f32,
        rider_yaw: f32,
        _out: &mut Outcome,
    ) -> bool {
        let old_snap = snap_to_rail(world, self.minecarts[idx].position);
        let shape = rail_shape_of(rstate);
        let powered_rail = rstate >> 4 == 27;
        let mut boost = powered_rail && (rstate & 8 != 0);
        let mut brake = powered_rail && !boost;

        {
            let cart = &mut self.minecarts[idx];
            cart.fall_distance = 0.0;
            cart.on_ground = false;
            cart.position.y = f64::from(rpos.y);
            match shape {
                2 => {
                    cart.velocity.x -= 0.0078125;
                    cart.position.y += 1.0;
                }
                3 => {
                    cart.velocity.x += 0.0078125;
                    cart.position.y += 1.0;
                }
                4 => {
                    cart.velocity.z += 0.0078125;
                    cart.position.y += 1.0;
                }
                5 => {
                    cart.velocity.z -= 0.0078125;
                    cart.position.y += 1.0;
                }
                _ => {}
            }

            let ends = RAIL_MATRIX[shape];
            let mut rx = f64::from(ends[1][0] - ends[0][0]);
            let mut rz = f64::from(ends[1][2] - ends[0][2]);
            let rlen = (rx * rx + rz * rz).sqrt();
            if cart.velocity.x * rx + cart.velocity.z * rz < 0.0 {
                rx = -rx;
                rz = -rz;
            }
            let speed = (cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z)
                .sqrt()
                .min(2.0);
            cart.velocity.x = speed * rx / rlen;
            cart.velocity.z = speed * rz / rlen;

            if ridden && (rider_forward != 0.0 || rider_strafe != 0.0) {
                let yaw_rad = f64::from(rider_yaw).to_radians();
                let sin = yaw_rad.sin();
                let cos = yaw_rad.cos();
                let fwd = f64::from(rider_forward);
                let strf = f64::from(rider_strafe);
                let wish_x = -sin * fwd + cos * strf;
                let wish_z = cos * fwd + sin * strf;
                let cur_sq =
                    cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z;
                if cur_sq < 0.01 {
                    cart.velocity.x += wish_x * 0.1;
                    cart.velocity.z += wish_z * 0.1;
                    brake = false;
                }
            }

            if brake {
                let s =
                    (cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z).sqrt();
                if s < 0.03 {
                    cart.velocity = DVec3::ZERO;
                } else {
                    cart.velocity.x *= 0.5;
                    cart.velocity.y = 0.0;
                    cart.velocity.z *= 0.5;
                }
            }

            let x0 = f64::from(rpos.x) + 0.5 + f64::from(ends[0][0]) * 0.5;
            let z0 = f64::from(rpos.z) + 0.5 + f64::from(ends[0][2]) * 0.5;
            let x1 = f64::from(rpos.x) + 0.5 + f64::from(ends[1][0]) * 0.5;
            let z1 = f64::from(rpos.z) + 0.5 + f64::from(ends[1][2]) * 0.5;
            let dx = x1 - x0;
            let dz = z1 - z0;
            let t = if dx == 0.0 {
                cart.position.x = f64::from(rpos.x) + 0.5;
                cart.position.z - f64::from(rpos.z)
            } else if dz == 0.0 {
                cart.position.z = f64::from(rpos.z) + 0.5;
                cart.position.x - f64::from(rpos.x)
            } else {
                ((cart.position.x - x0) * dx + (cart.position.z - z0) * dz) * 2.0
            };
            cart.position.x = x0 + dx * t;
            cart.position.z = z0 + dz * t;
        }

        let max_s = self.minecarts[idx].max_speed();
        let factor = if ridden { 0.75 } else { 1.0 };
        let move_x = (self.minecarts[idx].velocity.x * factor).clamp(-max_s, max_s);
        let move_z = (self.minecarts[idx].velocity.z * factor).clamp(-max_s, max_s);
        let moved = collision::move_body_with_step(
            world,
            self.minecarts[idx].bounds(),
            DVec3::new(move_x, 0.0, move_z),
            false,
            false,
            0.0,
        );
        let clipped_horiz = moved.clipped[0] || moved.clipped[2];
        {
            let cart = &mut self.minecarts[idx];
            cart.position = moved.bounds.feet();
            if moved.clipped[0] {
                cart.velocity.x = 0.0;
            }
            if moved.clipped[2] {
                cart.velocity.z = 0.0;
            }
            let ends = RAIL_MATRIX[shape];
            let cell = cart.position.floor().as_ivec3();
            if ends[0][1] != 0
                && cell.x - rpos.x == ends[0][0]
                && cell.z - rpos.z == ends[0][2]
            {
                cart.position.y += f64::from(ends[0][1]);
            } else if ends[1][1] != 0
                && cell.x - rpos.x == ends[1][0]
                && cell.z - rpos.z == ends[1][2]
            {
                cart.position.y += f64::from(ends[1][1]);
            }

            // Furnace cart propulsion & base drag
            if cart.kind == MinecartKind::Furnace {
                let mut push_len_sq = cart.push_x * cart.push_x + cart.push_z * cart.push_z;
                if push_len_sq > 1e-4 {
                    let plen = push_len_sq.sqrt();
                    cart.push_x /= plen;
                    cart.push_z /= plen;
                    cart.velocity.x = cart.velocity.x * 0.8 + cart.push_x * 0.05;
                    cart.velocity.y = 0.0;
                    cart.velocity.z = cart.velocity.z * 0.8 + cart.push_z * 0.05;
                } else {
                    cart.velocity.x *= 0.98;
                    cart.velocity.y = 0.0;
                    cart.velocity.z *= 0.98;
                }
                push_len_sq = cart.push_x * cart.push_x + cart.push_z * cart.push_z;
                if push_len_sq > 1e-4
                    && cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z > 0.001
                {
                    let plen = push_len_sq.sqrt();
                    cart.push_x /= plen;
                    cart.push_z /= plen;
                    if cart.push_x * cart.velocity.x + cart.push_z * cart.velocity.z < 0.0 {
                        cart.push_x = 0.0;
                        cart.push_z = 0.0;
                    } else {
                        let speed =
                            (cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z)
                                .sqrt()
                                .max(1e-6);
                        cart.push_x = cart.velocity.x / speed * plen;
                        cart.push_z = cart.velocity.z / speed * plen;
                    }
                }
            }
            let drag = if ridden {
                f64::from(0.997f32)
            } else {
                f64::from(0.96f32)
            };
            cart.velocity.x *= drag;
            cart.velocity.y = 0.0;
            cart.velocity.z *= drag;
        }

        if let (Some(old_s), Some(new_s)) = (old_snap, snap_to_rail(world, self.minecarts[idx].position))
        {
            let cart = &mut self.minecarts[idx];
            let dy_speed = (old_s.y - new_s.y) * 0.05;
            let s = (cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z).sqrt();
            if s > 0.0 {
                cart.velocity.x = cart.velocity.x / s * (s + dy_speed);
                cart.velocity.z = cart.velocity.z / s * (s + dy_speed);
            }
            cart.position.y = new_s.y;
        }

        {
            let cart = &mut self.minecarts[idx];
            let cell = cart.position.floor().as_ivec3();
            if cell.x != rpos.x || cell.z != rpos.z {
                let s =
                    (cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z).sqrt();
                cart.velocity.x = s * f64::from(cell.x - rpos.x);
                cart.velocity.z = s * f64::from(cell.z - rpos.z);
            }
            if boost {
                let s =
                    (cart.velocity.x * cart.velocity.x + cart.velocity.z * cart.velocity.z).sqrt();
                if s > 0.01 {
                    cart.velocity.x += cart.velocity.x / s * 0.06;
                    cart.velocity.z += cart.velocity.z / s * 0.06;
                } else if shape == 1 {
                    if material::normal_cube(world.get(rpos - IVec3::X)) {
                        cart.velocity.x = 0.02;
                    } else if material::normal_cube(world.get(rpos + IVec3::X)) {
                        cart.velocity.x = -0.02;
                    }
                } else if shape == 0 {
                    if material::normal_cube(world.get(rpos - IVec3::Z)) {
                        cart.velocity.z = 0.02;
                    } else if material::normal_cube(world.get(rpos + IVec3::Z)) {
                        cart.velocity.z = -0.02;
                    }
                }
            }
        }

        // Furnace fuel tick & largesmoke particle
        if self.minecarts[idx].kind == MinecartKind::Furnace {
            if self.minecarts[idx].fuel > 0 {
                self.minecarts[idx].fuel -= 1;
            }
            if self.minecarts[idx].fuel <= 0 {
                self.minecarts[idx].push_x = 0.0;
                self.minecarts[idx].push_z = 0.0;
            } else {
                let mut rng = JavaRandom::from_state(self.particle_random[2]);
                if rng.next_int(4) == 0 && self.particles.len() < 4096 {
                    if let Some(p) = Particle::network_sprite(
                        12,
                        self.minecarts[idx].position + DVec3::Y * 0.8,
                        DVec3::ZERO,
                        &mut rng,
                    ) {
                        self.particles.push(p);
                    }
                }
                self.particle_random[2] = rng.state();
            }
        }
        let _ = &mut boost;
        clipped_horiz
    }

    fn step_derailed_minecart(&mut self, world: &World, idx: usize) -> (bool, f32) {
        let max_s = self.minecarts[idx].max_speed();
        {
            let cart = &mut self.minecarts[idx];
            cart.velocity.x = cart.velocity.x.clamp(-max_s, max_s);
            cart.velocity.z = cart.velocity.z.clamp(-max_s, max_s);
            if cart.on_ground {
                cart.velocity *= 0.5;
            }
        }
        let prev_y = self.minecarts[idx].position.y;
        let moved = collision::move_body_with_step(
            world,
            self.minecarts[idx].bounds(),
            self.minecarts[idx].velocity,
            self.minecarts[idx].on_ground,
            false,
            0.0,
        );
        let cart = &mut self.minecarts[idx];
        cart.position = moved.bounds.feet();
        let dy = cart.position.y - prev_y;
        let mut landed_fall = 0.0f32;
        if moved.grounded {
            landed_fall = cart.fall_distance;
            cart.fall_distance = 0.0;
        } else if dy < 0.0 {
            cart.fall_distance += (-dy) as f32;
        }
        cart.on_ground = moved.grounded;
        for axis in 0..3 {
            if moved.clipped[axis] {
                cart.velocity[axis] = 0.0;
            }
        }
        if !cart.on_ground {
            cart.velocity *= f64::from(0.95f32);
        }
        (moved.clipped[0] || moved.clipped[2], landed_fall)
    }

    fn collide_minecart_pair(&mut self, a: usize, b: usize) {
        let mut dx = self.minecarts[b].position.x - self.minecarts[a].position.x;
        let mut dz = self.minecarts[b].position.z - self.minecarts[a].position.z;
        let dist_sq = dx * dx + dz * dz;
        if dist_sq < 1e-4 {
            return;
        }
        let dist = dist_sq.sqrt();
        dx /= dist;
        dz /= dist;
        let scale = (1.0 / dist).min(1.0) * 0.1 * 0.5;
        let px = dx * scale;
        let pz = dz * scale;

        let a_furnace = self.minecarts[a].kind == MinecartKind::Furnace;
        let b_furnace = self.minecarts[b].kind == MinecartKind::Furnace;
        let sum_vx = self.minecarts[a].velocity.x + self.minecarts[b].velocity.x;
        let sum_vz = self.minecarts[a].velocity.z + self.minecarts[b].velocity.z;

        if b_furnace && !a_furnace {
            let bvx = self.minecarts[b].velocity.x;
            let bvz = self.minecarts[b].velocity.z;
            self.minecarts[a].velocity.x = self.minecarts[a].velocity.x * 0.2 + bvx - px;
            self.minecarts[a].velocity.z = self.minecarts[a].velocity.z * 0.2 + bvz - pz;
            self.minecarts[b].velocity.x *= 0.95;
            self.minecarts[b].velocity.z *= 0.95;
        } else if a_furnace && !b_furnace {
            let avx = self.minecarts[a].velocity.x;
            let avz = self.minecarts[a].velocity.z;
            self.minecarts[b].velocity.x = self.minecarts[b].velocity.x * 0.2 + avx + px;
            self.minecarts[b].velocity.z = self.minecarts[b].velocity.z * 0.2 + avz + pz;
            self.minecarts[a].velocity.x *= 0.95;
            self.minecarts[a].velocity.z *= 0.95;
        } else {
            let avg_x = sum_vx * 0.5;
            let avg_z = sum_vz * 0.5;
            self.minecarts[a].velocity.x = self.minecarts[a].velocity.x * 0.2 + avg_x - px;
            self.minecarts[a].velocity.z = self.minecarts[a].velocity.z * 0.2 + avg_z - pz;
            self.minecarts[b].velocity.x = self.minecarts[b].velocity.x * 0.2 + avg_x + px;
            self.minecarts[b].velocity.z = self.minecarts[b].velocity.z * 0.2 + avg_z + pz;
        }
    }

    fn tick_hopper_minecart_pull(
        &mut self,
        world: &mut World,
        cart_idx: usize,
        out: &mut Outcome,
    ) -> bool {
        let has_space = self.minecarts[cart_idx]
            .slots
            .iter()
            .any(|s| s.as_ref().is_none_or(|st| st.count < stack_limit(st.id)));
        if !has_space {
            return false;
        }
        let pos = self.minecarts[cart_idx].position;
        let above = DVec3::new(pos.x, pos.y + 0.5, pos.z).floor().as_ivec3() + IVec3::Y;

        if let Some(parts) = super::container_parts(world, above) {
            for part in parts {
                let slot_indices: Vec<usize> = {
                    let c = self.live_container(world, part);
                    if c.kind == Kind::Furnace {
                        vec![2, 1]
                    } else if c.special == Special::Brewing {
                        vec![0, 1, 2]
                    } else {
                        (0..c.slots.len()).collect()
                    }
                };
                for si in slot_indices {
                    let candidate = {
                        let c = self.live_container(world, part);
                        let Some(stack) = c.slots[si].as_ref() else {
                            continue;
                        };
                        if c.kind == Kind::Furnace && si == 1 && !matches!(stack.id, 325 | 326) {
                            continue;
                        }
                        let mut unit = stack.copied();
                        unit.count = 1;
                        unit
                    };
                    let mut unit = candidate;
                    if insert_into_slots(&mut self.minecarts[cart_idx].slots, &mut unit) {
                        {
                            let slot = &mut self.live_container(world, part).slots[si];
                            let orig = slot.as_mut().unwrap();
                            orig.count -= 1;
                            if orig.count == 0 {
                                *slot = None;
                            }
                        }
                        self.block_ticks.notify(world, part);
                        self.refresh_detector_rails_near(world, pos, out);
                        return true;
                    }
                }
            }
            return false;
        }

        // Check container minecart above
        let above_box = Aabb {
            min: above.as_dvec3(),
            max: (above + IVec3::ONE).as_dvec3(),
        };
        if let Some(other_idx) = self.minecarts.iter().position(|c| {
            c.id != self.minecarts[cart_idx].id
                && matches!(c.kind, MinecartKind::Chest | MinecartKind::Hopper)
                && above_box.intersects(c.bounds())
        }) {
            let n = self.minecarts[other_idx].slots.len();
            for si in 0..n {
                let Some(stack) = self.minecarts[other_idx].slots[si].as_ref() else {
                    continue;
                };
                let mut unit = stack.copied();
                unit.count = 1;
                if insert_into_slots(&mut self.minecarts[cart_idx].slots, &mut unit) {
                    let slot = &mut self.minecarts[other_idx].slots[si];
                    let orig = slot.as_mut().unwrap();
                    orig.count -= 1;
                    if orig.count == 0 {
                        *slot = None;
                    }
                    self.refresh_detector_rails_near(world, pos, out);
                    return true;
                }
            }
            return false;
        }

        // Suck nearby dropped items in [-0.5..0.5, 0.0..1.5, -0.5..0.5] and expanded cart bounds
        let top_suck = Aabb {
            min: DVec3::new(pos.x - 0.5, pos.y, pos.z - 0.5),
            max: DVec3::new(pos.x + 0.5, pos.y + 2.0, pos.z + 0.5),
        };
        let side_suck = self.minecarts[cart_idx]
            .bounds()
            .expand(DVec3::new(0.25, 0.0, 0.25));
        let mut sucked = false;
        for drop in &mut self.drops {
            if drop.dead || (!top_suck.intersects(drop.bounds) && !side_suck.intersects(drop.bounds))
            {
                continue;
            }
            if insert_into_slots(&mut self.minecarts[cart_idx].slots, &mut drop.stack) {
                sucked = true;
            }
            if drop.stack.count == 0 {
                drop.dead = true;
                break;
            }
        }
        if sucked {
            self.drops.retain(|d| !d.dead);
            self.refresh_detector_rails_near(world, pos, out);
        }
        sucked
    }

    fn tick_boats(&mut self, world: &mut World, player: &mut Player, out: &mut Outcome) {
        let mut forward = f32::from(player.keys.contains(&KeyCode::KeyW))
            - f32::from(player.keys.contains(&KeyCode::KeyS));
        if forward < 0.0 {
            forward *= 0.5;
        } else if forward > 0.0 {
            forward *= 0.98;
        }
        let strafe = (f32::from(player.keys.contains(&KeyCode::KeyA))
            - f32::from(player.keys.contains(&KeyCode::KeyD)))
            * 0.98;
        let rider_yaw = player.java_rotation()[0];
        let tile_drops = tile_drops_enabled(self);
        let entity_drops = self.entity_drops_enabled();

        let mut i = 0;
        while i < self.boats.len() {
            if !world.contains(self.boats[i].position.as_vec3()) {
                i += 1;
                continue;
            }
            let boat_id = self.boats[i].id;
            let ridden = self.ridden_boat == Some(boat_id);
            {
                let boat = &mut self.boats[i];
                if boat.hit_ticks > 0 {
                    boat.hit_ticks -= 1;
                }
                if boat.damage > 0.0 {
                    boat.damage = (boat.damage - 1.0).max(0.0);
                }
                boat.previous = boat.position;
                boat.prev_yaw = boat.yaw;
                boat.prev_pitch = boat.pitch;
            }
            if self.boats[i].position.y < -64.0 {
                let removed = self.boats.remove(i);
                if ridden {
                    self.dismount_from_vehicle(world, player, removed.position);
                }
                continue;
            }

            // Water flow acceleration (matching remote_vehicle.rs)
            let flow_query = Aabb {
                min: self.boats[i].position
                    + DVec3::new(
                        -0.749,
                        f64::from(0.6f32) - f64::from(0.4f32) + 0.001,
                        -0.749,
                    ),
                max: self.boats[i].position + DVec3::new(0.749, f64::from(0.4f32) - 0.001, 0.749),
            };
            if let Some(flow) = movement::water_acceleration(world, flow_query).1 {
                self.boats[i].velocity += flow * 0.014;
            }

            // 5-slice water submersion buoyancy
            let bounds = self.boats[i].bounds();
            let mut submersion = 0.0;
            for slice in 0..5 {
                let min_y =
                    bounds.min.y + (bounds.max.y - bounds.min.y) * f64::from(slice) / 5.0 - 0.125;
                let max_y = bounds.min.y
                    + (bounds.max.y - bounds.min.y) * f64::from(slice + 1) / 5.0
                    - 0.125;
                let mut wet = false;
                'water: for x in (bounds.min.x.floor() as i32)..=(bounds.max.x.floor() as i32) {
                    for y in (min_y.floor() as i32)..=(max_y.floor() as i32) {
                        for z in (bounds.min.z.floor() as i32)..=(bounds.max.z.floor() as i32) {
                            let s = world.get(IVec3::new(x, y, z));
                            if matches!(s >> 4, 8 | 9) {
                                let surface = f64::from(y + 1)
                                    - if s & 15 < 8 {
                                        f64::from(s & 15) / 8.0
                                    } else {
                                        0.0
                                    };
                                if surface >= min_y {
                                    wet = true;
                                    break 'water;
                                }
                            }
                        }
                    }
                }
                if wet {
                    submersion += 0.2;
                }
            }

            let prev_horiz_speed = {
                let v = self.boats[i].velocity;
                (v.x * v.x + v.z * v.z).sqrt()
            };

            {
                let boat = &mut self.boats[i];
                if submersion < 1.0 {
                    boat.velocity.y += f64::from(0.04f32) * (submersion * 2.0 - 1.0);
                } else {
                    if boat.velocity.y < 0.0 {
                        boat.velocity.y *= 0.5;
                    }
                    boat.velocity.y += f64::from(0.007f32);
                }

                if ridden {
                    let angle =
                        f64::from((rider_yaw - strafe * 90.0) * std::f32::consts::PI / 180.0);
                    boat.velocity.x -= angle.sin()
                        * boat.speed_multiplier
                        * f64::from(forward)
                        * f64::from(0.05f32);
                    boat.velocity.z += angle.cos()
                        * boat.speed_multiplier
                        * f64::from(forward)
                        * f64::from(0.05f32);
                }

                let mut speed =
                    (boat.velocity.x * boat.velocity.x + boat.velocity.z * boat.velocity.z).sqrt();
                if speed > 0.35 {
                    let factor = 0.35 / speed;
                    boat.velocity.x *= factor;
                    boat.velocity.z *= factor;
                    speed = 0.35;
                }
                if speed > prev_horiz_speed && boat.speed_multiplier < 0.35 {
                    boat.speed_multiplier =
                        (boat.speed_multiplier + (0.35 - boat.speed_multiplier) / 35.0).min(0.35);
                } else {
                    boat.speed_multiplier =
                        (boat.speed_multiplier - (boat.speed_multiplier - 0.07) / 35.0).max(0.07);
                }
            }

            // Break adjacent snow_layer (78) and waterlily (111)
            let swept = self.boats[i]
                .bounds()
                .sweep(DVec3::new(
                    self.boats[i].velocity.x,
                    0.0,
                    self.boats[i].velocity.z,
                ));
            let base_y = self.boats[i].position.y.floor() as i32;
            let min_bx = swept.min.x.floor() as i32;
            let max_bx = swept.max.x.floor() as i32;
            let min_bz = swept.min.z.floor() as i32;
            let max_bz = swept.max.z.floor() as i32;
            for bx in min_bx..=max_bx {
                for bz in min_bz..=max_bz {
                    for dy in 0..2 {
                        let bp = IVec3::new(bx, base_y + dy, bz);
                        let id = world.get(bp) >> 4;
                        if id == 78 && world.set(bp, 0) {
                            out.changed.push(bp);
                            self.block_ticks.notify(world, bp);
                        } else if id == 111 && world.set(bp, 0) {
                            out.changed.push(bp);
                            self.block_ticks.notify(world, bp);
                            if tile_drops {
                                self.spawn_item_entity(
                                    Stack::new(111, 1),
                                    bp.as_dvec3() + DVec3::splat(0.5),
                                    DVec3::ZERO,
                                    10,
                                );
                            }
                        }
                    }
                }
            }

            if self.boats[i].grounded {
                self.boats[i].velocity *= 0.5;
            }
            let moved = collision::move_body_with_step(
                world,
                self.boats[i].bounds(),
                self.boats[i].velocity,
                self.boats[i].grounded,
                false,
                0.0,
            );
            let clipped_horiz = moved.clipped[0] || moved.clipped[2];
            self.boats[i].position = moved.bounds.feet();
            self.boats[i].grounded = moved.grounded;
            for axis in 0..3 {
                if moved.clipped[axis] {
                    self.boats[i].velocity[axis] = 0.0;
                }
            }

            if clipped_horiz && prev_horiz_speed > 0.2975 {
                let crashed = self.boats.remove(i);
                if ridden {
                    self.dismount_from_vehicle(world, player, crashed.position);
                }
                if entity_drops {
                    for _ in 0..3 {
                        self.spawn_item_entity(
                            Stack::new(5, 1),
                            crashed.position,
                            DVec3::ZERO,
                            10,
                        );
                    }
                    for _ in 0..2 {
                        self.spawn_item_entity(
                            Stack::new(280, 1),
                            crashed.position,
                            DVec3::ZERO,
                            10,
                        );
                    }
                }
                continue;
            }

            {
                let boat = &mut self.boats[i];
                boat.velocity *=
                    DVec3::new(f64::from(0.99f32), f64::from(0.95f32), f64::from(0.99f32));
                boat.pitch = 0.0;
                let mut wanted = f64::from(boat.yaw);
                let dx = boat.previous.x - boat.position.x;
                let dz = boat.previous.z - boat.position.z;
                if dx * dx + dz * dz > 0.001 {
                    wanted = ai::motion::atan2(dz, dx) * 180.0 / std::f64::consts::PI;
                }
                let turn = wrap_degrees_f64(wanted - f64::from(boat.yaw)).clamp(-20.0, 20.0);
                boat.yaw = (f64::from(boat.yaw) + turn) as f32;
            }

            i += 1;
        }

        // Boat-boat soft push
        let len = self.boats.len();
        for a in 0..len {
            for b in (a + 1)..len {
                if self.boats[a]
                    .bounds()
                    .expand(DVec3::new(0.2, 0.0, 0.2))
                    .intersects(self.boats[b].bounds())
                {
                    let mut dx = self.boats[b].position.x - self.boats[a].position.x;
                    let mut dz = self.boats[b].position.z - self.boats[a].position.z;
                    let dist_sq = dx * dx + dz * dz;
                    if dist_sq >= 1e-4 {
                        let dist = dist_sq.sqrt();
                        dx /= dist;
                        dz /= dist;
                        let scale = (1.0 / dist).min(1.0) * 0.05;
                        self.boats[a].velocity.x -= dx * scale;
                        self.boats[a].velocity.z -= dz * scale;
                        self.boats[b].velocity.x += dx * scale;
                        self.boats[b].velocity.z += dz * scale;
                    }
                }
            }
        }
    }

    pub(crate) fn drive_ridden_pig(&mut self, world: &World, mob_idx: usize, player: &Player) {
        if mob_idx >= self.mobs.len() {
            return;
        }
        let mob_id = self.mobs[mob_idx].id;
        if self.mobs[mob_idx].kind != MobKind::Pig || self.mobs[mob_idx].health <= 0.0 {
            return;
        }
        let holding_stick = self.inventory.held().is_some_and(|s| s.id == 398);
        let boosting = self.pig_boost_ticks > 0;
        if holding_stick || boosting {
            if self.pig_speed < 0.07 {
                self.pig_speed = 0.07;
            }
            self.pig_speed = (self.pig_speed + (0.3 - self.pig_speed) * 0.02 + 0.002).min(0.3);
        } else {
            self.pig_speed = (self.pig_speed - 0.03).max(0.0);
        }

        let boost_mult = if boosting && self.pig_boost_total > 0 {
            let elapsed = (self.pig_boost_total.saturating_sub(self.pig_boost_ticks)) as f32
                / self.pig_boost_total as f32;
            1.0 + 1.15 * (elapsed * std::f32::consts::PI).sin()
        } else {
            1.0
        };
        let effective_speed = self.pig_speed * boost_mult;

        let data = self.command_entities.mobs.get(&mob_id);
        let scale = if self
            .mob_brains
            .get(&mob_id)
            .is_some_and(|b| b.growing_age < 0)
            || species_ai::number(data, "IsBaby") != 0
        {
            0.5
        } else {
            1.0
        };
        let (adult_w, adult_h) = MobKind::Pig.adult_size(data);
        let size = (adult_w * scale, adult_h * scale);

        let m = &mut self.mobs[mob_idx];
        m.previous = m.position;

        let target_java_yaw = player.java_rotation()[0];
        let cur_java_yaw = m.yaw.to_degrees() - 180.0;
        let next_java_yaw = if holding_stick || boosting || effective_speed > 0.0 {
            let delta = wrap_degrees_f32(target_java_yaw - cur_java_yaw).clamp(-10.0, 10.0);
            cur_java_yaw + delta
        } else {
            cur_java_yaw
        };
        m.yaw = (next_java_yaw + 180.0).to_radians();

        let brain = self.mob_brains.entry(mob_id).or_default();
        brain.navigator.clear();
        brain.path_target = None;
        brain.path_timer = 0;

        let movement = brain.movement.get_or_insert_with(|| {
            Box::new(ai::locomotion::Locomotion::new(
                m.position.as_dvec3(),
                size.0,
                size.1,
                next_java_yaw,
                brain.grounded,
            ))
        });
        movement.reconcile(world, m, size);
        let body = &mut movement.body;
        body.previous = body.position;
        body.step_height = Some(1.0);
        body.jump_delay = body.jump_delay.saturating_sub(1);

        let (water, flow) = movement::water_contact(world, body.bounds);
        if let Some(flow) = flow {
            body.velocity += flow * 0.014;
        }
        let lava = movement::touches_liquid(
            world,
            body.bounds.expand(DVec3::new(-0.1, -0.4, -0.1)),
            10,
            11,
        );
        body.in_water = water;
        body.in_lava = lava;

        if (water || lava) && effective_speed > 0.0 {
            body.velocity.y += f64::from(0.04f32);
        } else if body.collided_horizontal
            && body.grounded
            && body.jump_delay == 0
            && effective_speed > 0.0
        {
            body.velocity.y = f64::from(0.42f32);
            body.jump_delay = 10;
        }

        let forward = if effective_speed > 0.0 { 0.98 } else { 0.0 };
        body.travel(
            world,
            Travel {
                strafe: 0.0,
                forward,
                yaw: next_java_yaw,
                sneak: false,
                depth_strider: 0,
                water,
                lava,
                land_speed: effective_speed,
                air_speed: effective_speed * 0.1,
                ladder_override: None,
                server: true,
            },
        );

        m.velocity = body.velocity.as_vec3();
        m.position = body.position.as_vec3();
        brain.grounded = body.grounded;
        brain
            .gait
            .advance(m.yaw, (m.position - m.previous).with_y(0.0).length());
        movement.control.yaw = next_java_yaw;
    }

    pub(crate) fn build_local_vehicles(&self, d: &mut Dynamic, c: &ModelContext<'_>) {
        let reach = c.settings.entity_distance as f32;
        let reach_sq = reach * reach;
        for cart in &self.minecarts {
            let actor = RenderActor {
                cart: true,
                from: cart.previous,
                to: cart.position,
                previous_yaw: cart.prev_yaw,
                yaw: cart.yaw,
                previous_pitch: cart.prev_pitch,
                pitch: cart.pitch,
                damage: RenderDamage {
                    ticks: cart.hit_ticks,
                    direction: cart.hit_dir,
                    amount: cart.damage,
                },
                display: cart.kind.display(cart.fuel),
            };
            let tnt_fuse = (cart.kind == MinecartKind::Tnt).then_some(cart.fuse);
            vehicle_model::draw_with_tnt_fuse(d, c, cart.id as i32, &actor, tnt_fuse);
            if c.hitboxes {
                let feet = cart.previous.lerp(cart.position, f64::from(c.alpha)).as_vec3();
                if feet.distance_squared(c.eye) <= reach_sq {
                    models::wirebox(
                        &mut d.opaque,
                        feet - Vec3::new(0.49, 0.0, 0.49),
                        feet + Vec3::new(0.49, 0.7, 0.49),
                    );
                }
            }
        }
        for boat in &self.boats {
            let actor = RenderActor {
                cart: false,
                from: boat.previous,
                to: boat.position,
                previous_yaw: boat.prev_yaw,
                yaw: boat.yaw,
                previous_pitch: boat.prev_pitch,
                pitch: boat.pitch,
                damage: RenderDamage {
                    ticks: boat.hit_ticks,
                    direction: boat.hit_dir,
                    amount: boat.damage,
                },
                display: None,
            };
            vehicle_model::draw_with_tnt_fuse(d, c, boat.id as i32, &actor, None);
            if c.hitboxes {
                let feet = boat.previous.lerp(boat.position, f64::from(c.alpha)).as_vec3();
                if feet.distance_squared(c.eye) <= reach_sq {
                    models::wirebox(
                        &mut d.opaque,
                        feet - Vec3::new(0.75, 0.0, 0.75),
                        feet + Vec3::new(0.75, 0.6, 0.75),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use std::sync::Arc;

    fn flat_stone_world() -> World {
        let mut chunk = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 3, z, 1 << 4);
            }
        }
        World {
            chunks: [((0, 0), Arc::new(chunk))].into(),
            spawn: Vec3::new(8.5, 4.0, 8.5),
            label: String::new(),
            save_path: Default::default(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }

    #[test]
    fn minecart_powered_launch_curves_slopes_detector_comparator_activator_tnt_and_hopper_suction() {
        let mut world = flat_stone_world();
        let mut sim = Simulation::default();
        let mut player = Player::new(Vec3::new(2.5, 4.0, 2.5));
        let mut out = Outcome::default();

        // Wall at (2, 4, 5), powered rail at (3, 4, 5) EW, curve at (4, 4, 5) SW (7), detector at (4, 5, 5)
        world.set(IVec3::new(2, 4, 5), 1 << 4);
        world.set(IVec3::new(3, 4, 5), 27 << 4 | 1 | 8);
        world.set(IVec3::new(4, 4, 5), 66 << 4 | 7);
        world.set(IVec3::new(4, 4, 6), 28 << 4);
        world.set(IVec3::new(4, 4, 7), 157 << 4 | 8);

        sim.inventory.slots[0] = Some(Stack::new(328, 1));
        assert!(sim.try_place_minecart_on_rail(
            &mut world,
            &player,
            IVec3::new(3, 4, 5),
            328,
            &mut out
        ));
        assert!(sim.inventory.slots[0].is_none());
        let ride_id = sim.minecarts[0].id;
        assert!(sim.interact_minecart(ride_id, &mut world, &mut player, &mut out));
        assert_eq!(sim.ridden_minecart, Some(ride_id));

        for _ in 0..35 {
            sim.tick_local_vehicles(&mut world, &mut player, &mut out);
        }
        // Cart launched off wall (+X), turned South around curve (4,4,5), crossed detector (4,4,6),
        // and hit powered activator rail (4,4,7) which dismounted the player!
        assert!(world.get(IVec3::new(4, 4, 6)) & 8 != 0 || out.changed.contains(&IVec3::new(4, 4, 6)));
        assert_eq!(sim.ridden_minecart, None);

        // Chest cart on detector rail gives Vanilla comparator signal
        let det_pos = IVec3::new(8, 4, 8);
        world.set(det_pos, 28 << 4);
        assert!(sim.try_place_minecart_on_rail(&mut world, &player, det_pos, 342, &mut out));
        assert_ne!(world.get(det_pos) & 8, 0);
        assert_eq!(sim.detector_rail_comparator_signal(det_pos, world.get(det_pos)), 0);
        let chest_idx = sim
            .minecarts
            .iter()
            .position(|c| c.kind == MinecartKind::Chest)
            .unwrap();
        sim.minecarts[chest_idx].slots[0] = Some(Stack::new(264, 64));
        assert!(sim.detector_rail_comparator_signal(det_pos, world.get(det_pos)) >= 1);

        // Slope rail ascent & descent + TNT ignition on powered activator rail
        world.set(IVec3::new(10, 4, 2), 27 << 4 | 1 | 8);
        world.set(IVec3::new(9, 4, 2), 1 << 4);
        world.set(IVec3::new(12, 4, 2), 1 << 4);
        world.set(IVec3::new(11, 4, 2), 66 << 4 | 2); // ascending east
        world.set(IVec3::new(12, 5, 2), 157 << 4 | 1 | 8); // powered activator at top of slope
        assert!(sim.try_place_minecart_on_rail(
            &mut world,
            &player,
            IVec3::new(10, 4, 2),
            407,
            &mut out
        ));
        let tnt_id = sim.minecarts.last().unwrap().id;
        let mut ignited = false;
        for _ in 0..30 {
            sim.tick_local_vehicles(&mut world, &mut player, &mut out);
            if let Some(tnt_cart) = sim.minecarts.iter().find(|c| c.id == tnt_id) {
                if tnt_cart.fuse > 0 && tnt_cart.position.y >= 5.0 {
                    ignited = true;
                    break;
                }
            }
        }
        assert!(ignited, "TNT cart should climb slope and ignite on powered activator rail");

        // Hopper cart sucks nearby item drop
        let hop_pos = IVec3::new(6, 4, 12);
        world.set(hop_pos, 66 << 4);
        assert!(sim.try_place_minecart_on_rail(&mut world, &player, hop_pos, 408, &mut out));
        let hop_id = sim.minecarts.last().unwrap().id;
        sim.spawn_item_entity(
            Stack::new(265, 5),
            hop_pos.as_dvec3() + DVec3::new(0.5, 0.8, 0.5),
            DVec3::ZERO,
            40,
        );
        sim.tick_local_vehicles(&mut world, &mut player, &mut out);
        let hop_cart = sim.minecarts.iter().find(|c| c.id == hop_id).unwrap();
        assert_eq!(hop_cart.slots[0], Some(Stack::new(265, 5)));
    }

    #[test]
    fn boat_places_on_water_floats_breaks_lilypad_and_crashes_into_wall_for_planks_and_sticks() {
        let mut world = flat_stone_world();
        for x in 1..15 {
            for z in 2..15 {
                world.set(IVec3::new(x, 4, z), 9 << 4); // stationary water
            }
        }
        // Lilypad ahead at (6, 5, 8) and stone wall at (14, 5, 8) & (14, 4, 8)
        world.set(IVec3::new(6, 5, 8), 111 << 4);
        for z in 6..=10 {
            world.set(IVec3::new(14, 4, z), 1 << 4);
            world.set(IVec3::new(14, 5, z), 1 << 4);
        }

        let mut sim = Simulation::default();
        sim.inventory.slots[0] = Some(Stack::new(333, 1));
        // Stand at (1.5, 5.0, 8.5) looking down into water at (2, 4, 8)
        let mut player = Player::new(Vec3::new(1.5, 5.0, 8.5));
        player.set_rotation_degrees(-90.0, 45.0);
        let mut out = Outcome::default();
        assert!(sim.try_place_boat(&world, &player, &mut out));
        assert_eq!(sim.boats.len(), 1);
        assert!(sim.inventory.slots[0].is_none());

        let boat_id = sim.boats[0].id;
        assert!(sim.interact_boat(boat_id, &world, &mut player));
        assert_eq!(sim.ridden_boat, Some(boat_id));

        // Steer East (+X -> java yaw = -90.0) with KeyW held across 10.75 blocks of water
        player.set_rotation_degrees(-90.0, 0.0);
        player.keys.insert(KeyCode::KeyW);
        for _ in 0..100 {
            sim.tick_local_vehicles(&mut world, &mut player, &mut out);
            if sim.boats.is_empty() {
                break;
            }
        }

        // Lilypad broken and dropped 111; boat crashed into wall at >0.2975 speed dropping 3 planks + 2 sticks!
        assert_eq!(world.get(IVec3::new(6, 5, 8)), 0);
        assert!(sim.boats.is_empty());
        assert_eq!(sim.ridden_boat, None);
        let lilypads: u32 = sim
            .drops
            .iter()
            .filter(|d| d.stack.id == 111)
            .map(|d| u32::from(d.stack.count))
            .sum();
        let planks: u32 = sim
            .drops
            .iter()
            .filter(|d| d.stack.id == 5 && d.stack.meta == 0)
            .map(|d| u32::from(d.stack.count))
            .sum();
        let sticks: u32 = sim
            .drops
            .iter()
            .filter(|d| d.stack.id == 280)
            .map(|d| u32::from(d.stack.count))
            .sum();
        assert_eq!(lilypads, 1);
        assert_eq!(planks, 3);
        assert_eq!(sticks, 2);
    }

    #[test]
    fn ridden_pig_travels_along_heading_steps_up_one_block_ledge_and_boosts_with_carrot_stick() {
        let mut world = flat_stone_world();
        // 1-block ledge across z = 7 at y = 4
        for x in 0..16 {
            for z in 7..14 {
                world.set(IVec3::new(x, 4, z), 1 << 4);
            }
        }
        let mut sim = Simulation::default();
        let pig_id = sim.spawn_registered_raw(MobKind::Pig, Vec3::new(8.5, 4.0, 5.0), std::f32::consts::PI);
        sim.ridden_pig = Some(pig_id);
        sim.inventory.slots[0] = Some(Stack::new(398, 1));

        let mut player = Player::new(Vec3::new(8.5, 4.5, 5.0));
        player.set_rotation_degrees(0.0, 0.0); // Facing +Z

        // Settle on ground and walk without boost
        for _ in 0..25 {
            sim.drive_ridden_pig(&world, 0, &player);
        }
        let unboosted_step = (sim.mobs[0].position - sim.mobs[0].previous).with_y(0.0).length();
        assert!(
            sim.mobs[0].position.z > 7.1 && sim.mobs[0].position.y >= 5.0,
            "Pig should step up 1.0-block ledge at z=7: {:?}",
            sim.mobs[0].position
        );

        // Trigger boost curve at midpoint (sin(PI/2) = 1.0 => 2.15x speed)
        sim.pig_boost_total = 200;
        sim.pig_boost_ticks = 100;
        for _ in 0..5 {
            sim.drive_ridden_pig(&world, 0, &player);
        }
        let boosted_step = (sim.mobs[0].position - sim.mobs[0].previous).with_y(0.0).length();
        assert!(
            boosted_step > unboosted_step * 1.3,
            "Carrot-on-a-stick boost should accelerate pig ({unboosted_step} -> {boosted_step})"
        );
    }
}
