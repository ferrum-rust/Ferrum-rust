//! Direct native transcription of original village block templates.
//! Regenerate with scripts/port_village_templates.py after preparing local references.
#![allow(
    unused_variables,
    unused_mut,
    unused_assignments,
    unused_parens,
    clippy::too_many_arguments,
    clippy::double_parens,
    clippy::unnecessary_cast
)]
use super::*;
impl Piece {
    pub(super) fn template_torch(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 2, 3, 1, 0, 0, false);
        self.place(world, 1360, 1, 0, 0, clip);
        self.place(world, 1360, 1, 1, 0, clip);
        self.place(world, 1360, 1, 2, 0, clip);
        self.place(world, ((35 << 4) | (15) as u16), 1, 3, 0, clip);
        let bl = self.orientation == 3 || self.orientation == 2;
        self.place(
            world,
            torch(((self.orientation + 1) & 3)),
            if bl { 2 } else { 0 },
            3,
            0,
            clip,
        );
        self.place(world, torch(self.orientation), 1, 3, 1, clip);
        self.place(
            world,
            torch(((self.orientation + 3) & 3)),
            if bl { 0 } else { 2 },
            3,
            0,
            clip,
        );
        self.place(world, torch(((self.orientation + 2) & 3)), 1, 3, -1, clip);
    }

    pub(super) fn template_field1(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        self.fill(world, clip, 0, 1, 0, 12, 4, 8, 0, 0, false);
        self.fill(world, clip, 1, 0, 1, 2, 0, 7, 960, 960, false);
        self.fill(world, clip, 4, 0, 1, 5, 0, 7, 960, 960, false);
        self.fill(world, clip, 7, 0, 1, 8, 0, 7, 960, 960, false);
        self.fill(world, clip, 10, 0, 1, 11, 0, 7, 960, 960, false);
        self.fill(world, clip, 0, 0, 0, 0, 0, 8, 272, 272, false);
        self.fill(world, clip, 6, 0, 0, 6, 0, 8, 272, 272, false);
        self.fill(world, clip, 12, 0, 0, 12, 0, 8, 272, 272, false);
        self.fill(world, clip, 1, 0, 0, 11, 0, 0, 272, 272, false);
        self.fill(world, clip, 1, 0, 8, 11, 0, 8, 272, 272, false);
        self.fill(world, clip, 3, 0, 1, 3, 0, 7, 144, 144, false);
        self.fill(world, clip, 9, 0, 1, 9, 0, 7, 144, 144, false);
        for n in 1..=7 {
            self.place(
                world,
                ((self.crops[0] << 4) | (2 + random.next_int(6)) as u16),
                1,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[0] << 4) | (2 + random.next_int(6)) as u16),
                2,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[1] << 4) | (2 + random.next_int(6)) as u16),
                4,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[1] << 4) | (2 + random.next_int(6)) as u16),
                5,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[2] << 4) | (2 + random.next_int(6)) as u16),
                7,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[2] << 4) | (2 + random.next_int(6)) as u16),
                8,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[3] << 4) | (2 + random.next_int(6)) as u16),
                10,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[3] << 4) | (2 + random.next_int(6)) as u16),
                11,
                1,
                n,
                clip,
            );
        }
        for n in 0..9 {
            for i in 0..13 {
                self.clear(world, i, 4, n, clip);
                self.down(world, 48, i, -1, n, clip);
            }
        }
    }

    pub(super) fn template_field2(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        self.fill(world, clip, 0, 1, 0, 6, 4, 8, 0, 0, false);
        self.fill(world, clip, 1, 0, 1, 2, 0, 7, 960, 960, false);
        self.fill(world, clip, 4, 0, 1, 5, 0, 7, 960, 960, false);
        self.fill(world, clip, 0, 0, 0, 0, 0, 8, 272, 272, false);
        self.fill(world, clip, 6, 0, 0, 6, 0, 8, 272, 272, false);
        self.fill(world, clip, 1, 0, 0, 5, 0, 0, 272, 272, false);
        self.fill(world, clip, 1, 0, 8, 5, 0, 8, 272, 272, false);
        self.fill(world, clip, 3, 0, 1, 3, 0, 7, 144, 144, false);
        for n in 1..=7 {
            self.place(
                world,
                ((self.crops[0] << 4) | (2 + random.next_int(6)) as u16),
                1,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[0] << 4) | (2 + random.next_int(6)) as u16),
                2,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[1] << 4) | (2 + random.next_int(6)) as u16),
                4,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((self.crops[1] << 4) | (2 + random.next_int(6)) as u16),
                5,
                1,
                n,
                clip,
            );
        }
        for n in 0..9 {
            for i in 0..7 {
                self.clear(world, i, 4, n, clip);
                self.down(world, 48, i, -1, n, clip);
            }
        }
    }

    pub(super) fn template_house2(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        self.fill(world, clip, 0, 1, 0, 9, 4, 6, 0, 0, false);
        self.fill(world, clip, 0, 0, 0, 9, 0, 6, 64, 64, false);
        self.fill(world, clip, 0, 4, 0, 9, 4, 6, 64, 64, false);
        self.fill(world, clip, 0, 5, 0, 9, 5, 6, 704, 704, false);
        self.fill(world, clip, 1, 5, 1, 8, 5, 5, 0, 0, false);
        self.fill(world, clip, 1, 1, 0, 2, 3, 0, 80, 80, false);
        self.fill(world, clip, 0, 1, 0, 0, 4, 0, 272, 272, false);
        self.fill(world, clip, 3, 1, 0, 3, 4, 0, 272, 272, false);
        self.fill(world, clip, 0, 1, 6, 0, 4, 6, 272, 272, false);
        self.place(world, 80, 3, 3, 1, clip);
        self.fill(world, clip, 3, 1, 2, 3, 3, 2, 80, 80, false);
        self.fill(world, clip, 4, 1, 3, 5, 3, 3, 80, 80, false);
        self.fill(world, clip, 0, 1, 1, 0, 3, 5, 80, 80, false);
        self.fill(world, clip, 1, 1, 6, 5, 3, 6, 80, 80, false);
        self.fill(world, clip, 5, 1, 0, 5, 3, 0, 1360, 1360, false);
        self.fill(world, clip, 9, 1, 0, 9, 3, 0, 1360, 1360, false);
        self.fill(world, clip, 6, 1, 4, 9, 4, 6, 64, 64, false);
        self.place(world, 160, 7, 1, 5, clip);
        self.place(world, 160, 8, 1, 5, clip);
        self.place(world, 1616, 9, 2, 5, clip);
        self.place(world, 1616, 9, 2, 4, clip);
        self.fill(world, clip, 7, 2, 4, 8, 2, 5, 0, 0, false);
        self.place(world, 64, 6, 1, 3, clip);
        self.place(world, 978, 6, 2, 3, clip);
        self.place(world, 978, 6, 3, 3, clip);
        self.place(world, 688, 8, 1, 1, clip);
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 0, 2, 4, clip);
        self.place(world, 1632, 2, 2, 6, clip);
        self.place(world, 1632, 4, 2, 6, clip);
        self.place(world, 1360, 2, 1, 4, clip);
        self.place(world, 1152, 2, 2, 4, clip);
        self.place(world, 80, 1, 1, 5, clip);
        self.place(
            world,
            ((53 << 4) | (rotate_metadata(53, (3) as u8, self.orientation) as i32) as u16),
            2,
            1,
            5,
            clip,
        );
        self.place(
            world,
            ((53 << 4) | (rotate_metadata(53, (1) as u8, self.orientation) as i32) as u16),
            1,
            1,
            4,
            clip,
        );
        self.blacksmith_chest(world, clip, random);
        for n in 6..=8 {
            if self.get(world, n, 0, -1, clip) >> 4 != 0
                || self.get(world, n, -1, -1, clip) >> 4 == 0
            {
                continue;
            }
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                n,
                0,
                -1,
                clip,
            );
        }
        for n in 0..7 {
            for i in 0..10 {
                self.clear(world, i, 6, n, clip);
                self.down(world, 64, i, -1, n, clip);
            }
        }
        self.spawn(world, clip, 7, 1, 1, 1);
    }

    pub(super) fn template_house3(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        let mut n3: i32;
        let mut n4: i32;
        self.fill(world, clip, 1, 1, 1, 7, 4, 4, 0, 0, false);
        self.fill(world, clip, 2, 1, 6, 8, 4, 10, 0, 0, false);
        self.fill(world, clip, 2, 0, 5, 8, 0, 10, 80, 80, false);
        self.fill(world, clip, 1, 0, 1, 7, 0, 4, 80, 80, false);
        self.fill(world, clip, 0, 0, 0, 0, 3, 5, 64, 64, false);
        self.fill(world, clip, 8, 0, 0, 8, 3, 10, 64, 64, false);
        self.fill(world, clip, 1, 0, 0, 7, 2, 0, 64, 64, false);
        self.fill(world, clip, 1, 0, 5, 2, 1, 5, 64, 64, false);
        self.fill(world, clip, 2, 0, 6, 2, 3, 10, 64, 64, false);
        self.fill(world, clip, 3, 0, 10, 7, 3, 10, 64, 64, false);
        self.fill(world, clip, 1, 2, 0, 7, 3, 0, 80, 80, false);
        self.fill(world, clip, 1, 2, 5, 2, 3, 5, 80, 80, false);
        self.fill(world, clip, 0, 4, 1, 8, 4, 1, 80, 80, false);
        self.fill(world, clip, 0, 4, 4, 3, 4, 4, 80, 80, false);
        self.fill(world, clip, 0, 5, 2, 8, 5, 3, 80, 80, false);
        self.place(world, 80, 0, 4, 2, clip);
        self.place(world, 80, 0, 4, 3, clip);
        self.place(world, 80, 8, 4, 2, clip);
        self.place(world, 80, 8, 4, 3, clip);
        self.place(world, 80, 8, 4, 4, clip);
        let mut n5 = (rotate_metadata(53, (3) as u8, self.orientation) as i32);
        let mut n6 = (rotate_metadata(53, (2) as u8, self.orientation) as i32);
        for n4 in -1..=2 {
            for n3 in 0..=8 {
                self.place(world, ((53 << 4) | (n5) as u16), n3, 4 + n4, n4, clip);
                if n4 <= -1 && n3 > 1 || n4 <= 0 && n3 > 3 || n4 <= 1 && n3 > 4 && n3 < 6 {
                    continue;
                }
                self.place(world, ((53 << 4) | (n6) as u16), n3, 4 + n4, 5 - n4, clip);
            }
        }
        self.fill(world, clip, 3, 4, 5, 3, 4, 10, 80, 80, false);
        self.fill(world, clip, 7, 4, 2, 7, 4, 10, 80, 80, false);
        self.fill(world, clip, 4, 5, 4, 4, 5, 10, 80, 80, false);
        self.fill(world, clip, 6, 5, 4, 6, 5, 10, 80, 80, false);
        self.fill(world, clip, 5, 6, 3, 5, 6, 10, 80, 80, false);
        n4 = (rotate_metadata(53, (0) as u8, self.orientation) as i32);
        for n3 in (1..=4).rev() {
            self.place(world, 80, n3, 2 + n3, 7 - n3, clip);
            for n2 in 8 - n3..=10 {
                self.place(world, ((53 << 4) | (n4) as u16), n3, 2 + n3, n2, clip);
            }
        }
        n3 = (rotate_metadata(53, (1) as u8, self.orientation) as i32);
        self.place(world, 80, 6, 6, 3, clip);
        self.place(world, 80, 7, 5, 4, clip);
        self.place(world, ((53 << 4) | (n3) as u16), 6, 6, 4, clip);
        for n2 in 6..=8 {
            for n in 5..=10 {
                self.place(world, ((53 << 4) | (n3) as u16), n2, 12 - n2, n, clip);
            }
        }
        self.place(world, 272, 0, 2, 1, clip);
        self.place(world, 272, 0, 2, 4, clip);
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 0, 2, 3, clip);
        self.place(world, 272, 4, 2, 0, clip);
        self.place(world, 1632, 5, 2, 0, clip);
        self.place(world, 272, 6, 2, 0, clip);
        self.place(world, 272, 8, 2, 1, clip);
        self.place(world, 1632, 8, 2, 2, clip);
        self.place(world, 1632, 8, 2, 3, clip);
        self.place(world, 272, 8, 2, 4, clip);
        self.place(world, 80, 8, 2, 5, clip);
        self.place(world, 272, 8, 2, 6, clip);
        self.place(world, 1632, 8, 2, 7, clip);
        self.place(world, 1632, 8, 2, 8, clip);
        self.place(world, 272, 8, 2, 9, clip);
        self.place(world, 272, 2, 2, 6, clip);
        self.place(world, 1632, 2, 2, 7, clip);
        self.place(world, 1632, 2, 2, 8, clip);
        self.place(world, 272, 2, 2, 9, clip);
        self.place(world, 272, 4, 4, 10, clip);
        self.place(world, 1632, 5, 4, 10, clip);
        self.place(world, 272, 6, 4, 10, clip);
        self.place(world, 80, 5, 5, 10, clip);
        self.place(world, 0, 2, 1, 0, clip);
        self.place(world, 0, 2, 2, 0, clip);
        self.place(world, torch(self.orientation), 2, 3, 1, clip);
        self.door(
            world,
            clip,
            2,
            1,
            0,
            ((rotate_metadata(64, (1) as u8, self.orientation) as i32) as u8 & 3),
        );
        self.fill(world, clip, 1, 0, -1, 3, 2, -1, 0, 0, false);
        if self.get(world, 2, 0, -1, clip) >> 4 == 0 && self.get(world, 2, -1, -1, clip) >> 4 != 0 {
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                2,
                0,
                -1,
                clip,
            );
        }
        for n2 in 0..5 {
            for n in 0..9 {
                self.clear(world, n, 7, n2, clip);
                self.down(world, 64, n, -1, n2, clip);
            }
        }
        for n2 in 5..11 {
            for n in 2..9 {
                self.clear(world, n, 7, n2, clip);
                self.down(world, 64, n, -1, n2, clip);
            }
        }
        self.spawn(world, clip, 4, 1, 2, 2);
    }

    pub(super) fn template_hall(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        self.fill(world, clip, 1, 1, 1, 7, 4, 4, 0, 0, false);
        self.fill(world, clip, 2, 1, 6, 8, 4, 10, 0, 0, false);
        self.fill(world, clip, 2, 0, 6, 8, 0, 10, 48, 48, false);
        self.place(world, 64, 6, 0, 6, clip);
        self.fill(world, clip, 2, 1, 6, 2, 1, 10, 1360, 1360, false);
        self.fill(world, clip, 8, 1, 6, 8, 1, 10, 1360, 1360, false);
        self.fill(world, clip, 3, 1, 10, 7, 1, 10, 1360, 1360, false);
        self.fill(world, clip, 1, 0, 1, 7, 0, 4, 80, 80, false);
        self.fill(world, clip, 0, 0, 0, 0, 3, 5, 64, 64, false);
        self.fill(world, clip, 8, 0, 0, 8, 3, 5, 64, 64, false);
        self.fill(world, clip, 1, 0, 0, 7, 1, 0, 64, 64, false);
        self.fill(world, clip, 1, 0, 5, 7, 1, 5, 64, 64, false);
        self.fill(world, clip, 1, 2, 0, 7, 3, 0, 80, 80, false);
        self.fill(world, clip, 1, 2, 5, 7, 3, 5, 80, 80, false);
        self.fill(world, clip, 0, 4, 1, 8, 4, 1, 80, 80, false);
        self.fill(world, clip, 0, 4, 4, 8, 4, 4, 80, 80, false);
        self.fill(world, clip, 0, 5, 2, 8, 5, 3, 80, 80, false);
        self.place(world, 80, 0, 4, 2, clip);
        self.place(world, 80, 0, 4, 3, clip);
        self.place(world, 80, 8, 4, 2, clip);
        self.place(world, 80, 8, 4, 3, clip);
        let mut n3 = (rotate_metadata(53, (3) as u8, self.orientation) as i32);
        let mut n4 = (rotate_metadata(53, (2) as u8, self.orientation) as i32);
        for n2 in -1..=2 {
            for n in 0..=8 {
                self.place(world, ((53 << 4) | (n3) as u16), n, 4 + n2, n2, clip);
                self.place(world, ((53 << 4) | (n4) as u16), n, 4 + n2, 5 - n2, clip);
            }
        }
        self.place(world, 272, 0, 2, 1, clip);
        self.place(world, 272, 0, 2, 4, clip);
        self.place(world, 272, 8, 2, 1, clip);
        self.place(world, 272, 8, 2, 4, clip);
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 0, 2, 3, clip);
        self.place(world, 1632, 8, 2, 2, clip);
        self.place(world, 1632, 8, 2, 3, clip);
        self.place(world, 1632, 2, 2, 5, clip);
        self.place(world, 1632, 3, 2, 5, clip);
        self.place(world, 1632, 5, 2, 0, clip);
        self.place(world, 1632, 6, 2, 5, clip);
        self.place(world, 1360, 2, 1, 3, clip);
        self.place(world, 1152, 2, 2, 3, clip);
        self.place(world, 80, 1, 1, 4, clip);
        self.place(
            world,
            ((53 << 4) | (rotate_metadata(53, (3) as u8, self.orientation) as i32) as u16),
            2,
            1,
            4,
            clip,
        );
        self.place(
            world,
            ((53 << 4) | (rotate_metadata(53, (1) as u8, self.orientation) as i32) as u16),
            1,
            1,
            3,
            clip,
        );
        self.fill(world, clip, 5, 0, 1, 7, 0, 3, 688, 688, false);
        self.place(world, 688, 6, 1, 1, clip);
        self.place(world, 688, 6, 1, 2, clip);
        self.place(world, 0, 2, 1, 0, clip);
        self.place(world, 0, 2, 2, 0, clip);
        self.place(world, torch(self.orientation), 2, 3, 1, clip);
        self.door(
            world,
            clip,
            2,
            1,
            0,
            ((rotate_metadata(64, (1) as u8, self.orientation) as i32) as u8 & 3),
        );
        if self.get(world, 2, 0, -1, clip) >> 4 == 0 && self.get(world, 2, -1, -1, clip) >> 4 != 0 {
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                2,
                0,
                -1,
                clip,
            );
        }
        self.place(world, 0, 6, 1, 5, clip);
        self.place(world, 0, 6, 2, 5, clip);
        self.place(world, torch(((self.orientation + 2) & 3)), 6, 3, 4, clip);
        self.door(
            world,
            clip,
            6,
            1,
            5,
            ((rotate_metadata(64, (1) as u8, self.orientation) as i32) as u8 & 3),
        );
        for n2 in 0..5 {
            for n in 0..9 {
                self.clear(world, n, 7, n2, clip);
                self.down(world, 64, n, -1, n2, clip);
            }
        }
        self.spawn(world, clip, 4, 1, 2, 2);
    }

    pub(super) fn template_woodhut(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 1, 1, 1, 3, 5, 4, 0, 0, false);
        self.fill(world, clip, 0, 0, 0, 3, 0, 4, 64, 64, false);
        self.fill(world, clip, 1, 0, 1, 2, 0, 3, 48, 48, false);
        if self.tall_house {
            self.fill(world, clip, 1, 4, 1, 2, 4, 3, 272, 272, false);
        } else {
            self.fill(world, clip, 1, 5, 1, 2, 5, 3, 272, 272, false);
        }
        self.place(world, 272, 1, 4, 0, clip);
        self.place(world, 272, 2, 4, 0, clip);
        self.place(world, 272, 1, 4, 4, clip);
        self.place(world, 272, 2, 4, 4, clip);
        self.place(world, 272, 0, 4, 1, clip);
        self.place(world, 272, 0, 4, 2, clip);
        self.place(world, 272, 0, 4, 3, clip);
        self.place(world, 272, 3, 4, 1, clip);
        self.place(world, 272, 3, 4, 2, clip);
        self.place(world, 272, 3, 4, 3, clip);
        self.fill(world, clip, 0, 1, 0, 0, 3, 0, 272, 272, false);
        self.fill(world, clip, 3, 1, 0, 3, 3, 0, 272, 272, false);
        self.fill(world, clip, 0, 1, 4, 0, 3, 4, 272, 272, false);
        self.fill(world, clip, 3, 1, 4, 3, 3, 4, 272, 272, false);
        self.fill(world, clip, 0, 1, 1, 0, 3, 3, 80, 80, false);
        self.fill(world, clip, 3, 1, 1, 3, 3, 3, 80, 80, false);
        self.fill(world, clip, 1, 1, 0, 2, 3, 0, 80, 80, false);
        self.fill(world, clip, 1, 1, 4, 2, 3, 4, 80, 80, false);
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 3, 2, 2, clip);
        if self.table_position > 0 {
            self.place(world, 1360, self.table_position, 1, 3, clip);
            self.place(world, 1152, self.table_position, 2, 3, clip);
        }
        self.place(world, 0, 1, 1, 0, clip);
        self.place(world, 0, 1, 2, 0, clip);
        self.door(
            world,
            clip,
            1,
            1,
            0,
            ((rotate_metadata(64, (1) as u8, self.orientation) as i32) as u8 & 3),
        );
        if self.get(world, 1, 0, -1, clip) >> 4 == 0 && self.get(world, 1, -1, -1, clip) >> 4 != 0 {
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                1,
                0,
                -1,
                clip,
            );
        }
        for i in 0..5 {
            for j in 0..4 {
                self.clear(world, j, 6, i, clip);
                self.down(world, 64, j, -1, i, clip);
            }
        }
        self.spawn(world, clip, 1, 1, 2, 1);
    }

    pub(super) fn template_house1(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        self.fill(world, clip, 1, 1, 1, 7, 5, 4, 0, 0, false);
        self.fill(world, clip, 0, 0, 0, 8, 0, 5, 64, 64, false);
        self.fill(world, clip, 0, 5, 0, 8, 5, 5, 64, 64, false);
        self.fill(world, clip, 0, 6, 1, 8, 6, 4, 64, 64, false);
        self.fill(world, clip, 0, 7, 2, 8, 7, 3, 64, 64, false);
        let mut n3 = (rotate_metadata(53, (3) as u8, self.orientation) as i32);
        let mut n4 = (rotate_metadata(53, (2) as u8, self.orientation) as i32);
        for n2 in -1..=2 {
            for n in 0..=8 {
                self.place(world, ((53 << 4) | (n3) as u16), n, 6 + n2, n2, clip);
                self.place(world, ((53 << 4) | (n4) as u16), n, 6 + n2, 5 - n2, clip);
            }
        }
        self.fill(world, clip, 0, 1, 0, 0, 1, 5, 64, 64, false);
        self.fill(world, clip, 1, 1, 5, 8, 1, 5, 64, 64, false);
        self.fill(world, clip, 8, 1, 0, 8, 1, 4, 64, 64, false);
        self.fill(world, clip, 2, 1, 0, 7, 1, 0, 64, 64, false);
        self.fill(world, clip, 0, 2, 0, 0, 4, 0, 64, 64, false);
        self.fill(world, clip, 0, 2, 5, 0, 4, 5, 64, 64, false);
        self.fill(world, clip, 8, 2, 5, 8, 4, 5, 64, 64, false);
        self.fill(world, clip, 8, 2, 0, 8, 4, 0, 64, 64, false);
        self.fill(world, clip, 0, 2, 1, 0, 4, 4, 80, 80, false);
        self.fill(world, clip, 1, 2, 5, 7, 4, 5, 80, 80, false);
        self.fill(world, clip, 8, 2, 1, 8, 4, 4, 80, 80, false);
        self.fill(world, clip, 1, 2, 0, 7, 4, 0, 80, 80, false);
        self.place(world, 1632, 4, 2, 0, clip);
        self.place(world, 1632, 5, 2, 0, clip);
        self.place(world, 1632, 6, 2, 0, clip);
        self.place(world, 1632, 4, 3, 0, clip);
        self.place(world, 1632, 5, 3, 0, clip);
        self.place(world, 1632, 6, 3, 0, clip);
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 0, 2, 3, clip);
        self.place(world, 1632, 0, 3, 2, clip);
        self.place(world, 1632, 0, 3, 3, clip);
        self.place(world, 1632, 8, 2, 2, clip);
        self.place(world, 1632, 8, 2, 3, clip);
        self.place(world, 1632, 8, 3, 2, clip);
        self.place(world, 1632, 8, 3, 3, clip);
        self.place(world, 1632, 2, 2, 5, clip);
        self.place(world, 1632, 3, 2, 5, clip);
        self.place(world, 1632, 5, 2, 5, clip);
        self.place(world, 1632, 6, 2, 5, clip);
        self.fill(world, clip, 1, 4, 1, 7, 4, 1, 80, 80, false);
        self.fill(world, clip, 1, 4, 4, 7, 4, 4, 80, 80, false);
        self.fill(world, clip, 1, 3, 4, 7, 3, 4, 752, 752, false);
        self.place(world, 80, 7, 1, 4, clip);
        self.place(
            world,
            ((53 << 4) | (rotate_metadata(53, (0) as u8, self.orientation) as i32) as u16),
            7,
            1,
            3,
            clip,
        );
        n2 = (rotate_metadata(53, (3) as u8, self.orientation) as i32);
        self.place(world, ((53 << 4) | (n2) as u16), 6, 1, 4, clip);
        self.place(world, ((53 << 4) | (n2) as u16), 5, 1, 4, clip);
        self.place(world, ((53 << 4) | (n2) as u16), 4, 1, 4, clip);
        self.place(world, ((53 << 4) | (n2) as u16), 3, 1, 4, clip);
        self.place(world, 1360, 6, 1, 3, clip);
        self.place(world, 1152, 6, 2, 3, clip);
        self.place(world, 1360, 4, 1, 3, clip);
        self.place(world, 1152, 4, 2, 3, clip);
        self.place(world, 928, 7, 1, 1, clip);
        self.place(world, 0, 1, 1, 0, clip);
        self.place(world, 0, 1, 2, 0, clip);
        self.door(
            world,
            clip,
            1,
            1,
            0,
            ((rotate_metadata(64, (1) as u8, self.orientation) as i32) as u8 & 3),
        );
        if self.get(world, 1, 0, -1, clip) >> 4 == 0 && self.get(world, 1, -1, -1, clip) >> 4 != 0 {
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                1,
                0,
                -1,
                clip,
            );
        }
        for n in 0..6 {
            for i in 0..9 {
                self.clear(world, i, 9, n, clip);
                self.down(world, 64, i, -1, n, clip);
            }
        }
        self.spawn(world, clip, 2, 1, 2, 1);
    }

    pub(super) fn template_church(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        self.fill(world, clip, 1, 1, 1, 3, 3, 7, 0, 0, false);
        self.fill(world, clip, 1, 5, 1, 3, 9, 3, 0, 0, false);
        self.fill(world, clip, 1, 0, 0, 3, 0, 8, 64, 64, false);
        self.fill(world, clip, 1, 1, 0, 3, 10, 0, 64, 64, false);
        self.fill(world, clip, 0, 1, 1, 0, 10, 3, 64, 64, false);
        self.fill(world, clip, 4, 1, 1, 4, 10, 3, 64, 64, false);
        self.fill(world, clip, 0, 0, 4, 0, 4, 7, 64, 64, false);
        self.fill(world, clip, 4, 0, 4, 4, 4, 7, 64, 64, false);
        self.fill(world, clip, 1, 1, 8, 3, 4, 8, 64, 64, false);
        self.fill(world, clip, 1, 5, 4, 3, 10, 4, 64, 64, false);
        self.fill(world, clip, 1, 5, 5, 3, 5, 7, 64, 64, false);
        self.fill(world, clip, 0, 9, 0, 4, 9, 4, 64, 64, false);
        self.fill(world, clip, 0, 4, 0, 4, 4, 4, 64, 64, false);
        self.place(world, 64, 0, 11, 2, clip);
        self.place(world, 64, 4, 11, 2, clip);
        self.place(world, 64, 2, 11, 0, clip);
        self.place(world, 64, 2, 11, 4, clip);
        self.place(world, 64, 1, 1, 6, clip);
        self.place(world, 64, 1, 1, 7, clip);
        self.place(world, 64, 2, 1, 7, clip);
        self.place(world, 64, 3, 1, 6, clip);
        self.place(world, 64, 3, 1, 7, clip);
        self.place(
            world,
            ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
            1,
            1,
            5,
            clip,
        );
        self.place(
            world,
            ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
            2,
            1,
            6,
            clip,
        );
        self.place(
            world,
            ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
            3,
            1,
            5,
            clip,
        );
        self.place(
            world,
            ((67 << 4) | (rotate_metadata(67, (1) as u8, self.orientation) as i32) as u16),
            1,
            2,
            7,
            clip,
        );
        self.place(
            world,
            ((67 << 4) | (rotate_metadata(67, (0) as u8, self.orientation) as i32) as u16),
            3,
            2,
            7,
            clip,
        );
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 0, 3, 2, clip);
        self.place(world, 1632, 4, 2, 2, clip);
        self.place(world, 1632, 4, 3, 2, clip);
        self.place(world, 1632, 0, 6, 2, clip);
        self.place(world, 1632, 0, 7, 2, clip);
        self.place(world, 1632, 4, 6, 2, clip);
        self.place(world, 1632, 4, 7, 2, clip);
        self.place(world, 1632, 2, 6, 0, clip);
        self.place(world, 1632, 2, 7, 0, clip);
        self.place(world, 1632, 2, 6, 4, clip);
        self.place(world, 1632, 2, 7, 4, clip);
        self.place(world, 1632, 0, 3, 6, clip);
        self.place(world, 1632, 4, 3, 6, clip);
        self.place(world, 1632, 2, 3, 8, clip);
        self.place(world, torch(((self.orientation + 2) & 3)), 2, 4, 7, clip);
        self.place(world, torch(((self.orientation + 1) & 3)), 1, 4, 6, clip);
        self.place(world, torch(((self.orientation + 3) & 3)), 3, 4, 6, clip);
        self.place(world, torch(self.orientation), 2, 4, 5, clip);
        let mut n2 = (rotate_metadata(65, (4) as u8, self.orientation) as i32);
        for n in 1..=9 {
            self.place(world, ((65 << 4) | (n2) as u16), 3, n, 3, clip);
        }
        self.place(world, 0, 2, 1, 0, clip);
        self.place(world, 0, 2, 2, 0, clip);
        self.door(
            world,
            clip,
            2,
            1,
            0,
            ((rotate_metadata(64, (1) as u8, self.orientation) as i32) as u8 & 3),
        );
        if self.get(world, 2, 0, -1, clip) >> 4 == 0 && self.get(world, 2, -1, -1, clip) >> 4 != 0 {
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                2,
                0,
                -1,
                clip,
            );
        }
        for n in 0..9 {
            for i in 0..5 {
                self.clear(world, i, 12, n, clip);
                self.down(world, 64, i, -1, n, clip);
            }
        }
        self.spawn(world, clip, 2, 1, 2, 1);
    }

    pub(super) fn template_house4garden(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        self.fill(world, clip, 0, 0, 0, 4, 0, 4, 64, 64, false);
        self.fill(world, clip, 0, 4, 0, 4, 4, 4, 272, 272, false);
        self.fill(world, clip, 1, 4, 1, 3, 4, 3, 80, 80, false);
        self.place(world, 64, 0, 1, 0, clip);
        self.place(world, 64, 0, 2, 0, clip);
        self.place(world, 64, 0, 3, 0, clip);
        self.place(world, 64, 4, 1, 0, clip);
        self.place(world, 64, 4, 2, 0, clip);
        self.place(world, 64, 4, 3, 0, clip);
        self.place(world, 64, 0, 1, 4, clip);
        self.place(world, 64, 0, 2, 4, clip);
        self.place(world, 64, 0, 3, 4, clip);
        self.place(world, 64, 4, 1, 4, clip);
        self.place(world, 64, 4, 2, 4, clip);
        self.place(world, 64, 4, 3, 4, clip);
        self.fill(world, clip, 0, 1, 1, 0, 3, 3, 80, 80, false);
        self.fill(world, clip, 4, 1, 1, 4, 3, 3, 80, 80, false);
        self.fill(world, clip, 1, 1, 4, 3, 3, 4, 80, 80, false);
        self.place(world, 1632, 0, 2, 2, clip);
        self.place(world, 1632, 2, 2, 4, clip);
        self.place(world, 1632, 4, 2, 2, clip);
        self.place(world, 80, 1, 1, 0, clip);
        self.place(world, 80, 1, 2, 0, clip);
        self.place(world, 80, 1, 3, 0, clip);
        self.place(world, 80, 2, 3, 0, clip);
        self.place(world, 80, 3, 3, 0, clip);
        self.place(world, 80, 3, 2, 0, clip);
        self.place(world, 80, 3, 1, 0, clip);
        if self.get(world, 2, 0, -1, clip) >> 4 == 0 && self.get(world, 2, -1, -1, clip) >> 4 != 0 {
            self.place(
                world,
                ((67 << 4) | (rotate_metadata(67, (3) as u8, self.orientation) as i32) as u16),
                2,
                0,
                -1,
                clip,
            );
        }
        self.fill(world, clip, 1, 1, 1, 3, 3, 3, 0, 0, false);
        if self.roof_accessible {
            self.place(world, 1360, 0, 5, 0, clip);
            self.place(world, 1360, 1, 5, 0, clip);
            self.place(world, 1360, 2, 5, 0, clip);
            self.place(world, 1360, 3, 5, 0, clip);
            self.place(world, 1360, 4, 5, 0, clip);
            self.place(world, 1360, 0, 5, 4, clip);
            self.place(world, 1360, 1, 5, 4, clip);
            self.place(world, 1360, 2, 5, 4, clip);
            self.place(world, 1360, 3, 5, 4, clip);
            self.place(world, 1360, 4, 5, 4, clip);
            self.place(world, 1360, 4, 5, 1, clip);
            self.place(world, 1360, 4, 5, 2, clip);
            self.place(world, 1360, 4, 5, 3, clip);
            self.place(world, 1360, 0, 5, 1, clip);
            self.place(world, 1360, 0, 5, 2, clip);
            self.place(world, 1360, 0, 5, 3, clip);
        }
        if self.roof_accessible {
            n = (rotate_metadata(65, (3) as u8, self.orientation) as i32);
            self.place(world, ((65 << 4) | (n) as u16), 3, 1, 3, clip);
            self.place(world, ((65 << 4) | (n) as u16), 3, 2, 3, clip);
            self.place(world, ((65 << 4) | (n) as u16), 3, 3, 3, clip);
            self.place(world, ((65 << 4) | (n) as u16), 3, 4, 3, clip);
        }
        self.place(world, torch(self.orientation), 2, 3, 1, clip);
        for n in 0..5 {
            for i in 0..5 {
                self.clear(world, i, 6, n, clip);
                self.down(world, 64, i, -1, n, clip);
            }
        }
        self.spawn(world, clip, 1, 1, 2, 1);
    }

    pub(super) fn template_well(
        &mut self,
        world: &mut impl VillageWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 1, 0, 1, 4, 12, 4, 64, 128, false);
        self.place(world, 0, 2, 12, 2, clip);
        self.place(world, 0, 3, 12, 2, clip);
        self.place(world, 0, 2, 12, 3, clip);
        self.place(world, 0, 3, 12, 3, clip);
        self.place(world, 1360, 1, 13, 1, clip);
        self.place(world, 1360, 1, 14, 1, clip);
        self.place(world, 1360, 4, 13, 1, clip);
        self.place(world, 1360, 4, 14, 1, clip);
        self.place(world, 1360, 1, 13, 4, clip);
        self.place(world, 1360, 1, 14, 4, clip);
        self.place(world, 1360, 4, 13, 4, clip);
        self.place(world, 1360, 4, 14, 4, clip);
        self.fill(world, clip, 1, 15, 1, 4, 15, 4, 64, 64, false);
        for i in 0..=5 {
            for j in 0..=5 {
                if j != 0 && j != 5 && i != 0 && i != 5 {
                    continue;
                }
                self.place(world, 208, j, 11, i, clip);
                self.clear(world, j, 12, i, clip);
            }
        }
    }
}
