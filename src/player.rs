//! Fixed 20 Hz movement, derived from the locally installed 1.8.9 client.
//! Authoritative motion uses the original f64 core; Vec3 fields are render/UI
//! mirrors. Unsupported potion/entity effects and block-shape gaps are explicit
//! in vanilla::movement and the shape adapter below.
#[cfg(test)]
use crate::assets;
use crate::{
    vanilla::{
        math,
        movement::{self, MovementWorld},
    },
    world::World,
};
use glam::{DVec3, IVec3, Vec3};
use std::collections::HashSet;
use winit::keyboard::KeyCode;

#[path="third_person_pose.rs"]
pub(crate) mod third_person;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Player {
    #[serde(skip)]
    pub(crate) third_person:third_person::Animation,
    pub position: Vec3,
    pub previous: Vec3,
    pub velocity: Vec3,
    pub yaw: f32,
    pub pitch: f32,
    pub grounded: bool,
    pub flying: bool,
    pub allow_flying: bool,
    pub fly_speed: f32,
    /// Capabilities.walkSpeed is separate from generic.movementSpeed, as in
    /// the original client. A server ability packet does not set attributes.
    pub walk_speed: f32,
    pub speed_boost: Option<i32>,
    pub slowness: Option<i32>,
    pub jump_boost: Option<i32>,
    pub blinded: bool,
    pub depth_strider: i32,
    /// Actual sprint state, rather than whether the sprint key is held.
    pub sprinting: bool,
    /// Set by survival before tick: food > 6, or creative flight capability.
    pub can_sprint: bool,
    /// Set by the item-use controller while eating, blocking, or drawing a bow.
    pub using_item: bool,
    /// One-tick ground-jump event; swimming and flying do not consume jump food.
    pub jumped: bool,
    pub collided_horizontal: bool,
    pub in_water: bool,
    pub selected: usize,
    /// Physical keys are sampled anew after reconnecting a native client.
    #[serde(skip)]
    pub keys: HashSet<KeyCode>,
    pub eyes_in_water: bool,
    #[serde(default)]
    pub sleeping: bool,
    #[serde(skip)]
    pub riding: bool,
    #[serde(skip)]
    pub suppress_view_input: bool,
    pub on_ladder: bool,
    pub jump_sprinting: bool,
    #[serde(skip)]
    pub in_portal: bool,
    #[serde(skip)]
    pub portal_cooldown: u16,
    #[serde(skip)]
    pub portal_counter: u16,
    #[serde(skip)]
    pub time_in_portal: f32,
    #[serde(skip)]
    pub prev_time_in_portal: f32,
    #[serde(skip)]
    pub close_portal_gui: bool,
    #[serde(skip, default = "default_last_portal_pos")]
    pub last_portal_pos: IVec3,
    #[serde(skip)]
    pub last_portal_vec: DVec3,
    #[serde(skip, default = "default_teleport_direction")]
    pub teleport_direction: crate::dimension_world::HorizFacing,
    motion: movement::State,
    yaw_degrees: f32,
    pitch_degrees: f32,
    pick_head_yaw: Option<f32>,
    #[serde(skip)]
    previous_pick_head_yaw: Option<f32>,
    view_yaw: f32,
    view_pitch: f32,
    previous_speed_boost: Option<i32>,
    previous_slowness: Option<i32>,
}
impl Player {
    #[cfg(test)]
    pub fn edit(&self, world: &mut World, place: bool) -> Option<IVec3> {
        let (hit, adjacent) = world.raycast(self.eye(1.0), self.direction(), 5.0)?;
        let position = if place { adjacent } else { hit };
        if place && self.overlaps(position) {
            return None;
        }
        world
            .set(
                position,
                if place {
                    assets::HOTBAR[self.selected]
                } else {
                    0
                },
            )
            .then_some(position)
    }
    pub fn new(position: Vec3) -> Self {
        Self {
            third_person:Default::default(),
            position,
            previous: position,
            velocity: Vec3::ZERO,
            yaw: -0.6,
            pitch: -0.15,
            grounded: false,
            flying: false,
            allow_flying: false,
            fly_speed: 0.05,
            walk_speed: 0.1,
            speed_boost: None,
            slowness: None,
            jump_boost: None,
            blinded: false,
            depth_strider: 0,
            sprinting: false,
            can_sprint: true,
            using_item: false,
            jumped: false,
            collided_horizontal: false,
            in_water: false,
            selected: 0,
            keys: HashSet::new(),
            eyes_in_water: false,
            sleeping: false,
            riding: false,
            suppress_view_input: false,
            on_ladder: false,
            jump_sprinting: false,
            in_portal: false,
            portal_cooldown: 0,
            portal_counter: 0,
            time_in_portal: 0.0,
            prev_time_in_portal: 0.0,
            close_portal_gui: false,
            last_portal_pos: default_last_portal_pos(),
            last_portal_vec: DVec3::ZERO,
            teleport_direction: default_teleport_direction(),
            motion: movement::State::new(position.as_dvec3()),
            yaw_degrees: (-0.6f32).to_degrees() - 180.,
            pitch_degrees: 0.15f32.to_degrees(),
            pick_head_yaw: None,
            previous_pick_head_yaw: None,
            view_yaw: -0.6,
            view_pitch: -0.15,
            previous_speed_boost: None,
            previous_slowness: None,
        }
    }
    fn sync_rotation(&mut self) {
        if self.yaw != self.view_yaw {
            self.yaw_degrees = self.yaw.to_degrees() - 180.;
            self.view_yaw = self.yaw;
        }
        if self.pitch != self.view_pitch {
            self.pitch_degrees = -self.pitch.to_degrees();
            self.view_pitch = self.pitch;
        }
    }
    pub fn java_rotation(&self) -> [f32; 2] {
        [
            if self.yaw != self.view_yaw {
                self.yaw.to_degrees() - 180.
            } else {
                self.yaw_degrees
            },
            if self.pitch != self.view_pitch {
                -self.pitch.to_degrees()
            } else {
                self.pitch_degrees
            },
        ]
    }
    pub fn set_rotation_degrees(&mut self, yaw: f32, pitch: f32) {
        self.yaw_degrees = yaw;
        self.pitch_degrees = pitch;
        self.yaw = (yaw + 180.).to_radians();
        self.pitch = -pitch.to_radians();
        self.view_yaw = self.yaw;
        self.view_pitch = self.pitch;
        self.pick_head_yaw = Some(yaw);
        self.previous_pick_head_yaw = Some(yaw);
    }
    pub fn look(&mut self, dx: f64, dy: f64) {
        self.sync_rotation();
        let head = self.pick_head_yaw.unwrap_or(self.yaw_degrees);
        let previous_head = self.previous_pick_head_yaw;
        let yaw = (self.yaw_degrees as f64 + dx as f32 as f64 * 0.15) as f32;
        let pitch = ((self.pitch_degrees as f64 + dy as f32 as f64 * 0.15) as f32).clamp(-90., 90.);
        self.set_rotation_degrees(yaw, pitch);
        self.pick_head_yaw = Some(head);
        self.previous_pick_head_yaw = previous_head;
    }
    pub fn direction(&self) -> Vec3 {
        let [yaw, pitch] = self.java_rotation();
        Self::direction_at(yaw, pitch)
    }
    fn direction_at(yaw: f32, pitch: f32) -> Vec3 {
        let factor = std::f32::consts::PI / 180.;
        let c = math::cos(-yaw * factor - std::f32::consts::PI);
        let s = math::sin(-yaw * factor - std::f32::consts::PI);
        let flat = -math::cos(-pitch * factor);
        Vec3::from_array([s * flat, math::sin(-pitch * factor), c * flat].map(|v| {
            if v == 0. {
                0.
            } else {
                v
            }
        }))
    }
    pub fn precise_position(&self) -> [f64; 3] {
        if self.position != self.motion.position.as_vec3() {
            self.position.as_dvec3().to_array()
        } else {
            self.motion.position.to_array()
        }
    }
    pub fn precise_velocity(&self) -> [f64; 3] {
        if self.velocity != self.motion.velocity.as_vec3() {
            self.velocity.as_dvec3().to_array()
        } else {
            self.motion.velocity.to_array()
        }
    }
    pub fn movement_delta(&self) -> [f64; 3] {
        (self.motion.position - self.motion.previous).to_array()
    }
    pub(crate) fn tick_local(&mut self, world: &World, fall_distance: &mut f32) -> Option<movement::Landing> {
        self.motion.fall_distance = *fall_distance;
        self.tick(world);
        *fall_distance = self.motion.fall_distance;
        self.motion.landing
    }
    pub(crate) fn in_lava(&self) -> bool {
        self.motion.in_lava
    }
    pub(crate) fn touching_fire(&self) -> bool {
        self.motion.touching_fire
    }
    pub(crate) fn touching_cactus(&self) -> bool {
        self.motion.touching_cactus
    }
    pub fn touching_portal(&self) -> bool {
        self.motion.touching_portal
    }
    pub fn touching_end_portal(&self) -> bool {
        self.motion.touching_end_portal
    }
    pub fn step_portal_client(&mut self, confusion_gt_60: bool, rand_float: f32) -> Option<f32> {
        self.prev_time_in_portal = self.time_in_portal;
        let mut sound = None;
        if self.in_portal {
            self.close_portal_gui = true;
            if self.time_in_portal == 0.0 {
                sound = Some(rand_float * 0.4 + 0.8);
            }
            self.time_in_portal = (self.time_in_portal + 0.0125).min(1.0);
            self.in_portal = false;
        } else if confusion_gt_60 {
            self.time_in_portal = (self.time_in_portal + 1.0 / 150.0).min(1.0);
        } else {
            self.time_in_portal = (self.time_in_portal - 0.05).max(0.0);
        }
        self.portal_cooldown = self.portal_cooldown.saturating_sub(1);
        sound
    }
    pub fn apply_portal_contact(&mut self) {
        if !self.riding && self.motion.touching_portal {
            if self.portal_cooldown > 0 {
                self.portal_cooldown = 10;
            } else {
                self.in_portal = true;
            }
        }
    }
    pub fn apply_portal_contact_with_world(&mut self, world: &World) {
        if !self.riding && self.motion.touching_portal {
            if self.portal_cooldown > 0 {
                self.portal_cooldown = 10;
            } else {
                if let Some(pos) = self.motion.first_portal_block {
                    if pos != self.last_portal_pos {
                        self.last_portal_pos = pos;
                        let helper = portal_pattern_helper(world, pos);
                        let p = DVec3::from_array(self.precise_position());
                        let finger_axis_x = matches!(
                            helper.finger,
                            crate::dimension_world::HorizFacing::West
                                | crate::dimension_world::HorizFacing::East
                        );
                        let d0 = if finger_axis_x {
                            f64::from(helper.origin.z)
                        } else {
                            f64::from(helper.origin.x)
                        };
                        let p_coord = if finger_axis_x { p.z } else { p.x };
                        let rot_y_neg = matches!(
                            helper.finger,
                            crate::dimension_world::HorizFacing::South
                                | crate::dimension_world::HorizFacing::West
                        );
                        let d1 = ((p_coord - if rot_y_neg { 1.0 } else { 0.0 } - d0)
                            / -f64::from(helper.width))
                        .abs();
                        let d2 = ((p.y - 1.0) - f64::from(helper.origin.y))
                            / -f64::from(helper.height);
                        self.last_portal_vec = DVec3::new(d1, d2, 0.0);
                        self.teleport_direction = helper.finger;
                    }
                }
                self.in_portal = true;
            }
        }
    }
    pub fn fov_modifier(&self, bow_use_ticks: Option<u32>) -> f32 {
        let mut f = 1.0_f32;
        if self.flying {
            f *= 1.1;
        }
        let mut attr_speed = f64::from(self.walk_speed);
        if self.sprinting {
            attr_speed *= 1.0 + 0.30000001192092896;
        }
        if let Some(amp) = self.speed_boost {
            attr_speed *= 1.0 + 0.20000000298023224 * f64::from(amp.wrapping_add(1));
        }
        if let Some(amp) = self.slowness {
            attr_speed = (attr_speed
                * (1.0 - 0.15000000596046448 * f64::from(amp.wrapping_add(1))))
            .max(0.0);
        }
        if self.walk_speed > 0.0 {
            f = (f64::from(f) * ((attr_speed / f64::from(self.walk_speed) + 1.0) / 2.0)) as f32;
        }
        if self.walk_speed == 0.0 || !f.is_finite() {
            f = 1.0;
        }
        if let Some(ticks) = bow_use_ticks {
            let mut f1 = ticks as f32 / 20.0;
            if f1 > 1.0 {
                f1 = 1.0;
            } else {
                f1 *= f1;
            }
            f *= 1.0 - f1 * 0.15;
        }
        f
    }
    pub fn precise_bounds(&self) -> movement::Aabb {
        if self.sleeping {
            let p=DVec3::from_array(self.precise_position());
            let half=f64::from(0.2f32/2.);
            return movement::Aabb { min:p-DVec3::new(half,0.,half), max:p+DVec3::new(half,f64::from(0.2f32),half) };
        }
        if self.position != self.motion.position.as_vec3() {
            movement::Aabb::player(self.position.as_dvec3())
        } else {
            self.motion.bounds
        }
    }
    pub fn flight_changes(&self) -> &[bool] {
        &self.motion.flight_changes
    }
    pub(crate) fn set_spectator(&mut self, spectator: bool) {
        self.motion.spectator = spectator;
        if spectator { self.grounded = false; }
    }
    pub fn clear_sprint(&mut self) {
        self.sprinting = false;
        self.motion.set_sprinting(false);
    }
    pub fn set_watched_sprinting(&mut self, sprinting: bool) {
        self.sprinting = sprinting;
        self.motion.sprinting = sprinting;
    }
    pub fn reset_respawn(&mut self) {
        self.set_velocity_f64([0.; 3]);
        self.sprinting = false;
        self.motion.set_sprinting(false);
        self.motion.movement_attribute = Default::default();
        self.previous_speed_boost = None;
        self.previous_slowness = None;
        self.speed_boost = None;
        self.slowness = None;
        self.jump_boost = None;
        self.blinded = false;
        self.depth_strider = 0;
        self.using_item = false;
        self.jumped = false;
        self.jump_sprinting = false;
        self.collided_horizontal = false;
        self.in_water = false;
        self.eyes_in_water = false;
        self.sleeping = false;
        self.riding = false;
        self.third_person.riding = false;
        self.third_person.mount_yaw = None;
        self.on_ladder = false;
        self.in_portal = false;
        self.portal_cooldown = 0;
        self.portal_counter = 0;
        self.time_in_portal = 0.0;
        self.prev_time_in_portal = 0.0;
        self.close_portal_gui = false;
        self.last_portal_pos = default_last_portal_pos();
        self.last_portal_vec = DVec3::ZERO;
        self.teleport_direction = default_teleport_direction();
        self.motion.touching_portal = false;
        self.motion.first_portal_block = None;
        self.motion.touching_end_portal = false;
        self.motion.fall_distance = 0.0;
        self.motion.in_web = false;
        self.motion.on_ladder = false;
        self.motion.jump_delay = 0;
        self.motion.sprint_toggle_ticks = 0;
        self.motion.fly_toggle_ticks = 0;
        self.motion.air_speed = 0.02;
        self.motion.flight_changes.clear();
    }
    pub fn reset_portal_travel(&mut self) {
        self.set_velocity_f64([0.; 3]);
        self.using_item = false;
        self.jumped = false;
        self.jump_sprinting = false;
        self.collided_horizontal = false;
        self.in_water = false;
        self.eyes_in_water = false;
        self.sleeping = false;
        self.riding = false;
        self.third_person.riding = false;
        self.third_person.mount_yaw = None;
        self.on_ladder = false;
        self.in_portal = false;
        self.portal_cooldown = 10;
        self.portal_counter = 0;
        self.time_in_portal = 0.0;
        self.prev_time_in_portal = 0.0;
        self.close_portal_gui = false;
        self.last_portal_pos = default_last_portal_pos();
        self.motion.touching_portal = false;
        self.motion.first_portal_block = None;
        self.motion.touching_end_portal = false;
        self.motion.fall_distance = 0.0;
        self.motion.in_web = false;
        self.motion.on_ladder = false;
        self.motion.jump_delay = 0;
    }
    pub(crate) fn is_spectator(&self) -> bool {
        self.motion.spectator
    }
    pub fn apply_movement_attribute(
        &mut self,
        snapshot: &movement::attribute::Snapshot,
    ) -> anyhow::Result<()> {
        self.motion.movement_attribute.replace(snapshot)?;
        self.motion
            .movement_attribute
            .set_modifier(movement::attribute::SPRINT, self.sprinting.then_some(0.3f32 as f64));
        Ok(())
    }
    pub fn begin_mounted_tick(&mut self) {
        self.motion.previous = self.motion.position;
        self.previous = self.position;
    }
    pub fn set_position_f64(&mut self, position: [f64; 3]) {
        let p = DVec3::from_array(position);
        self.motion.set_position(p);
        self.position = p.as_vec3();
        if !self.riding {
            self.motion.previous = p;
            self.previous = self.position;
        }
    }
    pub fn set_velocity_f64(&mut self, velocity: [f64; 3]) {
        self.motion.velocity = DVec3::from_array(velocity);
        self.velocity = self.motion.velocity.as_vec3();
    }
    pub fn set_pose_f64(&mut self, position: [f64; 3], yaw: f32, pitch: f32, grounded: bool) {
        self.set_position_f64(position);
        let p = DVec3::from_array(position);
        self.motion.previous = p;
        self.previous = self.position;
        self.set_rotation_degrees(yaw, pitch);
        self.grounded = grounded;
        self.motion.grounded = grounded;
    }
    pub fn eye(&self, alpha: f32) -> Vec3 {
        self.previous.lerp(self.position, alpha)
            + Vec3::Y * self.eye_height()
    }
    pub(crate) fn eye_height(&self)->f32 {
        let standing=if self.sleeping {0.2} else {1.62};
        if self.sneaking() && !self.riding {standing-0.08} else {standing}
    }
    pub fn precise_eye(&self, alpha: f32) -> DVec3 {
        let p = DVec3::from_array(self.precise_position());
        let previous = if self.previous != self.motion.previous.as_vec3() {
            self.previous.as_dvec3()
        } else {
            self.motion.previous
        };
        let feet = if alpha == 1. {
            p
        } else {
            previous + (p - previous) * alpha as f64
        };
        let height = self.eye_height();
        feet + DVec3::Y * height as f64
    }
    pub fn block_ray(
        &self,
        world: &World,
        reach: f64,
        options: crate::vanilla::ray_trace::Options,
    ) -> Option<crate::vanilla::ray_trace::Hit> {
        let eye = self.precise_eye(1.);
        crate::vanilla::ray_trace::trace(
            world,
            eye,
            eye + self.direction().as_dvec3() * reach,
            options,
        )
    }
    pub fn pick_view(&self, creative: bool, partial: f32) -> crate::vanilla::entity_pick::View {
        let [yaw, pitch] = self.java_rotation();
        // EntityLivingBase.getLook uses head yaw. Mouse movement changes camera
        // yaw immediately, while EntityPlayer.updateEntityActionState copies it
        // to the head during the next living update.
        let head = if self.yaw == self.view_yaw {
            let current = self.pick_head_yaw.unwrap_or(yaw);
            if partial == 1. {
                current
            } else {
                let previous = self.previous_pick_head_yaw.unwrap_or(current);
                previous + (current - previous) * partial
            }
        } else {
            yaw
        };
        crate::vanilla::entity_pick::View {
            bounds: self.precise_bounds(),
            eye: self.precise_eye(partial),
            look: Self::direction_at(head, pitch).as_dvec3(),
            creative,
        }
    }
    #[cfg(test)]
    pub fn overlaps(&self, p: IVec3) -> bool {
        self.precise_bounds().intersects(movement::Aabb {
            min: p.as_dvec3(),
            max: p.as_dvec3() + DVec3::ONE,
        })
    }
    fn pressed(&self, k: KeyCode) -> f32 {
        if self.keys.contains(&k) {
            1.0
        } else {
            0.0
        }
    }
    pub fn sneaking(&self) -> bool {
        self.keys.contains(&KeyCode::ShiftLeft) || self.keys.contains(&KeyCode::ShiftRight)
    }
    pub fn tick(&mut self, world: &World) {
        self.tick_with(world);
    }
    pub fn tick_with(&mut self, world: &impl MovementWorld) {
        self.sync_rotation();
        if self.position != self.motion.position.as_vec3() {
            self.motion.set_position(self.position.as_dvec3());
        }
        self.motion.velocity = DVec3::from_array(self.precise_velocity());
        self.motion.grounded = self.grounded;
        self.motion.flying = self.flying;
        if self.motion.sprinting != self.sprinting {
            self.motion.set_sprinting(self.sprinting);
        }
        if self.speed_boost != self.previous_speed_boost {
            self.motion.movement_attribute.set_modifier(
                movement::attribute::SPEED,
                self.speed_boost
                    .map(|a| 0.2f32 as f64 * a.wrapping_add(1) as f64),
            );
            self.previous_speed_boost = self.speed_boost;
        }
        if self.slowness != self.previous_slowness {
            self.motion.movement_attribute.set_modifier(
                movement::attribute::SLOWNESS,
                self.slowness
                    .map(|a| -0.15f32 as f64 * a.wrapping_add(1) as f64),
            );
            self.previous_slowness = self.slowness;
        }
        let input = movement::Input {
            forward: self.pressed(KeyCode::KeyW) - self.pressed(KeyCode::KeyS),
            strafe: self.pressed(KeyCode::KeyA) - self.pressed(KeyCode::KeyD),
            jump: self.keys.contains(&KeyCode::Space),
            sneak: self.sneaking(),
            sprint_key: self.keys.contains(&KeyCode::ControlLeft)
                || self.keys.contains(&KeyCode::ControlRight),
            can_sprint: self.can_sprint,
            using_item: self.using_item,
            yaw: self.yaw_degrees,
            allow_flying: self.allow_flying,
            fly_speed: self.fly_speed,
            jump_boost: self.jump_boost,
            blinded: self.blinded,
            depth_strider: self.depth_strider,
            suppress_view_input: self.suppress_view_input,
        };
        self.motion.tick(world, input);
        self.riding=false;
        self.third_person.riding=false;self.third_person.mount_yaw=None;
        self.third_person.tick(self.motion.position-self.motion.previous,self.yaw_degrees);
        let last_head = self.pick_head_yaw.unwrap_or(self.yaw_degrees);
        let turn = crate::player::third_person::wrap(self.yaw_degrees - last_head);
        self.previous_pick_head_yaw = Some(self.yaw_degrees - turn);
        self.pick_head_yaw = Some(self.yaw_degrees);
        self.position = self.motion.position.as_vec3();
        self.previous = self.motion.previous.as_vec3();
        self.velocity = self.motion.velocity.as_vec3();
        self.grounded = self.motion.grounded;
        self.flying = self.motion.flying;
        self.sprinting = self.motion.sprinting;
        self.in_water = self.motion.in_water;
        self.jumped = self.motion.jumped;
        self.collided_horizontal = self.motion.collided_horizontal;
        self.eyes_in_water = self.motion.eyes_in_water;
        self.on_ladder = self.motion.on_ladder;
        self.jump_sprinting = self.motion.jump_sprinting;
    }
    pub(crate) fn move_by_piston(&mut self, world: &impl MovementWorld, delta: DVec3) {
        if self.position != self.motion.position.as_vec3() {
            self.motion.set_position(self.position.as_dvec3());
        }
        self.motion.velocity = DVec3::from_array(self.precise_velocity());
        self.motion.grounded = self.grounded;
        self.motion.flying = self.flying;
        self.motion
            .move_entity(world, delta, self.sneaking() && !self.sleeping);
        self.position = self.motion.position.as_vec3();
        self.velocity = self.motion.velocity.as_vec3();
        self.grounded = self.motion.grounded;
        self.collided_horizontal = self.motion.collided_horizontal;
        self.in_water = self.motion.in_water;
    }
}
impl MovementWorld for World {
    fn get(&self, p: IVec3) -> u16 {
        World::get(self, p)
    }
    fn block_boxes(&self, p: IVec3, state: u16, out: &mut Vec<movement::Aabb>) {
        block_boxes64(self, p, state, |b| out.push(b));
    }
    fn loaded_column(&self, x: i32, z: i32) -> bool {
        self.chunks
            .contains_key(&(x.div_euclid(16), z.div_euclid(16)))
    }
}
#[cfg(test)]
#[path = "vanilla/movement_tests.rs"]
pub(crate) mod movement_tests;
/// Original block-state geometry shared by player, falling-entity and mesh callers.
fn block_boxes64(world: &World, p: IVec3, state: u16, emit: impl FnMut(movement::Aabb)) {
    let tile = if state >> 4 == 36 {
        world.piston_at(p)
    } else {
        None
    };
    crate::vanilla::collision_shapes::emit(p, state, |position| world.get(position), tile, emit);
}
/// Geometry shared with the mesher for shapes whose visual and collision
/// bounds agree. Callers must account for farmland, snow, fences, and similar
/// blocks whose visual model intentionally differs from its collision box.
pub(crate) fn block_collision_boxes(
    world: &World,
    p: IVec3,
    state: u16,
    mut emit: impl FnMut(Vec3, Vec3),
) {
    block_boxes64(world, p, state, |b| emit(b.min.as_vec3(), b.max.as_vec3()));
}

