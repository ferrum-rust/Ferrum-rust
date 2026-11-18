//! Original 1.8.9 SP/LivingBase/Entity movement arithmetic. Position, motion and
//! bounding boxes remain doubles; input/attributes retain Java float rounding.
//! Sleeping, riding, entity pushing and spectator motion require additional
//! inputs. Movement attributes, jump boost, blindness and Depth Strider follow
//! the original SP/LivingBase paths with explicit caller-supplied state.
use super::{material, math};
use glam::{DVec3, IVec3};
#[path = "movement_attribute.rs"]
pub mod attribute;
#[path = "movement_collision.rs"]
pub(crate) mod collision;
#[cfg(test)]
#[path = "movement_effect_tests.rs"]
mod effect_tests;
pub use collision::Aabb;
pub trait MovementWorld {
    fn get(&self, p: IVec3) -> u16;
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<Aabb>);
    fn loaded_column(&self, x: i32, z: i32) -> bool;
    fn extra_colliders(&self, _query: Aabb, _out: &mut Vec<Aabb>) {}
}
#[derive(Clone, Copy, Debug, Default)]
pub struct Input {
    pub forward: f32,
    pub strafe: f32,
    pub jump: bool,
    pub sneak: bool,
    pub sprint_key: bool,
    pub can_sprint: bool,
    pub using_item: bool,
    pub yaw: f32,
    pub allow_flying: bool,
    pub fly_speed: f32,
    pub jump_boost: Option<i32>,
    pub blinded: bool,
    pub depth_strider: i32,
    pub suppress_view_input: bool,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Travel {
    pub strafe: f32,
    pub forward: f32,
    pub yaw: f32,
    pub sneak: bool,
    pub depth_strider: i32,
    pub water: bool,
    pub lava: bool,
    pub land_speed: f32,
    pub air_speed: f32,
    pub ladder_override: Option<bool>,
    pub server: bool,
}
#[derive(Clone, Copy, Debug)]
pub(crate) struct Landing {
    pub distance: f32,
    pub state: u16,
    pub position: IVec3,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct State {
    #[serde(skip)]
    pub(crate) step_height: Option<f32>,
    #[serde(default)]
    pub(crate) spectator: bool,
    pub position: DVec3,
    pub previous: DVec3,
    pub velocity: DVec3,
    pub bounds: Aabb,
    pub grounded: bool,
    pub flying: bool,
    pub sprinting: bool,
    pub in_water: bool,
    #[serde(default)]
    pub(crate) fall_distance: f32,
    #[serde(skip)]
    pub(crate) landing: Option<Landing>,
    #[serde(skip)]
    pub(crate) in_lava: bool,
    #[serde(skip)]
    pub(crate) touching_fire: bool,
    #[serde(skip)]
    pub(crate) touching_cactus: bool,
    #[serde(skip)]
    pub touching_portal: bool,
    #[serde(skip)]
    pub first_portal_block: Option<IVec3>,
    #[serde(skip)]
    pub touching_end_portal: bool,
    pub on_ladder: bool,
    pub eyes_in_water: bool,
    pub jumped: bool,
    pub jump_sprinting: bool,
    pub collided_horizontal: bool,
    pub jump_delay: u8,
    pub sprint_toggle_ticks: u8,
    pub sprint_ticks: u16,
    pub air_speed: f32,
    pub fly_toggle_ticks: u8,
    /// Original sendPlayerAbilities calls, including toggle and landing in
    /// the same tick. Remote adapters send each event in order.
    pub flight_changes: Vec<bool>,
    pub movement_attribute: attribute::Attribute,
    previous_forward: f32,
    previous_sneak: bool,
    previous_jump: bool,
    pub(crate) in_web: bool,
}
impl State {
    pub fn new(position: DVec3) -> Self {
        Self {
            step_height: None,
            spectator: false,
            position,
            previous: position,
            velocity: DVec3::ZERO,
            bounds: Aabb::player(position),
            grounded: false,
            flying: false,
            sprinting: false,
            in_water: false,
            fall_distance: 0.,
            landing: None,
            in_lava: false,
            touching_fire: false,
            touching_cactus: false,
            touching_portal: false,
            first_portal_block: None,
            touching_end_portal: false,
            on_ladder: false,
            eyes_in_water: false,
            jumped: false,
            jump_sprinting: false,
            collided_horizontal: false,
            jump_delay: 0,
            sprint_toggle_ticks: 0,
            sprint_ticks: 0,
            air_speed: 0.02,
            fly_toggle_ticks: 0,
            flight_changes: Vec::with_capacity(2),
            movement_attribute: attribute::Attribute::default(),
            previous_forward: 0.,
            previous_sneak: false,
            previous_jump: false,
            in_web: false,
        }
    }
    pub fn set_position(&mut self, p: DVec3) {
        self.position = p;
        self.bounds = Aabb::player(p);
    }
    pub(crate) fn set_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.sprint_ticks = if sprinting { 600 } else { 0 };
        self.movement_attribute
            .set_modifier(attribute::SPRINT, sprinting.then_some(0.3f32 as f64));
    }
    fn push_out(&mut self, world: &impl MovementWorld, p: DVec3) {
        if self.spectator {
            return;
        }
        let cell = p.floor().as_ivec3();
        let open = |q| {
            !material::normal_cube(world.get(q)) && !material::normal_cube(world.get(q + IVec3::Y))
        };
        if open(cell) {
            return;
        }
        let dx = p.x - cell.x as f64;
        let dz = p.z - cell.z as f64;
        let mut nearest = 9999.;
        let mut side = None;
        for (d, distance) in [
            (-IVec3::X, dx),
            (IVec3::X, 1. - dx),
            (-IVec3::Z, dz),
            (IVec3::Z, 1. - dz),
        ] {
            if open(cell + d) && distance < nearest {
                nearest = distance;
                side = Some(d);
            }
        }
        if let Some(d) = side {
            if d.x != 0 {
                self.velocity.x = d.x as f64 * 0.1f32 as f64;
            }
            if d.z != 0 {
                self.velocity.z = d.z as f64 * 0.1f32 as f64;
            }
        }
    }
    fn move_flying(&mut self, mut strafe: f32, mut forward: f32, friction: f32, yaw: f32) {
        let length = strafe * strafe + forward * forward;
        if length < 1.0e-4f32 {
            return;
        }
        let mut length = (length as f64).sqrt() as f32;
        if length < 1. {
            length = 1.;
        }
        let scale = friction / length;
        let s = math::sin(yaw * std::f32::consts::PI / 180.);
        let c = math::cos(yaw * std::f32::consts::PI / 180.);
        strafe *= scale;
        forward *= scale;
        self.velocity.x += (strafe * c - forward * s) as f64;
        self.velocity.z += (forward * c + strafe * s) as f64;
    }
    pub fn tick(&mut self, world: &impl MovementWorld, mut input: Input) {
        self.previous = self.position;
        self.jumped = false;
        self.landing = None;
        self.touching_fire = false;
        self.touching_cactus = false;
        self.touching_portal = false;
        self.first_portal_block = None;
        self.touching_end_portal = false;
        self.jump_sprinting = false;
        self.flight_changes.clear();
        input.can_sprint |= input.allow_flying;
        // Entity.handleWaterMovement precedes the SP input handler.
        let (water, current) = water_contact(world, self.bounds);
        self.in_water = water;
        if water {
            self.fall_distance = 0.;
        }
        if let Some(current) = current.filter(|_| !self.flying) {
            self.velocity += current * 0.014;
        }
        if self.sprint_ticks > 0 {
            self.sprint_ticks -= 1;
            if self.sprint_ticks == 0 {
                self.set_sprinting(false);
            }
        }
        self.sprint_toggle_ticks = self.sprint_toggle_ticks.saturating_sub(1);
        let was_forward = self.previous_forward >= 0.8;
        let was_sneak = self.previous_sneak;
        if input.sneak {
            input.forward = (input.forward as f64 * 0.3) as f32;
            input.strafe = (input.strafe as f64 * 0.3) as f32;
        }
        if input.using_item {
            input.forward *= 0.2;
            input.strafe *= 0.2;
            self.sprint_toggle_ticks = 0;
        }
        let radius = 0.6f32 as f64 * 0.35;
        let y = self.bounds.min.y + 0.5;
        for (dx, dz) in [
            (-radius, radius),
            (-radius, -radius),
            (radius, -radius),
            (radius, radius),
        ] {
            self.push_out(
                world,
                DVec3::new(self.position.x + dx, y, self.position.z + dz),
            );
        }
        if self.grounded
            && !was_sneak
            && !was_forward
            && input.forward >= 0.8
            && !self.sprinting
            && input.can_sprint
            && !input.using_item
            && !input.blinded
        {
            if self.sprint_toggle_ticks > 0 || input.sprint_key {
                self.set_sprinting(true);
            } else {
                self.sprint_toggle_ticks = 7;
            }
        }
        if !self.sprinting
            && input.forward >= 0.8
            && input.can_sprint
            && !input.using_item
            && !input.blinded
            && input.sprint_key
        {
            self.set_sprinting(true);
        }
        if self.sprinting && (input.forward < 0.8 || self.collided_horizontal || !input.can_sprint)
        {
            self.set_sprinting(false);
        }
        self.movement_attribute
            .set_modifier(attribute::SPRINT, self.sprinting.then_some(0.3f32 as f64));
        self.previous_forward = input.forward;
        self.previous_sneak = input.sneak;
        if self.spectator {
            self.grounded = false;
            if !self.flying {
                self.flying = true;
                self.flight_changes.push(true);
            }
        } else if input.allow_flying && !self.previous_jump && input.jump {
            if self.fly_toggle_ticks == 0 {
                self.fly_toggle_ticks = 7;
            } else {
                self.flying = !self.flying;
                self.flight_changes.push(self.flying);
                self.fly_toggle_ticks = 0;
            }
        }
        self.previous_jump = input.jump;
        if self.flying && !input.suppress_view_input {
            if input.sneak {
                self.velocity.y -= (input.fly_speed * 3.) as f64;
            }
            if input.jump {
                self.velocity.y += (input.fly_speed * 3.) as f64;
            }
        }
        // EntityPlayer's timer decrement is after the SP edge handler and
        // before LivingBase's jump/motion stages, including the setting tick.
        self.fly_toggle_ticks = self.fly_toggle_ticks.saturating_sub(1);
        self.jump_delay = self.jump_delay.saturating_sub(1);
        for i in 0..3 {
            if self.velocity[i].abs() < 0.005 {
                self.velocity[i] = 0.;
            }
        }
        let lava = touches_liquid(
            world,
            self.bounds
                .expand(DVec3::new(-0.1f32 as f64, -0.4f32 as f64, -0.1f32 as f64)),
            10,
            11,
        );
        self.in_lava = lava;
        if lava {
            self.fall_distance *= 0.5;
        }
        if input.suppress_view_input {
            input.strafe = 0.;
            input.forward = 0.;
            input.jump = false;
        }
        if input.jump {
            if water || lava {
                self.velocity.y += 0.04f32 as f64;
            } else if self.grounded && self.jump_delay == 0 {
                self.velocity.y = 0.42f32 as f64;
                if let Some(amplifier) = input.jump_boost {
                    self.velocity.y += (amplifier.wrapping_add(1) as f32 * 0.1f32) as f64;
                }
                self.jump_delay = 10;
                self.jumped = true;
                self.jump_sprinting = self.sprinting;
                if self.sprinting {
                    let angle = input.yaw * (std::f32::consts::PI / 180.);
                    self.velocity.x -= (math::sin(angle) * 0.2) as f64;
                    self.velocity.z += (math::cos(angle) * 0.2) as f64;
                }
            }
        } else {
            self.jump_delay = 0;
        }
        input.forward *= 0.98;
        input.strafe *= 0.98;
        self.travel(
            world,
            Travel {
                strafe: input.strafe,
                forward: input.forward,
                yaw: input.yaw,
                sneak: input.sneak,
                depth_strider: input.depth_strider,
                water,
                lava,
                land_speed: self.movement_attribute.value() as f32,
                air_speed: if self.flying {
                    input.fly_speed * if self.sprinting { 2. } else { 1. }
                } else {
                    self.air_speed
                },
                ladder_override: None,
                server: false,
            },
        );
        self.air_speed = if self.sprinting {
            (0.02f32 as f64 + 0.02f32 as f64 * 0.3) as f32
        } else {
            0.02
        };
        if self.grounded && self.flying && !self.spectator {
            self.flying = false;
            self.flight_changes.push(false);
        }
        self.on_ladder = !self.spectator && ladder(world, self.position, self.bounds);
        self.eyes_in_water = eyes_in_water(world, self.position, input.sneak);
    }
    /// Shared EntityLivingBase.moveEntityWithHeading. The integrated server
    /// and client player provide their own movement speed and gravity context.
    pub(crate) fn travel(&mut self, world: &impl MovementWorld, input: Travel) {
        let flight_y = self.velocity.y;
        let old_y = self.position.y;
        if (input.water || input.lava) && !self.flying {
            let mut water_drag = 0.8f32;
            let mut acceleration = 0.02f32;
            if input.water {
                let mut depth = input.depth_strider.min(3) as f32;
                if !self.grounded {
                    depth *= 0.5;
                }
                if depth > 0. {
                    water_drag += (0.54600006 - water_drag) * depth / 3.;
                    acceleration += (input.land_speed * 1. - acceleration) * depth / 3.;
                }
            }
            self.move_flying(input.strafe, input.forward, acceleration, input.yaw);
            self.move_entity(world, self.velocity, input.sneak);
            self.velocity.x *= if input.water { water_drag as f64 } else { 0.5 };
            self.velocity.y *= if input.water { 0.8f32 as f64 } else { 0.5 };
            self.velocity.z *= if input.water { water_drag as f64 } else { 0.5 };
            self.velocity.y -= 0.02;
            if self.collided_horizontal {
                let moved = self.bounds.offset(DVec3::new(
                    self.velocity.x,
                    self.velocity.y + 0.6f32 as f64 - self.position.y + old_y,
                    self.velocity.z,
                ));
                if collision::colliders(world, moved).is_empty() && !any_liquid(world, moved) {
                    self.velocity.y = 0.3f32 as f64;
                }
            }
        } else {
            let mut friction = 0.91f32;
            if self.grounded {
                friction = slipperiness(world.get(IVec3::new(
                    self.position.x.floor() as i32,
                    self.bounds.min.y.floor() as i32 - 1,
                    self.position.z.floor() as i32,
                ))) * 0.91;
            }
            let factor = 0.16277136f32 / (friction * friction * friction);
            let acceleration = if self.grounded {
                input.land_speed * factor
            } else {
                input.air_speed
            };
            self.move_flying(input.strafe, input.forward, acceleration, input.yaw);
            if input
                .ladder_override
                .unwrap_or_else(|| !self.spectator && ladder(world, self.position, self.bounds))
            {
                self.fall_distance = 0.;
                self.velocity.x = self.velocity.x.clamp(-0.15f32 as f64, 0.15f32 as f64);
                self.velocity.z = self.velocity.z.clamp(-0.15f32 as f64, 0.15f32 as f64);
                if self.velocity.y < -0.15 {
                    self.velocity.y = -0.15;
                }
                if input.sneak && self.velocity.y < 0. {
                    self.velocity.y = 0.;
                }
            }
            self.move_entity(world, self.velocity, input.sneak);
            if self.collided_horizontal
                && input
                    .ladder_override
                    .unwrap_or_else(|| !self.spectator && ladder(world, self.position, self.bounds))
            {
                self.velocity.y = 0.2;
            }
            if input.server || world.loaded_column(self.position.x as i32, self.position.z as i32) {
                self.velocity.y -= 0.08;
            } else {
                self.velocity.y = if self.position.y > 0. { -0.1 } else { 0. };
            }
            self.velocity.y *= 0.98f32 as f64;
            self.velocity.x *= friction as f64;
            self.velocity.z *= friction as f64;
        }
        if self.flying {
            self.velocity.y = flight_y * 0.6;
        }
    }
    pub(crate) fn move_entity(&mut self, world: &impl MovementWorld, mut delta: DVec3, sneak: bool) {
        self.touching_end_portal = false;
        if self.spectator {
            self.bounds = self.bounds.offset(delta);
            self.position = self.bounds.feet();
            self.collided_horizontal = false;
            return;
        }
        if self.in_web {
            self.in_web = false;
            delta.x *= 0.25;
            delta.y *= 0.05f32 as f64;
            delta.z *= 0.25;
            self.velocity = DVec3::ZERO;
        }
        let edge_sneak = self.grounded && sneak;
        let movement = collision::move_body_with_step(world, self.bounds, delta, self.grounded, edge_sneak,
            f64::from(self.step_height.unwrap_or(0.6)));
        self.bounds = movement.bounds;
        self.position = self.bounds.feet();
        self.grounded = movement.grounded;
        self.collided_horizontal = movement.clipped[0] || movement.clipped[2];
        let mut under = IVec3::new(
            self.position.x.floor() as i32,
            (self.position.y - 0.2f32 as f64).floor() as i32,
            self.position.z.floor() as i32,
        );
        let mut block = world.get(under);
        if block >> 4 == 0 {
            let below = world.get(under - IVec3::Y);
            if matches!(below >> 4, 85 | 107 | 113 | 139 | 183..=192) {
                block = below;
                under -= IVec3::Y;
            }
        }
        // LivingBase.updateFallState checks newly entered water after moving,
        // before clipped velocity and block landing/contact callbacks. Existing
        // water contact remains latched until the next Entity base tick.
        if !self.in_water {
            let (water, current) = water_contact(world, self.bounds);
            self.in_water = water;
            if water {
                self.fall_distance = 0.;
            }
            if let Some(current) = current.filter(|_| !self.flying) {
                self.velocity += current * 0.014;
            }
        }
        // Entity.updateFallState calls the block with the accumulated airborne
        // distance. Its landing tick never adds the clipped downward remainder.
        if self.grounded {
            if self.fall_distance > 0. {
                self.landing = Some(Landing {
                    distance: self.fall_distance,
                    state: block,
                    position: under,
                });
            }
            self.fall_distance = 0.;
        } else if movement.delta.y < 0. {
            self.fall_distance = (f64::from(self.fall_distance) - movement.delta.y) as f32;
        }
        if movement.clipped[0] {
            self.velocity.x = 0.;
        }
        if movement.clipped[2] {
            self.velocity.z = 0.;
        }
        if movement.clipped[1] {
            if block >> 4 == 165 && !sneak {
                if self.velocity.y < 0. {
                    self.velocity.y = -self.velocity.y;
                }
            } else {
                self.velocity.y = 0.;
            }
        }
        // The three-argument walking callback is distinct from the
        // four-argument inside-block callback used by soul sand/webs.
        if !self.flying
            && !edge_sneak
            && self.grounded
            && block >> 4 == 165
            && self.velocity.y.abs() < 0.1
            && !sneak
        {
            let factor = 0.4 + self.velocity.y.abs() * 0.2;
            self.velocity.x *= factor;
            self.velocity.z *= factor;
        }
        let inner_min_y = self.bounds.min.y + 0.001;
        let min = (self.bounds.min + DVec3::splat(0.001)).floor().as_ivec3();
        let max = (self.bounds.max - DVec3::splat(0.001)).floor().as_ivec3();
        if !area_loaded(world, min, max) {
            return;
        }
        for x in min.x..=max.x {
            for y in min.y..=max.y {
                for z in min.z..=max.z {
                    let p = IVec3::new(x, y, z);
                    let state = world.get(p);
                    if state >> 4 == 119 && inner_min_y < f64::from(y) + 0.75 {
                        self.touching_end_portal = true;
                    }
                    if state >> 4 == 90 && self.first_portal_block.is_none() {
                        self.first_portal_block = Some(p);
                    }
                    self.block_contact(state);
                }
            }
        }
    }
    fn block_contact(&mut self, state: u16) {
        match state >> 4 {
            10 | 11 | 51 => self.touching_fire = true,
            81 => self.touching_cactus = true,
            90 => self.touching_portal = true,
            30 if !self.flying => {
                self.in_web = true;
                self.fall_distance = 0.;
            }
            88 => {
                self.velocity.x *= 0.4;
                self.velocity.z *= 0.4;
            }
            _ => {}
        }
    }
}
fn slipperiness(state: u16) -> f32 {
    match state >> 4 {
        79 | 174 => 0.98,
        165 => 0.8,
        _ => 0.6,
    }
}
fn ladder(world: &impl MovementWorld, p: DVec3, bounds: Aabb) -> bool {
    matches!(
        world.get(IVec3::new(
            p.x.floor() as i32,
            bounds.min.y.floor() as i32,
            p.z.floor() as i32
        )) >> 4,
        65 | 106
    )
}
fn normalized(v: DVec3) -> DVec3 {
    // Vec3.normalize delegates to MathHelper.sqrt_double, whose return type
    // is float even though the vector components and division are doubles.
    let length = ((v.x * v.x + v.y * v.y + v.z * v.z).sqrt() as f32) as f64;
    if length < 1.0e-4 {
        DVec3::ZERO
    } else {
        let r = v / length;
        DVec3::from_array(r.to_array().map(|c| if c == 0. { 0. } else { c }))
    }
}
pub(crate) fn flow(world: &impl MovementWorld, p: IVec3) -> DVec3 {
    let fluid = world.get(p);
    let decay = |q| {
        let state = world.get(q);
        if crate::block_ticks::same_liquid(fluid, state) {
            if state & 15 >= 8 {
                0
            } else {
                (state & 15) as i32
            }
        } else {
            -1
        }
    };
    let current = decay(p);
    let mut result = DVec3::ZERO;
    for d in [-IVec3::Z, IVec3::X, IVec3::Z, -IVec3::X] {
        let q = p + d;
        let mut next = decay(q);
        let difference = if next < 0 {
            if material::blocks_movement(world.get(q)) {
                continue;
            }
            next = decay(q - IVec3::Y);
            if next < 0 {
                continue;
            }
            next - (current - 8)
        } else {
            next - current
        };
        result += d.as_dvec3() * difference as f64;
    }
    if fluid & 15 >= 8 {
        let solid = |q| {
            let s = world.get(q);
            !crate::block_ticks::same_liquid(fluid, s) && s >> 4 != 79 && material::is_solid(s)
        };
        for d in [-IVec3::Z, IVec3::X, IVec3::Z, -IVec3::X] {
            if solid(p + d) || solid(p + d + IVec3::Y) {
                result = normalized(result) + DVec3::new(0., -6., 0.);
                break;
            }
        }
    }
    normalized(result)
}
pub(crate) fn water_contact(world: &impl MovementWorld, bounds: Aabb) -> (bool, Option<DVec3>) {
    let bounds = bounds
        .expand(DVec3::new(0., -0.4f32 as f64, 0.))
        .expand(DVec3::splat(-0.001));
    water_acceleration(world, bounds)
}
/// Original World.handleMaterialAcceleration water query on the supplied
/// bounds. Each entity type applies its own expansion/contraction first.
pub(crate) fn water_acceleration(
    world: &impl MovementWorld,
    bounds: Aabb,
) -> (bool, Option<DVec3>) {
    let min = bounds.min.floor().as_ivec3();
    let max = (bounds.max + DVec3::ONE).floor().as_ivec3();
    // World.handleMaterialAcceleration first requires its inclusive area
    // bounds to be loaded, including the next chunk at an exact edge.
    if !area_loaded(world, min, max) {
        return (false, None);
    }
    let mut contact = false;
    let mut current = DVec3::ZERO;
    for x in min.x..max.x {
        for y in min.y..max.y {
            for z in min.z..max.z {
                let p = IVec3::new(x, y, z);
                let s = world.get(p);
                if matches!(s >> 4, 8 | 9) {
                    let level = if s & 15 >= 8 { 0 } else { s & 15 };
                    let surface = (y + 1) as f32 - (level + 1) as f32 / 9.;
                    if max.y as f64 >= surface as f64 {
                        contact = true;
                        current += flow(world, p);
                    }
                }
            }
        }
    }
    let length =
        (current.x * current.x + current.y * current.y + current.z * current.z).sqrt() as f32;
    (contact, (length > 0.).then(|| normalized(current)))
}
fn area_loaded(world: &impl MovementWorld, min: IVec3, max: IVec3) -> bool {
    if max.y < 0 || min.y >= 256 {
        return false;
    }
    for cx in min.x.div_euclid(16)..=max.x.div_euclid(16) {
        for cz in min.z.div_euclid(16)..=max.z.div_euclid(16) {
            if !world.loaded_column(cx * 16, cz * 16) {
                return false;
            }
        }
    }
    true
}
pub(crate) fn touches_liquid(
    world: &impl MovementWorld,
    bounds: Aabb,
    first: u16,
    last: u16,
) -> bool {
    let min = bounds.min.floor().as_ivec3();
    let max = (bounds.max + DVec3::ONE).floor().as_ivec3();
    for x in min.x..max.x {
        for y in min.y..max.y {
            for z in min.z..max.z {
                if (first..=last).contains(&(world.get(IVec3::new(x, y, z)) >> 4)) {
                    return true;
                }
            }
        }
    }
    false
}
// World.isAnyLiquid uses an inclusive floor(max), unlike isMaterialInBB's
// exclusive floor(max + 1). Preserve that distinction at floating boundaries.
pub(crate) fn any_liquid(world: &impl MovementWorld, bounds: Aabb) -> bool {
    let min = bounds.min.floor().as_ivec3();
    let max = bounds.max.floor().as_ivec3();
    for x in min.x..=max.x {
        for y in min.y..=max.y {
            for z in min.z..=max.z {
                if matches!(world.get(IVec3::new(x, y, z)) >> 4, 8..=11) {
                    return true;
                }
            }
        }
    }
    false
}
pub(crate) fn eyes_in_water(world: &impl MovementWorld, position: DVec3, sneak: bool) -> bool {
    let height = if sneak { 1.62f32 - 0.08f32 } else { 1.62f32 };
    let eye = position.y + height as f64;
    let p = IVec3::new(
        position.x.floor() as i32,
        eye.floor() as i32,
        position.z.floor() as i32,
    );
    let state = world.get(p);
    if !matches!(state >> 4, 8 | 9) {
        return false;
    }
    let level = if state & 15 >= 8 { 0 } else { state & 15 };
    let percent = (level + 1) as f32 / 9. - 0.11111111f32;
    eye < ((p.y + 1) as f32 - percent) as f64
}
