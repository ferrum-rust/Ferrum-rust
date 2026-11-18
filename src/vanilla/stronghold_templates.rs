//! Original stronghold block template transcription.
#![allow(
    unused_variables,
    unused_mut,
    unused_assignments,
    unused_parens,
    clippy::double_parens,
    clippy::unnecessary_cast,
    clippy::too_many_arguments
)]
use super::*;
impl Piece {
    pub(super) fn template_portalroom(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        self.stones(world, clip, 0, 0, 0, 10, 7, 15, false, random);
        self.door(world, random, clip, Door::Grates, 4, 1, 0);
        let mut n3 = 6;
        self.stones(world, clip, 1, n3, 1, 1, n3, 14, false, random);
        self.stones(world, clip, 9, n3, 1, 9, n3, 14, false, random);
        self.stones(world, clip, 2, n3, 1, 8, n3, 2, false, random);
        self.stones(world, clip, 2, n3, 14, 8, n3, 14, false, random);
        self.stones(world, clip, 1, 1, 1, 2, 1, 4, false, random);
        self.stones(world, clip, 8, 1, 1, 9, 1, 4, false, random);
        self.fill(world, clip, 1, 1, 1, 1, 1, 3, 160, 160, false);
        self.fill(world, clip, 9, 1, 1, 9, 1, 3, 160, 160, false);
        self.stones(world, clip, 3, 1, 8, 7, 1, 12, false, random);
        self.fill(world, clip, 4, 1, 9, 6, 1, 11, 160, 160, false);
        for n2 in (3..14).step_by((2) as usize) {
            self.fill(world, clip, 0, 3, n2, 0, 4, n2, 1616, 1616, false);
            self.fill(world, clip, 10, 3, n2, 10, 4, n2, 1616, 1616, false);
        }
        for n2 in (2..9).step_by((2) as usize) {
            self.fill(world, clip, n2, 3, 15, n2, 4, 15, 1616, 1616, false);
        }
        n2 = i32::from(rotate_metadata(109, (3) as u8, self.orientation));
        self.stones(world, clip, 4, 1, 5, 6, 1, 7, false, random);
        self.stones(world, clip, 4, 2, 6, 6, 2, 7, false, random);
        self.stones(world, clip, 4, 3, 7, 6, 3, 7, false, random);
        for n in (4..=6) {
            self.place(world, ((109 << 4) | (n2) as u16), n, 1, 4, clip);
            self.place(world, ((109 << 4) | (n2) as u16), n, 2, 5, clip);
            self.place(world, ((109 << 4) | (n2) as u16), n, 3, 6, clip);
        }
        let (n, n4, n5, n6) = match self.orientation {
            0 => (0, 2, 3, 1),
            3 => (3, 1, 0, 2),
            1 => (1, 3, 0, 2),
            _ => (2, 0, 3, 1),
        };
        self.place(
            world,
            ((120 << 4) | (n) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            4,
            3,
            8,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            5,
            3,
            8,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            6,
            3,
            8,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n4) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            4,
            3,
            12,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n4) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            5,
            3,
            12,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n4) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            6,
            3,
            12,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n5) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            3,
            3,
            9,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n5) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            3,
            3,
            10,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n5) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            3,
            3,
            11,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n6) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            7,
            3,
            9,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n6) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            7,
            3,
            10,
            clip,
        );
        self.place(
            world,
            ((120 << 4) | (n6) as u16) | if random.next_float() > 0.9 { 4 } else { 0 },
            7,
            3,
            11,
            clip,
        );
        self.spawn_silverfish(world, clip);
    }
    pub(super) fn template_crossing(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 9, 8, 10, true, random);
        self.door(world, random, clip, self.door, 4, 3, 0);
        if (self.exits[0]) {
            self.fill(world, clip, 0, 3, 1, 0, 5, 3, 0, 0, false);
        }
        if (self.exits[2]) {
            self.fill(world, clip, 9, 3, 1, 9, 5, 3, 0, 0, false);
        }
        if (self.exits[1]) {
            self.fill(world, clip, 0, 5, 7, 0, 7, 9, 0, 0, false);
        }
        if (self.exits[3]) {
            self.fill(world, clip, 9, 5, 7, 9, 7, 9, 0, 0, false);
        }
        self.fill(world, clip, 5, 1, 10, 7, 3, 10, 0, 0, false);
        self.stones(world, clip, 1, 2, 1, 8, 2, 6, false, random);
        self.stones(world, clip, 4, 1, 5, 4, 4, 9, false, random);
        self.stones(world, clip, 8, 1, 5, 8, 4, 9, false, random);
        self.stones(world, clip, 1, 4, 7, 3, 4, 9, false, random);
        self.stones(world, clip, 1, 3, 5, 3, 3, 6, false, random);
        self.fill(world, clip, 1, 3, 4, 3, 3, 4, 704, 704, false);
        self.fill(world, clip, 1, 4, 6, 3, 4, 6, 704, 704, false);
        self.stones(world, clip, 5, 1, 7, 7, 1, 8, false, random);
        self.fill(world, clip, 5, 1, 9, 7, 1, 9, 704, 704, false);
        self.fill(world, clip, 5, 2, 7, 7, 2, 7, 704, 704, false);
        self.fill(world, clip, 4, 5, 7, 4, 5, 9, 704, 704, false);
        self.fill(world, clip, 8, 5, 7, 8, 5, 9, 704, 704, false);
        self.fill(world, clip, 5, 5, 7, 7, 5, 9, 688, 688, false);
        self.place(world, 805, 6, 5, 6, clip);
    }
    pub(super) fn template_library(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2 = 11;
        if (!self.large) {
            n2 = 6;
        }
        self.stones(world, clip, 0, 0, 0, 13, n2 - 1, 14, true, random);
        self.door(world, random, clip, self.door, 4, 1, 0);
        self.random_fill(
            world, clip, random, 0.07, 2, 1, 1, 11, 4, 13, 480, 480, false,
        );
        let mut bl = true;
        let mut n3 = 12;
        for n in (1..=13) {
            if ((n - 1) % 4 == 0) {
                self.fill(world, clip, 1, 1, n, 1, 4, n, 80, 80, false);
                self.fill(world, clip, 12, 1, n, 12, 4, n, 80, 80, false);
                self.place(world, 805, 2, 3, n, clip);
                self.place(world, 805, 11, 3, n, clip);
                if !self.large {
                    continue;
                }
                self.fill(world, clip, 1, 6, n, 1, 9, n, 80, 80, false);
                self.fill(world, clip, 12, 6, n, 12, 9, n, 80, 80, false);
                continue;
            }
            self.fill(world, clip, 1, 1, n, 1, 4, n, 752, 752, false);
            self.fill(world, clip, 12, 1, n, 12, 4, n, 752, 752, false);
            if !self.large {
                continue;
            }
            self.fill(world, clip, 1, 6, n, 1, 9, n, 752, 752, false);
            self.fill(world, clip, 12, 6, n, 12, 9, n, 752, 752, false);
        }
        for n in (3..12).step_by((2) as usize) {
            self.fill(world, clip, 3, 1, n, 4, 3, n, 752, 752, false);
            self.fill(world, clip, 6, 1, n, 7, 3, n, 752, 752, false);
            self.fill(world, clip, 9, 1, n, 10, 3, n, 752, 752, false);
        }
        if (self.large) {
            self.fill(world, clip, 1, 5, 1, 3, 5, 13, 80, 80, false);
            self.fill(world, clip, 10, 5, 1, 12, 5, 13, 80, 80, false);
            self.fill(world, clip, 4, 5, 1, 9, 5, 2, 80, 80, false);
            self.fill(world, clip, 4, 5, 12, 9, 5, 13, 80, 80, false);
            self.place(world, 80, 9, 5, 11, clip);
            self.place(world, 80, 8, 5, 11, clip);
            self.place(world, 80, 9, 5, 10, clip);
            self.fill(world, clip, 3, 6, 2, 3, 6, 12, 1360, 1360, false);
            self.fill(world, clip, 10, 6, 2, 10, 6, 10, 1360, 1360, false);
            self.fill(world, clip, 4, 6, 2, 9, 6, 2, 1360, 1360, false);
            self.fill(world, clip, 4, 6, 12, 8, 6, 12, 1360, 1360, false);
            self.place(world, 1360, 9, 6, 11, clip);
            self.place(world, 1360, 8, 6, 11, clip);
            self.place(world, 1360, 9, 6, 10, clip);
            n = i32::from(rotate_metadata(65, (3) as u8, self.orientation));
            self.place(world, ((65 << 4) | (n) as u16), 10, 1, 13, clip);
            self.place(world, ((65 << 4) | (n) as u16), 10, 2, 13, clip);
            self.place(world, ((65 << 4) | (n) as u16), 10, 3, 13, clip);
            self.place(world, ((65 << 4) | (n) as u16), 10, 4, 13, clip);
            self.place(world, ((65 << 4) | (n) as u16), 10, 5, 13, clip);
            self.place(world, ((65 << 4) | (n) as u16), 10, 6, 13, clip);
            self.place(world, ((65 << 4) | (n) as u16), 10, 7, 13, clip);
            let mut n4 = 7;
            let mut n5 = 7;
            self.place(world, 1360, n4 - 1, 9, n5, clip);
            self.place(world, 1360, n4, 9, n5, clip);
            self.place(world, 1360, n4 - 1, 8, n5, clip);
            self.place(world, 1360, n4, 8, n5, clip);
            self.place(world, 1360, n4 - 1, 7, n5, clip);
            self.place(world, 1360, n4, 7, n5, clip);
            self.place(world, 1360, n4 - 2, 7, n5, clip);
            self.place(world, 1360, n4 + 1, 7, n5, clip);
            self.place(world, 1360, n4 - 1, 7, n5 - 1, clip);
            self.place(world, 1360, n4 - 1, 7, n5 + 1, clip);
            self.place(world, 1360, n4, 7, n5 - 1, clip);
            self.place(world, 1360, n4, 7, n5 + 1, clip);
            self.place(world, 805, n4 - 2, 8, n5, clip);
            self.place(world, 805, n4 + 1, 8, n5, clip);
            self.place(world, 805, n4 - 1, 8, n5 - 1, clip);
            self.place(world, 805, n4 - 1, 8, n5 + 1, clip);
            self.place(world, 805, n4, 8, n5 - 1, clip);
            self.place(world, 805, n4, 8, n5 + 1, clip);
        }
        self.chest(world, clip, random, [3, 3, 5], 1);
        if (self.large) {
            self.place(world, 0, 12, 9, 1, clip);
            self.chest(world, clip, random, [12, 8, 1], 1);
        }
    }
    pub(super) fn template_prison(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 8, 4, 10, true, random);
        self.door(world, random, clip, self.door, 1, 1, 0);
        self.fill(world, clip, 1, 1, 10, 3, 3, 10, 0, 0, false);
        self.stones(world, clip, 4, 1, 1, 4, 3, 1, false, random);
        self.stones(world, clip, 4, 1, 3, 4, 3, 3, false, random);
        self.stones(world, clip, 4, 1, 7, 4, 3, 7, false, random);
        self.stones(world, clip, 4, 1, 9, 4, 3, 9, false, random);
        self.fill(world, clip, 4, 1, 4, 4, 3, 6, 1616, 1616, false);
        self.fill(world, clip, 5, 1, 5, 7, 3, 5, 1616, 1616, false);
        self.place(world, 1616, 4, 3, 2, clip);
        self.place(world, 1616, 4, 3, 8, clip);
        self.place(
            world,
            ((71 << 4) | (i32::from(rotate_metadata(71, (3) as u8, self.orientation))) as u16),
            4,
            1,
            2,
            clip,
        );
        self.place(
            world,
            ((71 << 4) | (i32::from(rotate_metadata(71, (3) as u8, self.orientation)) + 8) as u16),
            4,
            2,
            2,
            clip,
        );
        self.place(
            world,
            ((71 << 4) | (i32::from(rotate_metadata(71, (3) as u8, self.orientation))) as u16),
            4,
            1,
            8,
            clip,
        );
        self.place(
            world,
            ((71 << 4) | (i32::from(rotate_metadata(71, (3) as u8, self.orientation)) + 8) as u16),
            4,
            2,
            8,
            clip,
        );
    }
    pub(super) fn template_roomcrossing(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 10, 6, 10, true, random);
        self.door(world, random, clip, self.door, 4, 1, 0);
        self.fill(world, clip, 4, 1, 10, 6, 3, 10, 0, 0, false);
        self.fill(world, clip, 0, 1, 4, 0, 3, 6, 0, 0, false);
        self.fill(world, clip, 10, 1, 4, 10, 3, 6, 0, 0, false);
        match self.room_type {
            0 => {
                self.place(world, 1568, 5, 1, 5, clip);
                self.place(world, 1568, 5, 2, 5, clip);
                self.place(world, 1568, 5, 3, 5, clip);
                self.place(world, 805, 4, 3, 5, clip);
                self.place(world, 805, 6, 3, 5, clip);
                self.place(world, 805, 5, 3, 4, clip);
                self.place(world, 805, 5, 3, 6, clip);
                self.place(world, 704, 4, 1, 4, clip);
                self.place(world, 704, 4, 1, 5, clip);
                self.place(world, 704, 4, 1, 6, clip);
                self.place(world, 704, 6, 1, 4, clip);
                self.place(world, 704, 6, 1, 5, clip);
                self.place(world, 704, 6, 1, 6, clip);
                self.place(world, 704, 5, 1, 4, clip);
                self.place(world, 704, 5, 1, 6, clip);
            }
            1 => {
                for i in (0..5) {
                    self.place(world, 1568, 3, 1, 3 + i, clip);
                    self.place(world, 1568, 7, 1, 3 + i, clip);
                    self.place(world, 1568, 3 + i, 1, 3, clip);
                    self.place(world, 1568, 3 + i, 1, 7, clip);
                }
                self.place(world, 1568, 5, 1, 5, clip);
                self.place(world, 1568, 5, 2, 5, clip);
                self.place(world, 1568, 5, 3, 5, clip);
                self.place(world, 128, 5, 4, 5, clip);
            }
            2 => {
                let mut n: i32;
                for n in (1..=9) {
                    self.place(world, 64, 1, 3, n, clip);
                    self.place(world, 64, 9, 3, n, clip);
                }
                for n in (1..=9) {
                    self.place(world, 64, n, 3, 1, clip);
                    self.place(world, 64, n, 3, 9, clip);
                }
                self.place(world, 64, 5, 1, 4, clip);
                self.place(world, 64, 5, 1, 6, clip);
                self.place(world, 64, 5, 3, 4, clip);
                self.place(world, 64, 5, 3, 6, clip);
                self.place(world, 64, 4, 1, 5, clip);
                self.place(world, 64, 6, 1, 5, clip);
                self.place(world, 64, 4, 3, 5, clip);
                self.place(world, 64, 6, 3, 5, clip);
                for n in (1..=3) {
                    self.place(world, 64, 4, n, 4, clip);
                    self.place(world, 64, 6, n, 4, clip);
                    self.place(world, 64, 4, n, 6, clip);
                    self.place(world, 64, 6, n, 6, clip);
                }
                self.place(world, 805, 5, 3, 5, clip);
                for n in (2..=8) {
                    self.place(world, 80, 2, 3, n, clip);
                    self.place(world, 80, 3, 3, n, clip);
                    if (n <= 3 || n >= 7) {
                        self.place(world, 80, 4, 3, n, clip);
                        self.place(world, 80, 5, 3, n, clip);
                        self.place(world, 80, 6, 3, n, clip);
                    }
                    self.place(world, 80, 7, 3, n, clip);
                    self.place(world, 80, 8, 3, n, clip);
                }
                self.place(
                    world,
                    ((65 << 4)
                        | (i32::from(rotate_metadata(65, (4) as u8, self.orientation))) as u16),
                    9,
                    1,
                    3,
                    clip,
                );
                self.place(
                    world,
                    ((65 << 4)
                        | (i32::from(rotate_metadata(65, (4) as u8, self.orientation))) as u16),
                    9,
                    2,
                    3,
                    clip,
                );
                self.place(
                    world,
                    ((65 << 4)
                        | (i32::from(rotate_metadata(65, (4) as u8, self.orientation))) as u16),
                    9,
                    3,
                    3,
                    clip,
                );
                self.chest(world, clip, random, [3, 4, 8], 2);
            }
            _ => {}
        }
    }
    pub(super) fn template_rightturn(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 4, 4, 4, true, random);
        self.door(world, random, clip, self.door, 1, 1, 0);
        if (self.orientation == 2 || self.orientation == 3) {
            self.fill(world, clip, 4, 1, 1, 4, 3, 3, 0, 0, false);
        } else {
            self.fill(world, clip, 0, 1, 1, 0, 3, 3, 0, 0, false);
        }
    }
    pub(super) fn template_leftturn(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 4, 4, 4, true, random);
        self.door(world, random, clip, self.door, 1, 1, 0);
        if (self.orientation == 2 || self.orientation == 3) {
            self.fill(world, clip, 0, 1, 1, 0, 3, 3, 0, 0, false);
        } else {
            self.fill(world, clip, 4, 1, 1, 4, 3, 3, 0, 0, false);
        }
    }
    pub(super) fn template_stairsstraight(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 4, 10, 7, true, random);
        self.door(world, random, clip, self.door, 1, 7, 0);
        self.door(world, random, clip, Door::Opening, 1, 1, 7);
        let mut n = i32::from(rotate_metadata(67, (2) as u8, self.orientation));
        for i in (0..6) {
            self.place(world, ((67 << 4) | (n) as u16), 1, 6 - i, 1 + i, clip);
            self.place(world, ((67 << 4) | (n) as u16), 2, 6 - i, 1 + i, clip);
            self.place(world, ((67 << 4) | (n) as u16), 3, 6 - i, 1 + i, clip);
            if i >= 5 {
                continue;
            }
            self.place(world, 1568, 1, 5 - i, 1 + i, clip);
            self.place(world, 1568, 2, 5 - i, 1 + i, clip);
            self.place(world, 1568, 3, 5 - i, 1 + i, clip);
        }
    }
    pub(super) fn template_chestcorridor(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 4, 4, 6, true, random);
        self.door(world, random, clip, self.door, 1, 1, 0);
        self.door(world, random, clip, Door::Opening, 1, 1, 6);
        self.fill(world, clip, 3, 1, 2, 3, 1, 4, 1568, 1568, false);
        self.place(world, ((44 << 4) | (5) as u16), 3, 1, 1, clip);
        self.place(world, ((44 << 4) | (5) as u16), 3, 1, 5, clip);
        self.place(world, ((44 << 4) | (5) as u16), 3, 2, 2, clip);
        self.place(world, ((44 << 4) | (5) as u16), 3, 2, 4, clip);
        for i in (2..=4) {
            self.place(world, ((44 << 4) | (5) as u16), 2, 1, i, clip);
        }
        if !self.chest_made && clip.contains(self.point(3, 2, 3)) {
            self.chest_made = true;
            self.chest(world, clip, random, [3, 2, 3], 0);
        }
    }
    pub(super) fn template_straight(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 4, 4, 6, true, random);
        self.door(world, random, clip, self.door, 1, 1, 0);
        self.door(world, random, clip, Door::Opening, 1, 1, 6);
        self.random_place(world, clip, random, 0.1, 1, 2, 1, 805);
        self.random_place(world, clip, random, 0.1, 3, 2, 1, 805);
        self.random_place(world, clip, random, 0.1, 1, 2, 5, 805);
        self.random_place(world, clip, random, 0.1, 3, 2, 5, 805);
        if (self.exits[0]) {
            self.fill(world, clip, 0, 1, 2, 0, 3, 4, 0, 0, false);
        }
        if (self.exits[1]) {
            self.fill(world, clip, 4, 1, 2, 4, 3, 4, 0, 0, false);
        }
    }
    pub(super) fn template_stairs(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.stones(world, clip, 0, 0, 0, 4, 10, 4, true, random);
        self.door(world, random, clip, self.door, 1, 7, 0);
        self.door(world, random, clip, Door::Opening, 1, 1, 4);
        self.place(world, 1568, 2, 6, 1, clip);
        self.place(world, 1568, 1, 5, 1, clip);
        self.place(world, ((44 << 4) | (0) as u16), 1, 6, 1, clip);
        self.place(world, 1568, 1, 5, 2, clip);
        self.place(world, 1568, 1, 4, 3, clip);
        self.place(world, ((44 << 4) | (0) as u16), 1, 5, 3, clip);
        self.place(world, 1568, 2, 4, 3, clip);
        self.place(world, 1568, 3, 3, 3, clip);
        self.place(world, ((44 << 4) | (0) as u16), 3, 4, 3, clip);
        self.place(world, 1568, 3, 3, 2, clip);
        self.place(world, 1568, 3, 2, 1, clip);
        self.place(world, ((44 << 4) | (0) as u16), 3, 3, 1, clip);
        self.place(world, 1568, 2, 2, 1, clip);
        self.place(world, 1568, 1, 1, 1, clip);
        self.place(world, ((44 << 4) | (0) as u16), 1, 2, 1, clip);
        self.place(world, 1568, 1, 1, 2, clip);
        self.place(world, ((44 << 4) | (0) as u16), 1, 1, 3, clip);
    }
    pub(super) fn template_corridor(
        &mut self,
        world: &mut impl StrongholdWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        for i in (0..self.steps) {
            self.place(world, 1568, 0, 0, i, clip);
            self.place(world, 1568, 1, 0, i, clip);
            self.place(world, 1568, 2, 0, i, clip);
            self.place(world, 1568, 3, 0, i, clip);
            self.place(world, 1568, 4, 0, i, clip);
            for j in (1..=3) {
                self.place(world, 1568, 0, j, i, clip);
                self.place(world, 0, 1, j, i, clip);
                self.place(world, 0, 2, j, i, clip);
                self.place(world, 0, 3, j, i, clip);
                self.place(world, 1568, 4, j, i, clip);
            }
            self.place(world, 1568, 0, 4, i, clip);
            self.place(world, 1568, 1, 4, i, clip);
            self.place(world, 1568, 2, 4, i, clip);
            self.place(world, 1568, 3, 4, i, clip);
            self.place(world, 1568, 4, 4, i, clip);
        }
    }
    fn door(
        &self,
        world: &mut impl StrongholdWorld,
        random: &mut JavaRandom,
        clip: BoundingBox,
        door: Door,
        n: i32,
        n2: i32,
        n3: i32,
    ) {
        match door {
            Door::Opening => {
                self.fill(
                    world,
                    clip,
                    n,
                    n2,
                    n3,
                    n + 3 - 1,
                    n2 + 3 - 1,
                    n3,
                    0,
                    0,
                    false,
                );
            }
            Door::Wood => {
                self.place(world, 1568, n, n2, n3, clip);
                self.place(world, 1568, n, n2 + 1, n3, clip);
                self.place(world, 1568, n, n2 + 2, n3, clip);
                self.place(world, 1568, n + 1, n2 + 2, n3, clip);
                self.place(world, 1568, n + 2, n2 + 2, n3, clip);
                self.place(world, 1568, n + 2, n2 + 1, n3, clip);
                self.place(world, 1568, n + 2, n2, n3, clip);
                self.place(world, 1027, n + 1, n2, n3, clip);
                self.place(world, ((64 << 4) | (8) as u16), n + 1, n2 + 1, n3, clip);
            }
            Door::Grates => {
                self.place(world, 0, n + 1, n2, n3, clip);
                self.place(world, 0, n + 1, n2 + 1, n3, clip);
                self.place(world, 1616, n, n2, n3, clip);
                self.place(world, 1616, n, n2 + 1, n3, clip);
                self.place(world, 1616, n, n2 + 2, n3, clip);
                self.place(world, 1616, n + 1, n2 + 2, n3, clip);
                self.place(world, 1616, n + 2, n2 + 2, n3, clip);
                self.place(world, 1616, n + 2, n2 + 1, n3, clip);
                self.place(world, 1616, n + 2, n2, n3, clip);
            }
            Door::Iron => {
                self.place(world, 1568, n, n2, n3, clip);
                self.place(world, 1568, n, n2 + 1, n3, clip);
                self.place(world, 1568, n, n2 + 2, n3, clip);
                self.place(world, 1568, n + 1, n2 + 2, n3, clip);
                self.place(world, 1568, n + 2, n2 + 2, n3, clip);
                self.place(world, 1568, n + 2, n2 + 1, n3, clip);
                self.place(world, 1568, n + 2, n2, n3, clip);
                self.place(world, 1139, n + 1, n2, n3, clip);
                self.place(world, ((71 << 4) | (8) as u16), n + 1, n2 + 1, n3, clip);
                self.place(
                    world,
                    ((77 << 4)
                        | (i32::from(rotate_metadata(77, (4) as u8, self.orientation))) as u16),
                    n + 2,
                    n2 + 1,
                    n3 + 1,
                    clip,
                );
                self.place(
                    world,
                    ((77 << 4)
                        | (i32::from(rotate_metadata(77, (3) as u8, self.orientation))) as u16),
                    n + 2,
                    n2 + 1,
                    n3 - 1,
                    clip,
                );
            }
        }
    }
}