fn default_last_portal_pos() -> IVec3 {
    IVec3::splat(i32::MIN)
}

fn default_teleport_direction() -> crate::dimension_world::HorizFacing {
    crate::dimension_world::HorizFacing::North
}

struct PortalPatternHelper {
    origin: IVec3,
    finger: crate::dimension_world::HorizFacing,
    width: i32,
    height: i32,
}

struct PortalFrameSize {
    bottom_left: IVec3,
    right_dir: IVec3,
    width: i32,
    height: i32,
}

fn is_portal_empty_block(state: u16) -> bool {
    let id = state >> 4;
    id == 0 || id == 51 || id == 90
}

fn portal_distance_until_obsidian(world: &World, origin: IVec3, dir: IVec3) -> i32 {
    let mut n = 0;
    while n < 22 {
        let p = origin + dir * n;
        if !is_portal_empty_block(world.get(p)) || (world.get(p - IVec3::Y) >> 4) != 49 {
            break;
        }
        n += 1;
    }
    if (world.get(origin + dir * n) >> 4) == 49 {
        n
    } else {
        0
    }
}

fn measure_portal_frame(world: &World, mut pos: IVec3, axis_x: bool) -> Option<PortalFrameSize> {
    let (right_dir, left_dir) = if axis_x {
        (IVec3::new(-1, 0, 0), IVec3::new(1, 0, 0))
    } else {
        (IVec3::new(0, 0, 1), IVec3::new(0, 0, -1))
    };
    let min_y = (pos.y - 21).max(0);
    while pos.y > min_y && is_portal_empty_block(world.get(pos - IVec3::Y)) {
        pos.y -= 1;
    }
    let i = portal_distance_until_obsidian(world, pos, left_dir) - 1;
    if i < 0 {
        return None;
    }
    let bottom_left = pos + left_dir * i;
    let width = portal_distance_until_obsidian(world, bottom_left, right_dir);
    if !(2..=21).contains(&width) {
        return None;
    }
    let mut height = 0;
    'rows: while height < 21 {
        for w in 0..width {
            let q = bottom_left + right_dir * w + IVec3::Y * height;
            if !is_portal_empty_block(world.get(q)) {
                break 'rows;
            }
            if w == 0 && (world.get(q + left_dir) >> 4) != 49 {
                break 'rows;
            }
            if w == width - 1 && (world.get(q + right_dir) >> 4) != 49 {
                break 'rows;
            }
        }
        height += 1;
    }
    for w in 0..width {
        if (world.get(bottom_left + right_dir * w + IVec3::Y * height) >> 4) != 49 {
            return None;
        }
    }
    (3..=21).contains(&height).then_some(PortalFrameSize {
        bottom_left,
        right_dir,
        width,
        height,
    })
}

