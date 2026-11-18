//! Original ocean-monument block template transcription.
#![allow(
    unused_variables,
    unused_mut,
    unused_assignments,
    unused_parens,
    clippy::double_parens,
    clippy::unnecessary_cast,
    clippy::too_many_arguments
)]
use super::population::room;
use super::*;
impl Piece {
    pub(super) fn template_penthouse(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let mut n: i32;
        self.fill(world, clip, 2, -1, 2, 11, -1, 11, 2689, 2689, false);
        self.fill(world, clip, 0, -1, 0, 1, -1, 11, 2688, 2688, false);
        self.fill(world, clip, 12, -1, 0, 13, -1, 11, 2688, 2688, false);
        self.fill(world, clip, 2, -1, 0, 11, -1, 1, 2688, 2688, false);
        self.fill(world, clip, 2, -1, 12, 11, -1, 13, 2688, 2688, false);
        self.fill(world, clip, 0, 0, 0, 0, 0, 13, 2689, 2689, false);
        self.fill(world, clip, 13, 0, 0, 13, 0, 13, 2689, 2689, false);
        self.fill(world, clip, 1, 0, 0, 12, 0, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 0, 13, 12, 0, 13, 2689, 2689, false);
        for n in (2..=11).step_by((3) as usize) {
            self.place(world, 2704, 0, 0, n, clip);
            self.place(world, 2704, 13, 0, n, clip);
            self.place(world, 2704, n, 0, 0, clip);
        }
        self.fill(world, clip, 2, 0, 3, 4, 0, 9, 2689, 2689, false);
        self.fill(world, clip, 9, 0, 3, 11, 0, 9, 2689, 2689, false);
        self.fill(world, clip, 4, 0, 9, 9, 0, 11, 2689, 2689, false);
        self.place(world, 2689, 5, 0, 8, clip);
        self.place(world, 2689, 8, 0, 8, clip);
        self.place(world, 2689, 10, 0, 10, clip);
        self.place(world, 2689, 3, 0, 10, clip);
        self.fill(world, clip, 3, 0, 3, 3, 0, 7, 2690, 2690, false);
        self.fill(world, clip, 10, 0, 3, 10, 0, 7, 2690, 2690, false);
        self.fill(world, clip, 6, 0, 10, 7, 0, 10, 2690, 2690, false);
        n = 3;
        for i in (0..2) {
            for j in (2..=8).step_by((3) as usize) {
                self.fill(world, clip, n, 0, j, n, 2, j, 2689, 2689, false);
            }
            n = 10;
        }
        self.fill(world, clip, 5, 0, 10, 5, 2, 10, 2689, 2689, false);
        self.fill(world, clip, 8, 0, 10, 8, 2, 10, 2689, 2689, false);
        self.fill(world, clip, 6, -1, 7, 7, -1, 8, 2690, 2690, false);
        self.water(world, clip, 6, -1, 3, 7, -1, 4, false);
        self.elder(world, clip, 6, 1, 6);
    }
    pub(super) fn template_wingroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        if (self.variant == 0) {
            let mut n: i32;
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    10 - n,
                    3 - n,
                    20 - n,
                    12 + n,
                    3 - n,
                    20,
                    2689,
                    2689,
                    false,
                );
            }
            self.fill(world, clip, 7, 0, 6, 15, 0, 16, 2689, 2689, false);
            self.fill(world, clip, 6, 0, 6, 6, 3, 20, 2689, 2689, false);
            self.fill(world, clip, 16, 0, 6, 16, 3, 20, 2689, 2689, false);
            self.fill(world, clip, 7, 1, 7, 7, 1, 20, 2689, 2689, false);
            self.fill(world, clip, 15, 1, 7, 15, 1, 20, 2689, 2689, false);
            self.fill(world, clip, 7, 1, 6, 9, 3, 6, 2689, 2689, false);
            self.fill(world, clip, 13, 1, 6, 15, 3, 6, 2689, 2689, false);
            self.fill(world, clip, 8, 1, 7, 9, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 13, 1, 7, 14, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 9, 0, 5, 13, 0, 5, 2689, 2689, false);
            self.fill(world, clip, 10, 0, 7, 12, 0, 7, 2690, 2690, false);
            self.fill(world, clip, 8, 0, 10, 8, 0, 12, 2690, 2690, false);
            self.fill(world, clip, 14, 0, 10, 14, 0, 12, 2690, 2690, false);
            for n in (7..=18).rev().step_by((3) as usize) {
                self.place(world, 2704, 6, 3, n, clip);
                self.place(world, 2704, 16, 3, n, clip);
            }
            self.place(world, 2704, 10, 0, 10, clip);
            self.place(world, 2704, 12, 0, 10, clip);
            self.place(world, 2704, 10, 0, 12, clip);
            self.place(world, 2704, 12, 0, 12, clip);
            self.place(world, 2704, 8, 3, 6, clip);
            self.place(world, 2704, 14, 3, 6, clip);
            self.place(world, 2689, 4, 2, 4, clip);
            self.place(world, 2704, 4, 1, 4, clip);
            self.place(world, 2689, 4, 0, 4, clip);
            self.place(world, 2689, 18, 2, 4, clip);
            self.place(world, 2704, 18, 1, 4, clip);
            self.place(world, 2689, 18, 0, 4, clip);
            self.place(world, 2689, 4, 2, 18, clip);
            self.place(world, 2704, 4, 1, 18, clip);
            self.place(world, 2689, 4, 0, 18, clip);
            self.place(world, 2689, 18, 2, 18, clip);
            self.place(world, 2704, 18, 1, 18, clip);
            self.place(world, 2689, 18, 0, 18, clip);
            self.place(world, 2689, 9, 7, 20, clip);
            self.place(world, 2689, 13, 7, 20, clip);
            self.fill(world, clip, 6, 0, 21, 7, 4, 21, 2689, 2689, false);
            self.fill(world, clip, 15, 0, 21, 16, 4, 21, 2689, 2689, false);
            self.elder(world, clip, 11, 2, 16);
        } else if (self.variant == 1) {
            let mut n: i32;
            self.fill(world, clip, 9, 3, 18, 13, 3, 20, 2689, 2689, false);
            self.fill(world, clip, 9, 0, 18, 9, 2, 18, 2689, 2689, false);
            self.fill(world, clip, 13, 0, 18, 13, 2, 18, 2689, 2689, false);
            let mut n2 = 9;
            let mut n3 = 20;
            let mut n4 = 5;
            for n in (0..2) {
                self.place(world, 2689, n2, n4 + 1, n3, clip);
                self.place(world, 2704, n2, n4, n3, clip);
                self.place(world, 2689, n2, n4 - 1, n3, clip);
                n2 = 13;
            }
            self.fill(world, clip, 7, 3, 7, 15, 3, 14, 2689, 2689, false);
            n2 = 10;
            for n in (0..2) {
                self.fill(world, clip, n2, 0, 10, n2, 6, 10, 2689, 2689, false);
                self.fill(world, clip, n2, 0, 12, n2, 6, 12, 2689, 2689, false);
                self.place(world, 2704, n2, 0, 10, clip);
                self.place(world, 2704, n2, 0, 12, clip);
                self.place(world, 2704, n2, 4, 10, clip);
                self.place(world, 2704, n2, 4, 12, clip);
                n2 = 12;
            }
            n2 = 8;
            for n in (0..2) {
                self.fill(world, clip, n2, 0, 7, n2, 2, 7, 2689, 2689, false);
                self.fill(world, clip, n2, 0, 14, n2, 2, 14, 2689, 2689, false);
                n2 = 14;
            }
            self.fill(world, clip, 8, 3, 8, 8, 3, 13, 2690, 2690, false);
            self.fill(world, clip, 14, 3, 8, 14, 3, 13, 2690, 2690, false);
            self.elder(world, clip, 11, 5, 13);
        }
    }
    pub(super) fn template_monumentcoreroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        self.water_replace(world, clip, 1, 8, 0, 14, 8, 14, 2688);
        let mut n = 7;
        let mut block_state = 2689;
        self.fill(
            world,
            clip,
            0,
            n,
            0,
            0,
            n,
            15,
            block_state,
            block_state,
            false,
        );
        self.fill(
            world,
            clip,
            15,
            n,
            0,
            15,
            n,
            15,
            block_state,
            block_state,
            false,
        );
        self.fill(
            world,
            clip,
            1,
            n,
            0,
            15,
            n,
            0,
            block_state,
            block_state,
            false,
        );
        self.fill(
            world,
            clip,
            1,
            n,
            15,
            14,
            n,
            15,
            block_state,
            block_state,
            false,
        );
        for n in (1..=6) {
            block_state = 2689;
            if (n == 2 || n == 6) {
                block_state = 2688;
            }
            for i in (0..=15).step_by((15) as usize) {
                self.fill(
                    world,
                    clip,
                    i,
                    n,
                    0,
                    i,
                    n,
                    1,
                    block_state,
                    block_state,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    i,
                    n,
                    6,
                    i,
                    n,
                    9,
                    block_state,
                    block_state,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    i,
                    n,
                    14,
                    i,
                    n,
                    15,
                    block_state,
                    block_state,
                    false,
                );
            }
            self.fill(
                world,
                clip,
                1,
                n,
                0,
                1,
                n,
                0,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                6,
                n,
                0,
                9,
                n,
                0,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                14,
                n,
                0,
                14,
                n,
                0,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                1,
                n,
                15,
                14,
                n,
                15,
                block_state,
                block_state,
                false,
            );
        }
        self.fill(world, clip, 6, 3, 6, 9, 6, 9, 2690, 2690, false);
        self.fill(world, clip, 7, 4, 7, 8, 5, 8, 656, 656, false);
        for n in (3..=6).step_by((3) as usize) {
            for i in (6..=9).step_by((3) as usize) {
                self.place(world, 2704, i, n, 6, clip);
                self.place(world, 2704, i, n, 9, clip);
            }
        }
        self.fill(world, clip, 5, 1, 6, 5, 2, 6, 2689, 2689, false);
        self.fill(world, clip, 5, 1, 9, 5, 2, 9, 2689, 2689, false);
        self.fill(world, clip, 10, 1, 6, 10, 2, 6, 2689, 2689, false);
        self.fill(world, clip, 10, 1, 9, 10, 2, 9, 2689, 2689, false);
        self.fill(world, clip, 6, 1, 5, 6, 2, 5, 2689, 2689, false);
        self.fill(world, clip, 9, 1, 5, 9, 2, 5, 2689, 2689, false);
        self.fill(world, clip, 6, 1, 10, 6, 2, 10, 2689, 2689, false);
        self.fill(world, clip, 9, 1, 10, 9, 2, 10, 2689, 2689, false);
        self.fill(world, clip, 5, 2, 5, 5, 6, 5, 2689, 2689, false);
        self.fill(world, clip, 5, 2, 10, 5, 6, 10, 2689, 2689, false);
        self.fill(world, clip, 10, 2, 5, 10, 6, 5, 2689, 2689, false);
        self.fill(world, clip, 10, 2, 10, 10, 6, 10, 2689, 2689, false);
        self.fill(world, clip, 5, 7, 1, 5, 7, 6, 2689, 2689, false);
        self.fill(world, clip, 10, 7, 1, 10, 7, 6, 2689, 2689, false);
        self.fill(world, clip, 5, 7, 9, 5, 7, 14, 2689, 2689, false);
        self.fill(world, clip, 10, 7, 9, 10, 7, 14, 2689, 2689, false);
        self.fill(world, clip, 1, 7, 5, 6, 7, 5, 2689, 2689, false);
        self.fill(world, clip, 1, 7, 10, 6, 7, 10, 2689, 2689, false);
        self.fill(world, clip, 9, 7, 5, 14, 7, 5, 2689, 2689, false);
        self.fill(world, clip, 9, 7, 10, 14, 7, 10, 2689, 2689, false);
        self.fill(world, clip, 2, 1, 2, 2, 1, 3, 2689, 2689, false);
        self.fill(world, clip, 3, 1, 2, 3, 1, 2, 2689, 2689, false);
        self.fill(world, clip, 13, 1, 2, 13, 1, 3, 2689, 2689, false);
        self.fill(world, clip, 12, 1, 2, 12, 1, 2, 2689, 2689, false);
        self.fill(world, clip, 2, 1, 12, 2, 1, 13, 2689, 2689, false);
        self.fill(world, clip, 3, 1, 13, 3, 1, 13, 2689, 2689, false);
        self.fill(world, clip, 13, 1, 12, 13, 1, 13, 2689, 2689, false);
        self.fill(world, clip, 12, 1, 13, 12, 1, 13, 2689, 2689, false);
    }
    pub(super) fn template_doubleyzroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        let mut block_state: u16;
        let mut n: i32;
        let mut room_definition = room(rooms, base.neighbors[2].unwrap());
        let mut room_definition2 = base;
        let mut room_definition3 = room(rooms, room_definition.neighbors[1].unwrap());
        let mut room_definition4 = room(rooms, room_definition2.neighbors[1].unwrap());
        if (base.id / 25 > 0) {
            self.floor(world, clip, 0, 8, room_definition.open[0]);
            self.floor(world, clip, 0, 0, room_definition2.open[0]);
        }
        if (room_definition4.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 8, 1, 6, 8, 7, 2688);
        }
        if (room_definition3.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 8, 8, 6, 8, 14, 2688);
        }
        for n in (1..=7) {
            block_state = 2689;
            if (n == 2 || n == 6) {
                block_state = 2688;
            }
            self.fill(
                world,
                clip,
                0,
                n,
                0,
                0,
                n,
                15,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                7,
                n,
                0,
                7,
                n,
                15,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                1,
                n,
                0,
                6,
                n,
                0,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                1,
                n,
                15,
                6,
                n,
                15,
                block_state,
                block_state,
                false,
            );
        }
        for n in (1..=7) {
            block_state = 2690;
            if (n == 2 || n == 6) {
                block_state = 2704;
            }
            self.fill(
                world,
                clip,
                3,
                n,
                7,
                4,
                n,
                8,
                block_state,
                block_state,
                false,
            );
        }
        if (room_definition2.open[3]) {
            self.water(world, clip, 3, 1, 0, 4, 2, 0, false);
        }
        if (room_definition2.open[5]) {
            self.water(world, clip, 7, 1, 3, 7, 2, 4, false);
        }
        if (room_definition2.open[4]) {
            self.water(world, clip, 0, 1, 3, 0, 2, 4, false);
        }
        if (room_definition.open[2]) {
            self.water(world, clip, 3, 1, 15, 4, 2, 15, false);
        }
        if (room_definition.open[4]) {
            self.water(world, clip, 0, 1, 11, 0, 2, 12, false);
        }
        if (room_definition.open[5]) {
            self.water(world, clip, 7, 1, 11, 7, 2, 12, false);
        }
        if (room_definition4.open[3]) {
            self.water(world, clip, 3, 5, 0, 4, 6, 0, false);
        }
        if (room_definition4.open[5]) {
            self.water(world, clip, 7, 5, 3, 7, 6, 4, false);
            self.fill(world, clip, 5, 4, 2, 6, 4, 5, 2689, 2689, false);
            self.fill(world, clip, 6, 1, 2, 6, 3, 2, 2689, 2689, false);
            self.fill(world, clip, 6, 1, 5, 6, 3, 5, 2689, 2689, false);
        }
        if (room_definition4.open[4]) {
            self.water(world, clip, 0, 5, 3, 0, 6, 4, false);
            self.fill(world, clip, 1, 4, 2, 2, 4, 5, 2689, 2689, false);
            self.fill(world, clip, 1, 1, 2, 1, 3, 2, 2689, 2689, false);
            self.fill(world, clip, 1, 1, 5, 1, 3, 5, 2689, 2689, false);
        }
        if (room_definition3.open[2]) {
            self.water(world, clip, 3, 5, 15, 4, 6, 15, false);
        }
        if (room_definition3.open[4]) {
            self.water(world, clip, 0, 5, 11, 0, 6, 12, false);
            self.fill(world, clip, 1, 4, 10, 2, 4, 13, 2689, 2689, false);
            self.fill(world, clip, 1, 1, 10, 1, 3, 10, 2689, 2689, false);
            self.fill(world, clip, 1, 1, 13, 1, 3, 13, 2689, 2689, false);
        }
        if (room_definition3.open[5]) {
            self.water(world, clip, 7, 5, 11, 7, 6, 12, false);
            self.fill(world, clip, 5, 4, 10, 6, 4, 13, 2689, 2689, false);
            self.fill(world, clip, 6, 1, 10, 6, 3, 10, 2689, 2689, false);
            self.fill(world, clip, 6, 1, 13, 6, 3, 13, 2689, 2689, false);
        }
    }
    pub(super) fn template_doublexyroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        let mut room_definition = room(rooms, base.neighbors[5].unwrap());
        let mut room_definition2 = base;
        let mut room_definition3 = room(rooms, room_definition2.neighbors[1].unwrap());
        let mut room_definition4 = room(rooms, room_definition.neighbors[1].unwrap());
        if (base.id / 25 > 0) {
            self.floor(world, clip, 8, 0, room_definition.open[0]);
            self.floor(world, clip, 0, 0, room_definition2.open[0]);
        }
        if (room_definition3.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 8, 1, 7, 8, 6, 2688);
        }
        if (room_definition4.neighbors[1].is_none()) {
            self.water_replace(world, clip, 8, 8, 1, 14, 8, 6, 2688);
        }
        for i in (1..=7) {
            let mut block_state = 2689;
            if (i == 2 || i == 6) {
                block_state = 2688;
            }
            self.fill(
                world,
                clip,
                0,
                i,
                0,
                0,
                i,
                7,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                15,
                i,
                0,
                15,
                i,
                7,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                1,
                i,
                0,
                15,
                i,
                0,
                block_state,
                block_state,
                false,
            );
            self.fill(
                world,
                clip,
                1,
                i,
                7,
                14,
                i,
                7,
                block_state,
                block_state,
                false,
            );
        }
        self.fill(world, clip, 2, 1, 3, 2, 7, 4, 2689, 2689, false);
        self.fill(world, clip, 3, 1, 2, 4, 7, 2, 2689, 2689, false);
        self.fill(world, clip, 3, 1, 5, 4, 7, 5, 2689, 2689, false);
        self.fill(world, clip, 13, 1, 3, 13, 7, 4, 2689, 2689, false);
        self.fill(world, clip, 11, 1, 2, 12, 7, 2, 2689, 2689, false);
        self.fill(world, clip, 11, 1, 5, 12, 7, 5, 2689, 2689, false);
        self.fill(world, clip, 5, 1, 3, 5, 3, 4, 2689, 2689, false);
        self.fill(world, clip, 10, 1, 3, 10, 3, 4, 2689, 2689, false);
        self.fill(world, clip, 5, 7, 2, 10, 7, 5, 2689, 2689, false);
        self.fill(world, clip, 5, 5, 2, 5, 7, 2, 2689, 2689, false);
        self.fill(world, clip, 10, 5, 2, 10, 7, 2, 2689, 2689, false);
        self.fill(world, clip, 5, 5, 5, 5, 7, 5, 2689, 2689, false);
        self.fill(world, clip, 10, 5, 5, 10, 7, 5, 2689, 2689, false);
        self.place(world, 2689, 6, 6, 2, clip);
        self.place(world, 2689, 9, 6, 2, clip);
        self.place(world, 2689, 6, 6, 5, clip);
        self.place(world, 2689, 9, 6, 5, clip);
        self.fill(world, clip, 5, 4, 3, 6, 4, 4, 2689, 2689, false);
        self.fill(world, clip, 9, 4, 3, 10, 4, 4, 2689, 2689, false);
        self.place(world, 2704, 5, 4, 2, clip);
        self.place(world, 2704, 5, 4, 5, clip);
        self.place(world, 2704, 10, 4, 2, clip);
        self.place(world, 2704, 10, 4, 5, clip);
        if (room_definition2.open[3]) {
            self.water(world, clip, 3, 1, 0, 4, 2, 0, false);
        }
        if (room_definition2.open[2]) {
            self.water(world, clip, 3, 1, 7, 4, 2, 7, false);
        }
        if (room_definition2.open[4]) {
            self.water(world, clip, 0, 1, 3, 0, 2, 4, false);
        }
        if (room_definition.open[3]) {
            self.water(world, clip, 11, 1, 0, 12, 2, 0, false);
        }
        if (room_definition.open[2]) {
            self.water(world, clip, 11, 1, 7, 12, 2, 7, false);
        }
        if (room_definition.open[5]) {
            self.water(world, clip, 15, 1, 3, 15, 2, 4, false);
        }
        if (room_definition3.open[3]) {
            self.water(world, clip, 3, 5, 0, 4, 6, 0, false);
        }
        if (room_definition3.open[2]) {
            self.water(world, clip, 3, 5, 7, 4, 6, 7, false);
        }
        if (room_definition3.open[4]) {
            self.water(world, clip, 0, 5, 3, 0, 6, 4, false);
        }
        if (room_definition4.open[3]) {
            self.water(world, clip, 11, 5, 0, 12, 6, 0, false);
        }
        if (room_definition4.open[2]) {
            self.water(world, clip, 11, 5, 7, 12, 6, 7, false);
        }
        if (room_definition4.open[5]) {
            self.water(world, clip, 15, 5, 3, 15, 6, 4, false);
        }
    }
    pub(super) fn template_doublezroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        let mut room_definition = room(rooms, base.neighbors[2].unwrap());
        let mut room_definition2 = base;
        if (base.id / 25 > 0) {
            self.floor(world, clip, 0, 8, room_definition.open[0]);
            self.floor(world, clip, 0, 0, room_definition2.open[0]);
        }
        if (room_definition2.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 4, 1, 6, 4, 7, 2688);
        }
        if (room_definition.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 4, 8, 6, 4, 14, 2688);
        }
        self.fill(world, clip, 0, 3, 0, 0, 3, 15, 2689, 2689, false);
        self.fill(world, clip, 7, 3, 0, 7, 3, 15, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 0, 7, 3, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 15, 6, 3, 15, 2689, 2689, false);
        self.fill(world, clip, 0, 2, 0, 0, 2, 15, 2688, 2688, false);
        self.fill(world, clip, 7, 2, 0, 7, 2, 15, 2688, 2688, false);
        self.fill(world, clip, 1, 2, 0, 7, 2, 0, 2688, 2688, false);
        self.fill(world, clip, 1, 2, 15, 6, 2, 15, 2688, 2688, false);
        self.fill(world, clip, 0, 1, 0, 0, 1, 15, 2689, 2689, false);
        self.fill(world, clip, 7, 1, 0, 7, 1, 15, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 0, 7, 1, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 15, 6, 1, 15, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 1, 1, 1, 2, 2689, 2689, false);
        self.fill(world, clip, 6, 1, 1, 6, 1, 2, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 1, 1, 3, 2, 2689, 2689, false);
        self.fill(world, clip, 6, 3, 1, 6, 3, 2, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 13, 1, 1, 14, 2689, 2689, false);
        self.fill(world, clip, 6, 1, 13, 6, 1, 14, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 13, 1, 3, 14, 2689, 2689, false);
        self.fill(world, clip, 6, 3, 13, 6, 3, 14, 2689, 2689, false);
        self.fill(world, clip, 2, 1, 6, 2, 3, 6, 2689, 2689, false);
        self.fill(world, clip, 5, 1, 6, 5, 3, 6, 2689, 2689, false);
        self.fill(world, clip, 2, 1, 9, 2, 3, 9, 2689, 2689, false);
        self.fill(world, clip, 5, 1, 9, 5, 3, 9, 2689, 2689, false);
        self.fill(world, clip, 3, 2, 6, 4, 2, 6, 2689, 2689, false);
        self.fill(world, clip, 3, 2, 9, 4, 2, 9, 2689, 2689, false);
        self.fill(world, clip, 2, 2, 7, 2, 2, 8, 2689, 2689, false);
        self.fill(world, clip, 5, 2, 7, 5, 2, 8, 2689, 2689, false);
        self.place(world, 2704, 2, 2, 5, clip);
        self.place(world, 2704, 5, 2, 5, clip);
        self.place(world, 2704, 2, 2, 10, clip);
        self.place(world, 2704, 5, 2, 10, clip);
        self.place(world, 2689, 2, 3, 5, clip);
        self.place(world, 2689, 5, 3, 5, clip);
        self.place(world, 2689, 2, 3, 10, clip);
        self.place(world, 2689, 5, 3, 10, clip);
        if (room_definition2.open[3]) {
            self.water(world, clip, 3, 1, 0, 4, 2, 0, false);
        }
        if (room_definition2.open[5]) {
            self.water(world, clip, 7, 1, 3, 7, 2, 4, false);
        }
        if (room_definition2.open[4]) {
            self.water(world, clip, 0, 1, 3, 0, 2, 4, false);
        }
        if (room_definition.open[2]) {
            self.water(world, clip, 3, 1, 15, 4, 2, 15, false);
        }
        if (room_definition.open[4]) {
            self.water(world, clip, 0, 1, 11, 0, 2, 12, false);
        }
        if (room_definition.open[5]) {
            self.water(world, clip, 7, 1, 11, 7, 2, 12, false);
        }
    }
    pub(super) fn template_doublexroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        let mut room_definition = room(rooms, base.neighbors[5].unwrap());
        let mut room_definition2 = base;
        if (base.id / 25 > 0) {
            self.floor(world, clip, 8, 0, room_definition.open[0]);
            self.floor(world, clip, 0, 0, room_definition2.open[0]);
        }
        if (room_definition2.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 4, 1, 7, 4, 6, 2688);
        }
        if (room_definition.neighbors[1].is_none()) {
            self.water_replace(world, clip, 8, 4, 1, 14, 4, 6, 2688);
        }
        self.fill(world, clip, 0, 3, 0, 0, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 15, 3, 0, 15, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 0, 15, 3, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 7, 14, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 0, 2, 0, 0, 2, 7, 2688, 2688, false);
        self.fill(world, clip, 15, 2, 0, 15, 2, 7, 2688, 2688, false);
        self.fill(world, clip, 1, 2, 0, 15, 2, 0, 2688, 2688, false);
        self.fill(world, clip, 1, 2, 7, 14, 2, 7, 2688, 2688, false);
        self.fill(world, clip, 0, 1, 0, 0, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 15, 1, 0, 15, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 0, 15, 1, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 7, 14, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 5, 1, 0, 10, 1, 4, 2689, 2689, false);
        self.fill(world, clip, 6, 2, 0, 9, 2, 3, 2688, 2688, false);
        self.fill(world, clip, 5, 3, 0, 10, 3, 4, 2689, 2689, false);
        self.place(world, 2704, 6, 2, 3, clip);
        self.place(world, 2704, 9, 2, 3, clip);
        if (room_definition2.open[3]) {
            self.water(world, clip, 3, 1, 0, 4, 2, 0, false);
        }
        if (room_definition2.open[2]) {
            self.water(world, clip, 3, 1, 7, 4, 2, 7, false);
        }
        if (room_definition2.open[4]) {
            self.water(world, clip, 0, 1, 3, 0, 2, 4, false);
        }
        if (room_definition.open[3]) {
            self.water(world, clip, 11, 1, 0, 12, 2, 0, false);
        }
        if (room_definition.open[2]) {
            self.water(world, clip, 11, 1, 7, 12, 2, 7, false);
        }
        if (room_definition.open[5]) {
            self.water(world, clip, 15, 1, 3, 15, 2, 4, false);
        }
    }
    pub(super) fn template_doubleyroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        if (base.id / 25 > 0) {
            self.floor(world, clip, 0, 0, base.open[0]);
        }
        let mut room_definition = room(rooms, base.neighbors[1].unwrap());
        if (room_definition.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 8, 1, 6, 8, 6, 2688);
        }
        self.fill(world, clip, 0, 4, 0, 0, 4, 7, 2689, 2689, false);
        self.fill(world, clip, 7, 4, 0, 7, 4, 7, 2689, 2689, false);
        self.fill(world, clip, 1, 4, 0, 6, 4, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 4, 7, 6, 4, 7, 2689, 2689, false);
        self.fill(world, clip, 2, 4, 1, 2, 4, 2, 2689, 2689, false);
        self.fill(world, clip, 1, 4, 2, 1, 4, 2, 2689, 2689, false);
        self.fill(world, clip, 5, 4, 1, 5, 4, 2, 2689, 2689, false);
        self.fill(world, clip, 6, 4, 2, 6, 4, 2, 2689, 2689, false);
        self.fill(world, clip, 2, 4, 5, 2, 4, 6, 2689, 2689, false);
        self.fill(world, clip, 1, 4, 5, 1, 4, 5, 2689, 2689, false);
        self.fill(world, clip, 5, 4, 5, 5, 4, 6, 2689, 2689, false);
        self.fill(world, clip, 6, 4, 5, 6, 4, 5, 2689, 2689, false);
        let mut room_definition2 = base;
        for i in (1..=5).step_by((4) as usize) {
            let mut n = 0;
            if (room_definition2.open[3]) {
                self.fill(world, clip, 2, i, n, 2, i + 2, n, 2689, 2689, false);
                self.fill(world, clip, 5, i, n, 5, i + 2, n, 2689, 2689, false);
                self.fill(world, clip, 3, i + 2, n, 4, i + 2, n, 2689, 2689, false);
            } else {
                self.fill(world, clip, 0, i, n, 7, i + 2, n, 2689, 2689, false);
                self.fill(world, clip, 0, i + 1, n, 7, i + 1, n, 2688, 2688, false);
            }
            n = 7;
            if (room_definition2.open[2]) {
                self.fill(world, clip, 2, i, n, 2, i + 2, n, 2689, 2689, false);
                self.fill(world, clip, 5, i, n, 5, i + 2, n, 2689, 2689, false);
                self.fill(world, clip, 3, i + 2, n, 4, i + 2, n, 2689, 2689, false);
            } else {
                self.fill(world, clip, 0, i, n, 7, i + 2, n, 2689, 2689, false);
                self.fill(world, clip, 0, i + 1, n, 7, i + 1, n, 2688, 2688, false);
            }
            let mut n2 = 0;
            if (room_definition2.open[4]) {
                self.fill(world, clip, n2, i, 2, n2, i + 2, 2, 2689, 2689, false);
                self.fill(world, clip, n2, i, 5, n2, i + 2, 5, 2689, 2689, false);
                self.fill(world, clip, n2, i + 2, 3, n2, i + 2, 4, 2689, 2689, false);
            } else {
                self.fill(world, clip, n2, i, 0, n2, i + 2, 7, 2689, 2689, false);
                self.fill(world, clip, n2, i + 1, 0, n2, i + 1, 7, 2688, 2688, false);
            }
            n2 = 7;
            if (room_definition2.open[5]) {
                self.fill(world, clip, n2, i, 2, n2, i + 2, 2, 2689, 2689, false);
                self.fill(world, clip, n2, i, 5, n2, i + 2, 5, 2689, 2689, false);
                self.fill(world, clip, n2, i + 2, 3, n2, i + 2, 4, 2689, 2689, false);
            } else {
                self.fill(world, clip, n2, i, 0, n2, i + 2, 7, 2689, 2689, false);
                self.fill(world, clip, n2, i + 1, 0, n2, i + 1, 7, 2688, 2688, false);
            }
            room_definition2 = room_definition;
        }
    }
    pub(super) fn template_simpletoproom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        if (base.id / 25 > 0) {
            self.floor(world, clip, 0, 0, base.open[0]);
        }
        if (base.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 4, 1, 6, 4, 6, 2688);
        }
        for i in (1..=6) {
            for j in (1..=6) {
                if random.next_int(3) == 0 {
                    continue;
                }
                let mut n = 2 + (if random.next_int(4) == 0 { 0 } else { 1 });
                self.fill(
                    world,
                    clip,
                    i,
                    n,
                    j,
                    i,
                    3,
                    j,
                    ((19 << 4) | (1) as u16),
                    ((19 << 4) | (1) as u16),
                    false,
                );
            }
        }
        self.fill(world, clip, 0, 1, 0, 0, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 7, 1, 0, 7, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 0, 6, 1, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 7, 6, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 0, 2, 0, 0, 2, 7, 2690, 2690, false);
        self.fill(world, clip, 7, 2, 0, 7, 2, 7, 2690, 2690, false);
        self.fill(world, clip, 1, 2, 0, 6, 2, 0, 2690, 2690, false);
        self.fill(world, clip, 1, 2, 7, 6, 2, 7, 2690, 2690, false);
        self.fill(world, clip, 0, 3, 0, 0, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 7, 3, 0, 7, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 0, 6, 3, 0, 2689, 2689, false);
        self.fill(world, clip, 1, 3, 7, 6, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 0, 1, 3, 0, 2, 4, 2690, 2690, false);
        self.fill(world, clip, 7, 1, 3, 7, 2, 4, 2690, 2690, false);
        self.fill(world, clip, 3, 1, 0, 4, 2, 0, 2690, 2690, false);
        self.fill(world, clip, 3, 1, 7, 4, 2, 7, 2690, 2690, false);
        if (base.open[3]) {
            self.water(world, clip, 3, 1, 0, 4, 2, 0, false);
        }
    }
    pub(super) fn template_simpleroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        let mut bl: bool;
        if (base.id / 25 > 0) {
            self.floor(world, clip, 0, 0, base.open[0]);
        }
        if (base.neighbors[1].is_none()) {
            self.water_replace(world, clip, 1, 4, 1, 6, 4, 6, 2688);
        }
        let mut bl = self.variant != 0
            && random.next_boolean()
            && !base.open[0]
            && !base.open[1]
            && base.open.iter().filter(|&&v| v).count() > 1;
        if (self.variant == 0) {
            self.fill(world, clip, 0, 1, 0, 2, 1, 2, 2689, 2689, false);
            self.fill(world, clip, 0, 3, 0, 2, 3, 2, 2689, 2689, false);
            self.fill(world, clip, 0, 2, 0, 0, 2, 2, 2688, 2688, false);
            self.fill(world, clip, 1, 2, 0, 2, 2, 0, 2688, 2688, false);
            self.place(world, 2704, 1, 2, 1, clip);
            self.fill(world, clip, 5, 1, 0, 7, 1, 2, 2689, 2689, false);
            self.fill(world, clip, 5, 3, 0, 7, 3, 2, 2689, 2689, false);
            self.fill(world, clip, 7, 2, 0, 7, 2, 2, 2688, 2688, false);
            self.fill(world, clip, 5, 2, 0, 6, 2, 0, 2688, 2688, false);
            self.place(world, 2704, 6, 2, 1, clip);
            self.fill(world, clip, 0, 1, 5, 2, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 0, 3, 5, 2, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 0, 2, 5, 0, 2, 7, 2688, 2688, false);
            self.fill(world, clip, 1, 2, 7, 2, 2, 7, 2688, 2688, false);
            self.place(world, 2704, 1, 2, 6, clip);
            self.fill(world, clip, 5, 1, 5, 7, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 5, 3, 5, 7, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 7, 2, 5, 7, 2, 7, 2688, 2688, false);
            self.fill(world, clip, 5, 2, 7, 6, 2, 7, 2688, 2688, false);
            self.place(world, 2704, 6, 2, 6, clip);
            if (base.open[3]) {
                self.fill(world, clip, 3, 3, 0, 4, 3, 0, 2689, 2689, false);
            } else {
                self.fill(world, clip, 3, 3, 0, 4, 3, 1, 2689, 2689, false);
                self.fill(world, clip, 3, 2, 0, 4, 2, 0, 2688, 2688, false);
                self.fill(world, clip, 3, 1, 0, 4, 1, 1, 2689, 2689, false);
            }
            if (base.open[2]) {
                self.fill(world, clip, 3, 3, 7, 4, 3, 7, 2689, 2689, false);
            } else {
                self.fill(world, clip, 3, 3, 6, 4, 3, 7, 2689, 2689, false);
                self.fill(world, clip, 3, 2, 7, 4, 2, 7, 2688, 2688, false);
                self.fill(world, clip, 3, 1, 6, 4, 1, 7, 2689, 2689, false);
            }
            if (base.open[4]) {
                self.fill(world, clip, 0, 3, 3, 0, 3, 4, 2689, 2689, false);
            } else {
                self.fill(world, clip, 0, 3, 3, 1, 3, 4, 2689, 2689, false);
                self.fill(world, clip, 0, 2, 3, 0, 2, 4, 2688, 2688, false);
                self.fill(world, clip, 0, 1, 3, 1, 1, 4, 2689, 2689, false);
            }
            if (base.open[5]) {
                self.fill(world, clip, 7, 3, 3, 7, 3, 4, 2689, 2689, false);
            } else {
                self.fill(world, clip, 6, 3, 3, 7, 3, 4, 2689, 2689, false);
                self.fill(world, clip, 7, 2, 3, 7, 2, 4, 2688, 2688, false);
                self.fill(world, clip, 6, 1, 3, 7, 1, 4, 2689, 2689, false);
            }
        } else if (self.variant == 1) {
            self.fill(world, clip, 2, 1, 2, 2, 3, 2, 2689, 2689, false);
            self.fill(world, clip, 2, 1, 5, 2, 3, 5, 2689, 2689, false);
            self.fill(world, clip, 5, 1, 5, 5, 3, 5, 2689, 2689, false);
            self.fill(world, clip, 5, 1, 2, 5, 3, 2, 2689, 2689, false);
            self.place(world, 2704, 2, 2, 2, clip);
            self.place(world, 2704, 2, 2, 5, clip);
            self.place(world, 2704, 5, 2, 5, clip);
            self.place(world, 2704, 5, 2, 2, clip);
            self.fill(world, clip, 0, 1, 0, 1, 3, 0, 2689, 2689, false);
            self.fill(world, clip, 0, 1, 1, 0, 3, 1, 2689, 2689, false);
            self.fill(world, clip, 0, 1, 7, 1, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 0, 1, 6, 0, 3, 6, 2689, 2689, false);
            self.fill(world, clip, 6, 1, 7, 7, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 7, 1, 6, 7, 3, 6, 2689, 2689, false);
            self.fill(world, clip, 6, 1, 0, 7, 3, 0, 2689, 2689, false);
            self.fill(world, clip, 7, 1, 1, 7, 3, 1, 2689, 2689, false);
            self.place(world, 2688, 1, 2, 0, clip);
            self.place(world, 2688, 0, 2, 1, clip);
            self.place(world, 2688, 1, 2, 7, clip);
            self.place(world, 2688, 0, 2, 6, clip);
            self.place(world, 2688, 6, 2, 7, clip);
            self.place(world, 2688, 7, 2, 6, clip);
            self.place(world, 2688, 6, 2, 0, clip);
            self.place(world, 2688, 7, 2, 1, clip);
            if (!base.open[3]) {
                self.fill(world, clip, 1, 3, 0, 6, 3, 0, 2689, 2689, false);
                self.fill(world, clip, 1, 2, 0, 6, 2, 0, 2688, 2688, false);
                self.fill(world, clip, 1, 1, 0, 6, 1, 0, 2689, 2689, false);
            }
            if (!base.open[2]) {
                self.fill(world, clip, 1, 3, 7, 6, 3, 7, 2689, 2689, false);
                self.fill(world, clip, 1, 2, 7, 6, 2, 7, 2688, 2688, false);
                self.fill(world, clip, 1, 1, 7, 6, 1, 7, 2689, 2689, false);
            }
            if (!base.open[4]) {
                self.fill(world, clip, 0, 3, 1, 0, 3, 6, 2689, 2689, false);
                self.fill(world, clip, 0, 2, 1, 0, 2, 6, 2688, 2688, false);
                self.fill(world, clip, 0, 1, 1, 0, 1, 6, 2689, 2689, false);
            }
            if (!base.open[5]) {
                self.fill(world, clip, 7, 3, 1, 7, 3, 6, 2689, 2689, false);
                self.fill(world, clip, 7, 2, 1, 7, 2, 6, 2688, 2688, false);
                self.fill(world, clip, 7, 1, 1, 7, 1, 6, 2689, 2689, false);
            }
        } else if (self.variant == 2) {
            self.fill(world, clip, 0, 1, 0, 0, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 7, 1, 0, 7, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 1, 1, 0, 6, 1, 0, 2689, 2689, false);
            self.fill(world, clip, 1, 1, 7, 6, 1, 7, 2689, 2689, false);
            self.fill(world, clip, 0, 2, 0, 0, 2, 7, 2690, 2690, false);
            self.fill(world, clip, 7, 2, 0, 7, 2, 7, 2690, 2690, false);
            self.fill(world, clip, 1, 2, 0, 6, 2, 0, 2690, 2690, false);
            self.fill(world, clip, 1, 2, 7, 6, 2, 7, 2690, 2690, false);
            self.fill(world, clip, 0, 3, 0, 0, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 7, 3, 0, 7, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 1, 3, 0, 6, 3, 0, 2689, 2689, false);
            self.fill(world, clip, 1, 3, 7, 6, 3, 7, 2689, 2689, false);
            self.fill(world, clip, 0, 1, 3, 0, 2, 4, 2690, 2690, false);
            self.fill(world, clip, 7, 1, 3, 7, 2, 4, 2690, 2690, false);
            self.fill(world, clip, 3, 1, 0, 4, 2, 0, 2690, 2690, false);
            self.fill(world, clip, 3, 1, 7, 4, 2, 7, 2690, 2690, false);
            if (base.open[3]) {
                self.water(world, clip, 3, 1, 0, 4, 2, 0, false);
            }
            if (base.open[2]) {
                self.water(world, clip, 3, 1, 7, 4, 2, 7, false);
            }
            if (base.open[4]) {
                self.water(world, clip, 0, 1, 3, 0, 2, 4, false);
            }
            if (base.open[5]) {
                self.water(world, clip, 7, 1, 3, 7, 2, 4, false);
            }
        }
        if (bl) {
            self.fill(world, clip, 3, 1, 3, 4, 1, 4, 2689, 2689, false);
            self.fill(world, clip, 3, 2, 3, 4, 2, 4, 2688, 2688, false);
            self.fill(world, clip, 3, 3, 3, 4, 3, 4, 2689, 2689, false);
        }
    }
    pub(super) fn template_entryroom(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let base = room(rooms, self.room.unwrap());
        self.fill(world, clip, 0, 3, 0, 2, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 5, 3, 0, 7, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 0, 2, 0, 1, 2, 7, 2689, 2689, false);
        self.fill(world, clip, 6, 2, 0, 7, 2, 7, 2689, 2689, false);
        self.fill(world, clip, 0, 1, 0, 0, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 7, 1, 0, 7, 1, 7, 2689, 2689, false);
        self.fill(world, clip, 0, 1, 7, 7, 3, 7, 2689, 2689, false);
        self.fill(world, clip, 1, 1, 0, 2, 3, 0, 2689, 2689, false);
        self.fill(world, clip, 5, 1, 0, 6, 3, 0, 2689, 2689, false);
        if (base.open[2]) {
            self.water(world, clip, 3, 1, 7, 4, 2, 7, false);
        }
        if (base.open[4]) {
            self.water(world, clip, 0, 1, 3, 1, 2, 4, false);
        }
        if (base.open[5]) {
            self.water(world, clip, 6, 1, 3, 7, 2, 4, false);
        }
    }
    pub(super) fn template_monumentbuilding(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
        rooms: &[Option<Room>],
    ) {
        let mut n: i32;
        let mut n2 = world.sea_level().max(64) - self.bounds.min[1];
        self.water(world, clip, 0, 0, 0, 58, n2, 58, false);
        self.func_175840_a(false, 0, world, random, clip);
        self.func_175840_a(true, 33, world, random, clip);
        self.func_175839_b(world, random, clip);
        self.func_175837_c(world, random, clip);
        self.func_175841_d(world, random, clip);
        self.func_175835_e(world, random, clip);
        self.func_175842_f(world, random, clip);
        self.func_175838_g(world, random, clip);
        for n in (0..7) {
            let mut n3 = 0;
            while (n3 < 7) {
                if (n3 == 0 && n == 3) {
                    n3 = 6;
                }
                let mut n4 = n * 9;
                let mut n5 = n3 * 9;
                for i in (0..4) {
                    for j in (0..4) {
                        self.place(world, 2689, n4 + i, 0, n5 + j, clip);
                        self.down(world, 2689, n4 + i, -1, n5 + j, clip);
                    }
                }
                if (n == 0 || n == 6) {
                    n3 += 1;
                    continue;
                }
                n3 += 6;
            }
        }
        for n in (0..5) {
            self.water(
                world,
                clip,
                -1 - n,
                n * 2,
                -1 - n,
                -1 - n,
                23,
                58 + n,
                false,
            );
            self.water(
                world,
                clip,
                58 + n,
                n * 2,
                -1 - n,
                58 + n,
                23,
                58 + n,
                false,
            );
            self.water(world, clip, 0 - n, n * 2, -1 - n, 57 + n, 23, -1 - n, false);
            self.water(world, clip, 0 - n, n * 2, 58 + n, 57 + n, 23, 58 + n, false);
        }
    }
    fn func_175840_a(
        &self,
        bl: bool,
        n: i32,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        let mut n2 = 24;
        if (self.intersects(clip, n, 0, n + 23, 20)) {
            let mut n3: i32;
            let mut n4: i32;
            self.fill(world, clip, n, 0, 0, n + 24, 0, 20, 2688, 2688, false);
            self.water(world, clip, n, 1, 0, n + 24, 10, 20, false);
            for n4 in (0..4) {
                self.fill(
                    world,
                    clip,
                    n + n4,
                    n4 + 1,
                    n4,
                    n + n4,
                    n4 + 1,
                    20,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    n + n4 + 7,
                    n4 + 5,
                    n4 + 7,
                    n + n4 + 7,
                    n4 + 5,
                    20,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    n + 17 - n4,
                    n4 + 5,
                    n4 + 7,
                    n + 17 - n4,
                    n4 + 5,
                    20,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    n + 24 - n4,
                    n4 + 1,
                    n4,
                    n + 24 - n4,
                    n4 + 1,
                    20,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    n + n4 + 1,
                    n4 + 1,
                    n4,
                    n + 23 - n4,
                    n4 + 1,
                    n4,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    n + n4 + 8,
                    n4 + 5,
                    n4 + 7,
                    n + 16 - n4,
                    n4 + 5,
                    n4 + 7,
                    2689,
                    2689,
                    false,
                );
            }
            self.fill(world, clip, n + 4, 4, 4, n + 6, 4, 20, 2688, 2688, false);
            self.fill(world, clip, n + 7, 4, 4, n + 17, 4, 6, 2688, 2688, false);
            self.fill(world, clip, n + 18, 4, 4, n + 20, 4, 20, 2688, 2688, false);
            self.fill(world, clip, n + 11, 8, 11, n + 13, 8, 20, 2688, 2688, false);
            self.place(world, 2689, n + 12, 9, 12, clip);
            self.place(world, 2689, n + 12, 9, 15, clip);
            self.place(world, 2689, n + 12, 9, 18, clip);
            n4 = if bl { n + 19 } else { n + 5 };
            let mut n5 = if bl { n + 5 } else { n + 19 };
            for n3 in (5..=20).rev().step_by((3) as usize) {
                self.place(world, 2689, n4, 5, n3, clip);
            }
            for n3 in (7..=19).rev().step_by((3) as usize) {
                self.place(world, 2689, n5, 5, n3, clip);
            }
            for n3 in (0..4) {
                let mut n6 = if bl {
                    n + (24 - (17 - n3 * 3))
                } else {
                    n + 17 - n3 * 3
                };
                self.place(world, 2689, n6, 5, 5, clip);
            }
            self.place(world, 2689, n5, 5, 5, clip);
            self.fill(world, clip, n + 11, 1, 12, n + 13, 7, 12, 2688, 2688, false);
            self.fill(world, clip, n + 12, 1, 11, n + 12, 7, 13, 2688, 2688, false);
        }
    }
    fn func_175839_b(
        &self,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        if (self.intersects(clip, 22, 5, 35, 17)) {
            self.water(world, clip, 25, 0, 0, 32, 8, 20, false);
            for i in (0..4) {
                self.fill(
                    world,
                    clip,
                    24,
                    2,
                    5 + i * 4,
                    24,
                    4,
                    5 + i * 4,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    22,
                    4,
                    5 + i * 4,
                    23,
                    4,
                    5 + i * 4,
                    2689,
                    2689,
                    false,
                );
                self.place(world, 2689, 25, 5, 5 + i * 4, clip);
                self.place(world, 2689, 26, 6, 5 + i * 4, clip);
                self.place(world, 2704, 26, 5, 5 + i * 4, clip);
                self.fill(
                    world,
                    clip,
                    33,
                    2,
                    5 + i * 4,
                    33,
                    4,
                    5 + i * 4,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    34,
                    4,
                    5 + i * 4,
                    35,
                    4,
                    5 + i * 4,
                    2689,
                    2689,
                    false,
                );
                self.place(world, 2689, 32, 5, 5 + i * 4, clip);
                self.place(world, 2689, 31, 6, 5 + i * 4, clip);
                self.place(world, 2704, 31, 5, 5 + i * 4, clip);
                self.fill(
                    world,
                    clip,
                    27,
                    6,
                    5 + i * 4,
                    30,
                    6,
                    5 + i * 4,
                    2688,
                    2688,
                    false,
                );
            }
        }
    }
    fn func_175837_c(
        &self,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        if (self.intersects(clip, 15, 20, 42, 21)) {
            let mut n: i32;
            self.fill(world, clip, 15, 0, 21, 42, 0, 21, 2688, 2688, false);
            self.water(world, clip, 26, 1, 21, 31, 3, 21, false);
            self.fill(world, clip, 21, 12, 21, 36, 12, 21, 2688, 2688, false);
            self.fill(world, clip, 17, 11, 21, 40, 11, 21, 2688, 2688, false);
            self.fill(world, clip, 16, 10, 21, 41, 10, 21, 2688, 2688, false);
            self.fill(world, clip, 15, 7, 21, 42, 9, 21, 2688, 2688, false);
            self.fill(world, clip, 16, 6, 21, 41, 6, 21, 2688, 2688, false);
            self.fill(world, clip, 17, 5, 21, 40, 5, 21, 2688, 2688, false);
            self.fill(world, clip, 21, 4, 21, 36, 4, 21, 2688, 2688, false);
            self.fill(world, clip, 22, 3, 21, 26, 3, 21, 2688, 2688, false);
            self.fill(world, clip, 31, 3, 21, 35, 3, 21, 2688, 2688, false);
            self.fill(world, clip, 23, 2, 21, 25, 2, 21, 2688, 2688, false);
            self.fill(world, clip, 32, 2, 21, 34, 2, 21, 2688, 2688, false);
            self.fill(world, clip, 28, 4, 20, 29, 4, 21, 2689, 2689, false);
            self.place(world, 2689, 27, 3, 21, clip);
            self.place(world, 2689, 30, 3, 21, clip);
            self.place(world, 2689, 26, 2, 21, clip);
            self.place(world, 2689, 31, 2, 21, clip);
            self.place(world, 2689, 25, 1, 21, clip);
            self.place(world, 2689, 32, 1, 21, clip);
            for n in (0..7) {
                self.place(world, 2690, 28 - n, 6 + n, 21, clip);
                self.place(world, 2690, 29 + n, 6 + n, 21, clip);
            }
            for n in (0..4) {
                self.place(world, 2690, 28 - n, 9 + n, 21, clip);
                self.place(world, 2690, 29 + n, 9 + n, 21, clip);
            }
            self.place(world, 2690, 28, 12, 21, clip);
            self.place(world, 2690, 29, 12, 21, clip);
            for n in (0..3) {
                self.place(world, 2690, 22 - n * 2, 8, 21, clip);
                self.place(world, 2690, 22 - n * 2, 9, 21, clip);
                self.place(world, 2690, 35 + n * 2, 8, 21, clip);
                self.place(world, 2690, 35 + n * 2, 9, 21, clip);
            }
            self.water(world, clip, 15, 13, 21, 42, 15, 21, false);
            self.water(world, clip, 15, 1, 21, 15, 6, 21, false);
            self.water(world, clip, 16, 1, 21, 16, 5, 21, false);
            self.water(world, clip, 17, 1, 21, 20, 4, 21, false);
            self.water(world, clip, 21, 1, 21, 21, 3, 21, false);
            self.water(world, clip, 22, 1, 21, 22, 2, 21, false);
            self.water(world, clip, 23, 1, 21, 24, 1, 21, false);
            self.water(world, clip, 42, 1, 21, 42, 6, 21, false);
            self.water(world, clip, 41, 1, 21, 41, 5, 21, false);
            self.water(world, clip, 37, 1, 21, 40, 4, 21, false);
            self.water(world, clip, 36, 1, 21, 36, 3, 21, false);
            self.water(world, clip, 33, 1, 21, 34, 1, 21, false);
            self.water(world, clip, 35, 1, 21, 35, 2, 21, false);
        }
    }
    fn func_175841_d(
        &self,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        if (self.intersects(clip, 21, 21, 36, 36)) {
            self.fill(world, clip, 21, 0, 22, 36, 0, 36, 2688, 2688, false);
            self.water(world, clip, 21, 1, 22, 36, 23, 36, false);
            for i in (0..4) {
                self.fill(
                    world,
                    clip,
                    21 + i,
                    13 + i,
                    21 + i,
                    36 - i,
                    13 + i,
                    21 + i,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    21 + i,
                    13 + i,
                    36 - i,
                    36 - i,
                    13 + i,
                    36 - i,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    21 + i,
                    13 + i,
                    22 + i,
                    21 + i,
                    13 + i,
                    35 - i,
                    2689,
                    2689,
                    false,
                );
                self.fill(
                    world,
                    clip,
                    36 - i,
                    13 + i,
                    22 + i,
                    36 - i,
                    13 + i,
                    35 - i,
                    2689,
                    2689,
                    false,
                );
            }
            self.fill(world, clip, 25, 16, 25, 32, 16, 32, 2688, 2688, false);
            self.fill(world, clip, 25, 17, 25, 25, 19, 25, 2689, 2689, false);
            self.fill(world, clip, 32, 17, 25, 32, 19, 25, 2689, 2689, false);
            self.fill(world, clip, 25, 17, 32, 25, 19, 32, 2689, 2689, false);
            self.fill(world, clip, 32, 17, 32, 32, 19, 32, 2689, 2689, false);
            self.place(world, 2689, 26, 20, 26, clip);
            self.place(world, 2689, 27, 21, 27, clip);
            self.place(world, 2704, 27, 20, 27, clip);
            self.place(world, 2689, 26, 20, 31, clip);
            self.place(world, 2689, 27, 21, 30, clip);
            self.place(world, 2704, 27, 20, 30, clip);
            self.place(world, 2689, 31, 20, 31, clip);
            self.place(world, 2689, 30, 21, 30, clip);
            self.place(world, 2704, 30, 20, 30, clip);
            self.place(world, 2689, 31, 20, 26, clip);
            self.place(world, 2689, 30, 21, 27, clip);
            self.place(world, 2704, 30, 20, 27, clip);
            self.fill(world, clip, 28, 21, 27, 29, 21, 27, 2688, 2688, false);
            self.fill(world, clip, 27, 21, 28, 27, 21, 29, 2688, 2688, false);
            self.fill(world, clip, 28, 21, 30, 29, 21, 30, 2688, 2688, false);
            self.fill(world, clip, 30, 21, 28, 30, 21, 29, 2688, 2688, false);
        }
    }
    fn func_175835_e(
        &self,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        let mut n: i32;
        if (self.intersects(clip, 0, 21, 6, 58)) {
            self.fill(world, clip, 0, 0, 21, 6, 0, 57, 2688, 2688, false);
            self.water(world, clip, 0, 1, 21, 6, 7, 57, false);
            self.fill(world, clip, 4, 4, 21, 6, 4, 53, 2688, 2688, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    n,
                    n + 1,
                    21,
                    n,
                    n + 1,
                    57 - n,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (23..53).step_by((3) as usize) {
                self.place(world, 2689, 5, 5, n, clip);
            }
            self.place(world, 2689, 5, 5, 52, clip);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    n,
                    n + 1,
                    21,
                    n,
                    n + 1,
                    57 - n,
                    2689,
                    2689,
                    false,
                );
            }
            self.fill(world, clip, 4, 1, 52, 6, 3, 52, 2688, 2688, false);
            self.fill(world, clip, 5, 1, 51, 5, 3, 53, 2688, 2688, false);
        }
        if (self.intersects(clip, 51, 21, 58, 58)) {
            self.fill(world, clip, 51, 0, 21, 57, 0, 57, 2688, 2688, false);
            self.water(world, clip, 51, 1, 21, 57, 7, 57, false);
            self.fill(world, clip, 51, 4, 21, 53, 4, 53, 2688, 2688, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    57 - n,
                    n + 1,
                    21,
                    57 - n,
                    n + 1,
                    57 - n,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (23..53).step_by((3) as usize) {
                self.place(world, 2689, 52, 5, n, clip);
            }
            self.place(world, 2689, 52, 5, 52, clip);
            self.fill(world, clip, 51, 1, 52, 53, 3, 52, 2688, 2688, false);
            self.fill(world, clip, 52, 1, 51, 52, 3, 53, 2688, 2688, false);
        }
        if (self.intersects(clip, 0, 51, 57, 57)) {
            self.fill(world, clip, 7, 0, 51, 50, 0, 57, 2688, 2688, false);
            self.water(world, clip, 7, 1, 51, 50, 10, 57, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    n + 1,
                    n + 1,
                    57 - n,
                    56 - n,
                    n + 1,
                    57 - n,
                    2689,
                    2689,
                    false,
                );
            }
        }
    }
    fn func_175842_f(
        &self,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        let mut n: i32;
        if (self.intersects(clip, 7, 21, 13, 50)) {
            self.fill(world, clip, 7, 0, 21, 13, 0, 50, 2688, 2688, false);
            self.water(world, clip, 7, 1, 21, 13, 10, 50, false);
            self.fill(world, clip, 11, 8, 21, 13, 8, 53, 2688, 2688, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    n + 7,
                    n + 5,
                    21,
                    n + 7,
                    n + 5,
                    54,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (21..=45).step_by((3) as usize) {
                self.place(world, 2689, 12, 9, n, clip);
            }
        }
        if (self.intersects(clip, 44, 21, 50, 54)) {
            self.fill(world, clip, 44, 0, 21, 50, 0, 50, 2688, 2688, false);
            self.water(world, clip, 44, 1, 21, 50, 10, 50, false);
            self.fill(world, clip, 44, 8, 21, 46, 8, 53, 2688, 2688, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    50 - n,
                    n + 5,
                    21,
                    50 - n,
                    n + 5,
                    54,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (21..=45).step_by((3) as usize) {
                self.place(world, 2689, 45, 9, n, clip);
            }
        }
        if (self.intersects(clip, 8, 44, 49, 54)) {
            self.fill(world, clip, 14, 0, 44, 43, 0, 50, 2688, 2688, false);
            self.water(world, clip, 14, 1, 44, 43, 10, 50, false);
            for n in (12..=45).step_by((3) as usize) {
                self.place(world, 2689, n, 9, 45, clip);
                self.place(world, 2689, n, 9, 52, clip);
                if n != 12 && n != 18 && n != 24 && n != 33 && n != 39 && n != 45 {
                    continue;
                }
                self.place(world, 2689, n, 9, 47, clip);
                self.place(world, 2689, n, 9, 50, clip);
                self.place(world, 2689, n, 10, 45, clip);
                self.place(world, 2689, n, 10, 46, clip);
                self.place(world, 2689, n, 10, 51, clip);
                self.place(world, 2689, n, 10, 52, clip);
                self.place(world, 2689, n, 11, 47, clip);
                self.place(world, 2689, n, 11, 50, clip);
                self.place(world, 2689, n, 12, 48, clip);
                self.place(world, 2689, n, 12, 49, clip);
            }
            for n in (0..3) {
                self.fill(
                    world,
                    clip,
                    8 + n,
                    5 + n,
                    54,
                    49 - n,
                    5 + n,
                    54,
                    2688,
                    2688,
                    false,
                );
            }
            self.fill(world, clip, 11, 8, 54, 46, 8, 54, 2689, 2689, false);
            self.fill(world, clip, 14, 8, 44, 43, 8, 53, 2688, 2688, false);
        }
    }
    fn func_175838_g(
        &self,
        world: &mut impl MonumentWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
    ) {
        let mut n: i32;
        if (self.intersects(clip, 14, 21, 20, 43)) {
            self.fill(world, clip, 14, 0, 21, 20, 0, 43, 2688, 2688, false);
            self.water(world, clip, 14, 1, 22, 20, 14, 43, false);
            self.fill(world, clip, 18, 12, 22, 20, 12, 39, 2688, 2688, false);
            self.fill(world, clip, 18, 12, 21, 20, 12, 21, 2689, 2689, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    n + 14,
                    n + 9,
                    21,
                    n + 14,
                    n + 9,
                    43 - n,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (23..=39).step_by((3) as usize) {
                self.place(world, 2689, 19, 13, n, clip);
            }
        }
        if (self.intersects(clip, 37, 21, 43, 43)) {
            self.fill(world, clip, 37, 0, 21, 43, 0, 43, 2688, 2688, false);
            self.water(world, clip, 37, 1, 22, 43, 14, 43, false);
            self.fill(world, clip, 37, 12, 22, 39, 12, 39, 2688, 2688, false);
            self.fill(world, clip, 37, 12, 21, 39, 12, 21, 2689, 2689, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    43 - n,
                    n + 9,
                    21,
                    43 - n,
                    n + 9,
                    43 - n,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (23..=39).step_by((3) as usize) {
                self.place(world, 2689, 38, 13, n, clip);
            }
        }
        if (self.intersects(clip, 15, 37, 42, 43)) {
            self.fill(world, clip, 21, 0, 37, 36, 0, 43, 2688, 2688, false);
            self.water(world, clip, 21, 1, 37, 36, 14, 43, false);
            self.fill(world, clip, 21, 12, 37, 36, 12, 39, 2688, 2688, false);
            for n in (0..4) {
                self.fill(
                    world,
                    clip,
                    15 + n,
                    n + 9,
                    43 - n,
                    42 - n,
                    n + 9,
                    43 - n,
                    2689,
                    2689,
                    false,
                );
            }
            for n in (21..=36).step_by((3) as usize) {
                self.place(world, 2689, n, 13, 38, clip);
            }
        }
    }
    pub(super) fn floor(
        &self,
        world: &mut impl MonumentWorld,
        clip: BoundingBox,
        n: i32,
        n2: i32,
        bl: bool,
    ) {
        if (bl) {
            self.fill(
                world,
                clip,
                n,
                0,
                n2,
                n + 2,
                0,
                n2 + 8 - 1,
                2688,
                2688,
                false,
            );
            self.fill(
                world,
                clip,
                n + 5,
                0,
                n2,
                n + 8 - 1,
                0,
                n2 + 8 - 1,
                2688,
                2688,
                false,
            );
            self.fill(
                world,
                clip,
                n + 3,
                0,
                n2,
                n + 4,
                0,
                n2 + 2,
                2688,
                2688,
                false,
            );
            self.fill(
                world,
                clip,
                n + 3,
                0,
                n2 + 5,
                n + 4,
                0,
                n2 + 8 - 1,
                2688,
                2688,
                false,
            );
            self.fill(
                world,
                clip,
                n + 3,
                0,
                n2 + 2,
                n + 4,
                0,
                n2 + 2,
                2689,
                2689,
                false,
            );
            self.fill(
                world,
                clip,
                n + 3,
                0,
                n2 + 5,
                n + 4,
                0,
                n2 + 5,
                2689,
                2689,
                false,
            );
            self.fill(
                world,
                clip,
                n + 2,
                0,
                n2 + 3,
                n + 2,
                0,
                n2 + 4,
                2689,
                2689,
                false,
            );
            self.fill(
                world,
                clip,
                n + 5,
                0,
                n2 + 3,
                n + 5,
                0,
                n2 + 4,
                2689,
                2689,
                false,
            );
        } else {
            self.fill(
                world,
                clip,
                n,
                0,
                n2,
                n + 8 - 1,
                0,
                n2 + 8 - 1,
                2688,
                2688,
                false,
            );
        }
    }
}