fn portal_pattern_helper(world: &World, pos: IVec3) -> PortalPatternHelper {
    use crate::dimension_world::HorizFacing;
    let (normal_is_z, size) = if let Some(s) = measure_portal_frame(world, pos, true) {
        (true, s)
    } else if let Some(s) = measure_portal_frame(world, pos, false) {
        (false, s)
    } else {
        return PortalPatternHelper {
            origin: pos,
            finger: HorizFacing::North,
            width: 1,
            height: 1,
        };
    };
    let top_left = size.bottom_left + IVec3::Y * (size.height - 1);
    let count_front = |origin: IVec3, finger_vec: IVec3| -> usize {
        let cross = finger_vec.cross(IVec3::Y);
        let mut count = 0;
        for i in 0..size.width {
            for j in 0..size.height {
                let check = origin + IVec3::Y * (-j) + cross * i + finger_vec;
                if (world.get(check) >> 4) != 0 {
                    count += 1;
                }
            }
        }
        count
    };
    let (pos_finger, pos_vec, neg_finger, neg_vec) = if normal_is_z {
        (
            HorizFacing::South,
            IVec3::new(0, 0, 1),
            HorizFacing::North,
            IVec3::new(0, 0, -1),
        )
    } else {
        (
            HorizFacing::East,
            IVec3::new(1, 0, 0),
            HorizFacing::West,
            IVec3::new(-1, 0, 0),
        )
    };
    let pos_origin = top_left;
    let neg_origin = top_left + size.right_dir * (size.width - 1);
    let pos_count = count_front(pos_origin, pos_vec);
    let neg_count = count_front(neg_origin, neg_vec);
    let (origin, finger) = if neg_count < pos_count {
        (neg_origin, neg_finger)
    } else {
        (pos_origin, pos_finger)
    };
    PortalPatternHelper {
        origin,
        finger,
        width: size.width,
        height: size.height,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::Chunk;
    use std::{path::PathBuf, sync::Arc};
    #[derive(Clone, Copy)]
    struct Aabb(movement::Aabb);
    impl Aabb {
        fn player(p: Vec3) -> Self {
            Self(movement::Aabb::player(p.as_dvec3()))
        }
        fn feet(self) -> Vec3 {
            self.0.feet().as_vec3()
        }
    }
    struct Movement {
        bounds: Aabb,
        grounded: bool,
    }
    fn move_body(
        world: &World,
        bounds: Aabb,
        delta: Vec3,
        grounded: bool,
        sneak: bool,
    ) -> Movement {
        let r = movement::collision::move_body(
            world,
            bounds.0,
            delta.as_dvec3(),
            grounded,
            sneak && grounded,
        );
        Movement {
            bounds: Aabb(r.bounds),
            grounded: r.grounded,
        }
    }
    fn touches_lava(world: &World, b: Aabb) -> bool {
        movement::touches_liquid(
            world,
            b.0.expand(DVec3::new(-0.1f32 as f64, -0.4f32 as f64, -0.1f32 as f64)),
            10,
            11,
        )
    }
    fn touches_any_liquid(world: &World, b: Aabb) -> bool {
        movement::touches_liquid(world, b.0, 8, 11)
    }
    fn flat_world() -> World {
        let mut chunk = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                chunk.set(x, 0, z, 1 << 4);
            }
        }
        World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(chunk))]),
            spawn: Vec3::new(8.5, 1.0, 8.5),
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        }
    }
    fn standing(world: &World) -> Player {
        let mut p = Player::new(world.spawn);
        p.yaw = 0.0;
        for _ in 0..3 {
            p.tick(world);
        }
        assert!(p.grounded);
        p
    }
    fn close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.00005,
            "{actual} != {expected}"
        );
    }
    #[test]
    fn legacy_walk_and_sprint_twenty_tick_distance() {
        // EntityLivingBase.moveEntityWithHeading on stone: input .98,
        // acceleration .1, slipperiness .6*.91, then retained momentum.
        let w = flat_world();
        let mut walk = standing(&w);
        let mut sprint = standing(&w);
        walk.keys.insert(KeyCode::KeyW);
        sprint.keys.extend([KeyCode::KeyW, KeyCode::ControlLeft]);
        for expected in [0.098, 0.151508, 0.18072337, 0.19667496, 0.20538452] {
            let before = walk.position.z;
            walk.tick(&w);
            close(before - walk.position.z, expected);
            sprint.tick(&w);
        }
        for _ in 5..20 {
            walk.tick(&w);
            sprint.tick(&w);
        }
        close(w.spawn.z - walk.position.z, 4.057581);
        close(w.spawn.z - sprint.position.z, 5.274855);
        assert!(sprint.sprinting);
    }
    #[test]
    fn sprint_requires_forward_food_and_supports_double_tap() {
        let w = flat_world();
        let mut p = standing(&w);
        p.keys.extend([KeyCode::ControlLeft, KeyCode::KeyD]);
        p.tick(&w);
        assert!(!p.sprinting, "strafe-only must not sprint");
        p.keys.insert(KeyCode::KeyW);
        p.can_sprint = false;
        p.tick(&w);
        assert!(!p.sprinting, "food level at most 6 prevents sprinting");
        p.can_sprint = true;
        p.tick(&w);
        assert!(p.sprinting);
        p.keys.remove(&KeyCode::ControlLeft);
        p.tick(&w);
        assert!(p.sprinting, "sprinting continues after releasing Ctrl");
        p.keys.insert(KeyCode::ShiftLeft);
        p.tick(&w);
        assert!(!p.sprinting);
        let mut p = standing(&w);
        p.keys.insert(KeyCode::KeyW);
        p.tick(&w);
        assert!(!p.sprinting);
        p.keys.clear();
        p.tick(&w);
        p.keys.insert(KeyCode::KeyW);
        p.tick(&w);
        assert!(p.sprinting, "second forward press within 7 ticks sprints");
        p.using_item = true;
        p.tick(&w);
        assert!(!p.sprinting, "item use slows movement and cancels sprint");
    }
    #[test]
    fn jump_trajectory_matches_legacy_gravity_and_vertical_deadband() {
        let w = flat_world();
        let mut p = standing(&w);
        p.keys.insert(KeyCode::Space);
        // The .005 deadband zeroes the tiny upward velocity on tick six.
        let heights = [
            1.42, 1.7532, 2.001336, 2.1661093, 2.249187, 2.249187, 2.170787, 2.015555, 1.7850277,
            1.4807109, 1.1040804, 1.0,
        ];
        for (tick, height) in heights.into_iter().enumerate() {
            p.tick(&w);
            close(p.position.y, height);
            assert_eq!(p.jumped, tick == 0);
        }
        assert!(p.grounded);
    }
    #[test]
    fn sprint_jump_adds_forward_impulse_and_releasing_jump_resets_delay() {
        let mut w = flat_world();
        let mut p = standing(&w);
        p.keys
            .extend([KeyCode::Space, KeyCode::KeyW, KeyCode::ControlLeft]);
        p.tick(&w);
        close(w.spawn.z - p.position.z, 0.3274);
        assert!(p.jumped && p.sprinting);
        w.set(IVec3::new(8, 3, 8), 1 << 4);
        let mut p = standing(&w);
        p.keys.insert(KeyCode::Space);
        p.tick(&w);
        assert!(p.jumped);
        for _ in 0..3 {
            p.tick(&w);
            assert!(!p.jumped);
        }
        assert!(p.grounded);
        p.keys.clear();
        p.tick(&w);
        p.keys.insert(KeyCode::Space);
        p.tick(&w);
        assert!(
            p.jumped,
            "new press may jump before the held-jump timer expires"
        );
    }
    #[test]
    fn step_half_slab_but_not_full_cube_or_under_low_ceiling() {
        let mut w = flat_world();
        let body = Aabb::player(Vec3::new(7.5, 1.0, 8.5));
        let delta = Vec3::new(0.9, -0.0784, 0.0);
        w.set(IVec3::new(8, 1, 8), 44 << 4);
        let step = move_body(&w, body, delta, true, false);
        close(step.bounds.feet().x, 8.4);
        close(step.bounds.feet().y, 1.5);
        assert!(step.grounded);
        w.set(IVec3::new(8, 3, 8), 1 << 4);
        let ceiling = move_body(&w, body, delta, true, false);
        close(ceiling.bounds.feet().x, 7.7);
        close(ceiling.bounds.feet().y, 1.0);
        w.set(IVec3::new(8, 3, 8), 0);
        w.set(IVec3::new(8, 1, 8), 1 << 4);
        let cube = move_body(&w, body, delta, true, false);
        close(cube.bounds.feet().x, 7.7);
        close(cube.bounds.feet().y, 1.0);
    }
    #[test]
    fn crouching_keeps_support_at_diagonal_edges() {
        let mut w = flat_world();
        for x in 0..16 {
            for z in 0..16 {
                if x != 8 || z != 8 {
                    w.set(IVec3::new(x, 0, z), 0);
                }
            }
        }
        let mut body = Aabb::player(w.spawn);
        for _ in 0..10 {
            let result = move_body(&w, body, Vec3::new(0.2, -0.0784, 0.2), true, true);
            assert!(result.grounded);
            body = result.bounds;
        }
        assert!(body.feet().x < 9.3 && body.feet().z < 9.3);
        close(body.feet().y, 1.0);
        let fall = move_body(&w, body, Vec3::new(1.0, -0.0784, 1.0), true, false);
        assert!(fall.bounds.feet().x > 9.3);
    }
    #[test]
    fn open_doors_have_rotated_thin_collision_and_signs_do_not_block() {
        let mut w = flat_world();
        let p = IVec3::new(8, 1, 8);
        w.set(p, (64 << 4) | 1);
        w.set(p + IVec3::Y, (64 << 4) | 8);
        let body = Aabb::player(Vec3::new(8.5, 1.0, 7.5));
        let delta = Vec3::new(0.0, -0.0784, 1.0);
        let closed = move_body(&w, body, delta, true, false);
        close(closed.bounds.feet().z, 7.7);
        w.set(p, (64 << 4) | 5);
        let open = move_body(&w, body, delta, true, false);
        close(open.bounds.feet().z, 8.5);
        w.set(p, 63 << 4);
        w.set(p + IVec3::Y, 68 << 4);
        let sign = move_body(&w, body, delta, true, false);
        close(sign.bounds.feet().z, 8.5);
    }
    #[test]
    fn water_acceleration_is_not_boosted_by_sprint_and_swim_is_not_ground_jump() {
        let mut w = flat_world();
        for x in 7..=9 {
            for z in 6..=9 {
                for y in 1..=3 {
                    w.set(IVec3::new(x, y, z), 9 << 4);
                }
            }
        }
        let mut p = Player::new(Vec3::new(8.5, 1.1, 8.5));
        p.yaw = 0.0;
        p.keys
            .extend([KeyCode::KeyW, KeyCode::ControlLeft, KeyCode::Space]);
        p.tick(&w);
        assert!(p.in_water);
        assert!(!p.jumped);
        close(8.5 - p.position.z, 0.0196);
        close(p.position.y, 1.14);
        close(p.velocity.y, 0.012);
        close(p.velocity.z, -0.01568);
    }
    #[test]
    fn shallow_water_levels_still_apply_current_and_swimming_drag() {
        for level in 5_u16..=7 {
            let mut world = flat_world();
            for x in 7..=9 {
                for z in 7..=9 {
                    let depth = (level as i32 + x - 8).min(7) as u16;
                    world.set(IVec3::new(x, 1, z), (8 << 4) | depth);
                }
            }
            let mut player = Player::new(Vec3::new(8.5, 1.0, 8.5));
            player.tick(&world);
            assert!(
                player.in_water,
                "level {level} must be wet despite shallow visible surface"
            );
            close(player.position.x, 8.514);
            close(player.velocity.x, 0.0112);
            close(player.velocity.y, -0.02);
        }
    }
    #[test]
    fn lava_contact_and_climb_out_use_original_material_cell_queries() {
        let mut world = flat_world();
        world.set(IVec3::new(8, 1, 8), (11 << 4) | 7);
        assert!(!touches_lava(
            &world,
            Aabb::player(Vec3::new(7.75, 1., 8.5))
        ));
        assert!(touches_lava(&world, Aabb::player(Vec3::new(7.81, 1., 8.5))));
        assert!(touches_lava(&world, Aabb::player(Vec3::new(8.5, 1.5, 8.5))));
        // World.isAnyLiquid does not allow a wall-jump merely because the
        // candidate AABB lies above the shallow liquid's rendered surface.
        assert!(touches_any_liquid(
            &world,
            Aabb::player(Vec3::new(8.5, 1.9, 8.5))
        ));
        assert!(!touches_any_liquid(
            &world,
            Aabb::player(Vec3::new(8.5, 2.0, 8.5))
        ));
    }
    #[test]
    fn creative_flight_thrust_precedes_deadband_and_separate_jump() {
        for liquid in [8_u16, 10] {
            let mut world = flat_world();
            world.set(IVec3::new(8, 1, 8), liquid << 4);
            let mut player = Player::new(Vec3::new(8.5, 1.1, 8.5));
            player.flying = true;
            player.keys.insert(KeyCode::Space);
            player.tick(&world);
            // EntityPlayerSP +.15, LivingBase swim-jump +.04, then the
            // EntityPlayer flight override restores the captured Y * .6.
            close(player.position.y, 1.29);
            close(player.velocity.y, 0.114);
            assert!(!player.jumped);
        }
        let world = flat_world();
        let mut player = Player::new(Vec3::new(8.5, 1., 8.5));
        player.flying = true;
        player.grounded = true;
        player.keys.insert(KeyCode::Space);
        player.tick(&world);
        close(player.position.y, 1.42);
        close(player.velocity.y, 0.252);
        assert!(player.jumped);
        let mut player = Player::new(Vec3::new(8.5, 4., 8.5));
        player.flying = true;
        player.velocity.y = -0.148;
        player.keys.insert(KeyCode::Space);
        player.tick(&world);
        close(player.position.y, 4.0);
        close(player.velocity.y, 0.0);
    }
    #[test]
    fn creative_flight_uses_inertia_and_collides_with_walls() {
        let mut w = flat_world();
        for y in 1..=5 {
            w.set(IVec3::new(8, y, 6), 1 << 4);
        }
        let mut p = Player::new(Vec3::new(8.5, 3.0, 8.5));
        p.yaw = 0.0;
        p.flying = true;
        p.keys.insert(KeyCode::KeyW);
        p.tick(&w);
        close(8.5 - p.position.z, 0.049);
        p.keys.clear();
        let before = p.position.z;
        p.tick(&w);
        close(before - p.position.z, 0.04459);
        p.keys.insert(KeyCode::KeyW);
        for _ in 0..30 {
            p.tick(&w);
            assert!(p.position.z >= 7.3);
        }
        close(p.position.z, 7.3);
    }
    #[test]
    fn creative_flight_coasts_for_twenty_ticks_after_key_release() {
        let mut w = flat_world();
        w.chunks.insert((0, -1), w.chunks[&(0, 0)].clone());
        let mut p = Player::new(Vec3::new(8.5, 20., 8.5));
        p.yaw = 0.;
        p.flying = true;
        p.keys.insert(KeyCode::KeyW);
        for _ in 0..20 {
            p.tick(&w);
        }
        // Reference recurrence from EntityPlayer.moveEntityWithHeading and
        // EntityLivingBase: acceleration=(float)(.98f*.05f), drag=(double).91f.
        close(8.5 - p.position.z, 6.2187476);
        close(-p.velocity.z, 0.42031293);
        p.keys.clear();
        p.tick(&w);
        close(8.5 - p.position.z, 6.6390605);
        close(-p.velocity.z, 0.38248476);
        for _ in 1..20 {
            p.tick(&w);
        }
        close(8.5 - p.position.z, 10.180689);
        close(-p.velocity.z, 0.06373835);
        assert!(p.flying);
    }
    #[test]
    fn flowing_water_pushes_swimmer_but_not_flying_player() {
        let mut w = flat_world();
        for x in 7..=9 {
            for z in 7..=9 {
                w.set(IVec3::new(x, 1, z), (8 << 4) | (x - 7) as u16);
            }
        }
        let mut p = Player::new(Vec3::new(8.5, 1., 8.5));
        p.tick(&w);
        close(p.position.x, 8.514);
        assert!(p.in_water);
        let mut flyer = Player::new(Vec3::new(8.5, 1., 8.5));
        flyer.flying = true;
        flyer.tick(&w);
        close(flyer.position.x, 8.5);
    }
    #[test]
    fn special_surface_heights_and_slime_landing() {
        let mut w = flat_world();
        for (state, height) in [
            (44 << 4, 1.5),
            ((44 << 4) | 8, 2.0),
            (171 << 4, 1.0625),
            ((78 << 4) | 3, 1.375),
            (60 << 4, 2.0),
            (88 << 4, 1.875),
            (85 << 4, 2.5),
        ] {
            w.set(IVec3::new(8, 1, 8), state);
            let fall = move_body(
                &w,
                Aabb::player(Vec3::new(8.5, 5.0, 8.5)),
                Vec3::new(0.0, -10.0, 0.0),
                false,
                false,
            );
            close(fall.bounds.feet().y, height);
            assert!(fall.grounded);
        }
        w.set(IVec3::new(8, 1, 8), 165 << 4);
        let mut p = Player::new(Vec3::new(8.5, 2.5, 8.5));
        p.velocity.y = -1.0;
        p.tick(&w);
        close(p.position.y, 2.0);
        close(p.velocity.y, 0.9016);
        p.position.y = 2.5;
        p.velocity.y = -1.0;
        p.keys.insert(KeyCode::ShiftLeft);
        p.tick(&w);
        close(p.velocity.y, -0.0784);
    }
    #[test]
    fn falls_lands_and_jumps_without_floor_penetration() {
        let mut c = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                c.set(x, 0, z, 16);
            }
        }
        let w = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(c))]),
            spawn: Vec3::new(8.0, 2.0, 8.0),
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut p = Player::new(Vec3::new(8.0, 10.0, 8.0));
        for _ in 0..100 {
            p.tick(&w);
        }
        assert!((p.position.y - 1.0).abs() < 0.001);
        p.position = Vec3::new(8.0, 200.0, 8.0);
        p.previous = p.position;
        p.velocity = Vec3::ZERO;
        p.grounded = false;
        for _ in 0..160 {
            p.tick(&w);
            assert!(p.position.y >= 1.0);
        }
        assert!((p.position.y - 1.0).abs() < 0.001);
        assert!(p.grounded);
        p.keys.insert(KeyCode::Space);
        p.tick(&w);
        assert!(p.position.y > 1.4);
        p.keys.clear();
        for _ in 0..60 {
            p.tick(&w);
        }
        assert!((p.position.y - 1.0).abs() < 0.001);
    }
    #[test]
    fn creative_break_place_and_collision_use_the_same_world() {
        let mut c = Chunk::empty();
        for y in 0..2 {
            for x in 0..16 {
                for z in 0..16 {
                    c.set(x, y, z, 16);
                }
            }
        }
        let mut w = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(c))]),
            spawn: Vec3::new(8.5, 2.0, 8.5),
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut p = Player::new(w.spawn);
        p.yaw = 0.0;
        p.pitch = -1.5;
        p.selected = 7;
        let hit = p.edit(&mut w, false).unwrap();
        assert_eq!(w.get(hit), 0);
        assert_eq!(p.edit(&mut w, true), Some(hit));
        assert_eq!(w.get(hit), 45 << 4);
        assert_eq!(w.edits[&hit.to_array()], 45 << 4);
    }
    #[test]
    fn mounted_camera_interpolates_between_ticks_and_ignores_sneak_dip() {
        let mut p = Player::new(Vec3::new(0.0, 1.0, 0.0));
        // Initial mount tick snaps previous == position.
        p.set_position_f64([2.0, 1.85, 4.0]);
        p.riding = true;
        close(p.eye(0.5).x, 2.0);
        // Subsequent mounted tick preserves previous before updating seat position.
        p.begin_mounted_tick();
        p.keys.insert(KeyCode::ShiftLeft);
        p.set_position_f64([4.0, 1.85, 8.0]);
        close(p.eye(0.5).x, 3.0);
        close(p.eye(0.5).z, 6.0);
        close(p.eye(0.5).y, 1.85 + 1.62);
        close(p.precise_eye(0.25).x as f32, 2.5);
        // Explicit server teleport snaps previous even while mounted.
        p.set_pose_f64([10.0, 1.85, 10.0], 0.0, 0.0, false);
        close(p.eye(0.25).x, 10.0);
    }
    #[test]
    fn nether_portal_step_ramps_triggers_sound_once_closes_gui_and_decays() {
        let mut c = Chunk::empty();
        for x in 0..16 {
            for z in 0..16 {
                c.set(x, 0, z, 16);
            }
        }
        c.set(8, 1, 8, 90 << 4);
        c.set(8, 2, 8, 90 << 4);
        let w = World {
            chunks: crate::world::ChunkMap::from([((0, 0), Arc::new(c))]),
            spawn: Vec3::new(8.5, 1.0, 8.5),
            label: String::new(),
            save_path: PathBuf::new(),
            edits: Default::default(),
            imported: false,
            light_revision: 0,
        };
        let mut p = Player::new(Vec3::new(8.5, 1.0, 8.5));
        p.tick(&w);
        assert!(p.touching_portal());
        p.apply_portal_contact();
        assert!(p.in_portal);

        let first = p.step_portal_client(false, 0.5);
        assert!(first.is_some());
        close(first.unwrap(), 1.0);
        assert!(p.close_portal_gui);
        assert!(!p.in_portal);
        close(p.prev_time_in_portal, 0.0);
        close(p.time_in_portal, 0.0125);

        for _ in 1..80 {
            p.in_portal = true;
            assert!(p.step_portal_client(false, 0.5).is_none());
        }
        close(p.time_in_portal, 1.0);

        // Exiting the portal decays at -0.05 per tick down to 0.0.
        for _ in 0..10 {
            assert!(p.step_portal_client(false, 0.5).is_none());
        }
        close(p.time_in_portal, 0.5);
        for _ in 0..15 {
            assert!(p.step_portal_client(false, 0.5).is_none());
        }
        close(p.time_in_portal, 0.0);

        // Portal cooldown blocks re-triggering and refreshes to 10 while touching.
        p.portal_cooldown = 3;
        p.tick(&w);
        p.apply_portal_contact();
        assert!(!p.in_portal);
        assert_eq!(p.portal_cooldown, 10);
    }
}
