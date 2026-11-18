//! Original Nether fortress block template transcription.
#![allow(
    unused_variables,
    unused_mut,
    unused_assignments,
    unused_parens,
    clippy::double_parens,
    clippy::unnecessary_cast
)]
use super::*;
impl Piece {
    pub(super) fn template_straight(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 3, 0, 4, 4, 18, 1792, 1792, false);
        self.fill(world, clip, 1, 5, 0, 3, 7, 18, 0, 0, false);
        self.fill(world, clip, 0, 5, 0, 0, 5, 18, 1792, 1792, false);
        self.fill(world, clip, 4, 5, 0, 4, 5, 18, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 4, 2, 5, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 13, 4, 2, 18, 1792, 1792, false);
        self.fill(world, clip, 0, 0, 0, 4, 1, 3, 1792, 1792, false);
        self.fill(world, clip, 0, 0, 15, 4, 1, 18, 1792, 1792, false);
        for i in (0..=4) {
            for j in (0..=2) {
                self.down(world, 1792, i, -1, j, clip);
                self.down(world, 1792, i, -1, 18 - j, clip);
            }
        }
        self.fill(world, clip, 0, 1, 1, 0, 4, 1, 1808, 1808, false);
        self.fill(world, clip, 0, 3, 4, 0, 4, 4, 1808, 1808, false);
        self.fill(world, clip, 0, 3, 14, 0, 4, 14, 1808, 1808, false);
        self.fill(world, clip, 0, 1, 17, 0, 4, 17, 1808, 1808, false);
        self.fill(world, clip, 4, 1, 1, 4, 4, 1, 1808, 1808, false);
        self.fill(world, clip, 4, 3, 4, 4, 4, 4, 1808, 1808, false);
        self.fill(world, clip, 4, 3, 14, 4, 4, 14, 1808, 1808, false);
        self.fill(world, clip, 4, 1, 17, 4, 4, 17, 1808, 1808, false);
    }
    pub(super) fn template_crossing3(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        self.fill(world, clip, 7, 3, 0, 11, 4, 18, 1792, 1792, false);
        self.fill(world, clip, 0, 3, 7, 18, 4, 11, 1792, 1792, false);
        self.fill(world, clip, 8, 5, 0, 10, 7, 18, 0, 0, false);
        self.fill(world, clip, 0, 5, 8, 18, 7, 10, 0, 0, false);
        self.fill(world, clip, 7, 5, 0, 7, 5, 7, 1792, 1792, false);
        self.fill(world, clip, 7, 5, 11, 7, 5, 18, 1792, 1792, false);
        self.fill(world, clip, 11, 5, 0, 11, 5, 7, 1792, 1792, false);
        self.fill(world, clip, 11, 5, 11, 11, 5, 18, 1792, 1792, false);
        self.fill(world, clip, 0, 5, 7, 7, 5, 7, 1792, 1792, false);
        self.fill(world, clip, 11, 5, 7, 18, 5, 7, 1792, 1792, false);
        self.fill(world, clip, 0, 5, 11, 7, 5, 11, 1792, 1792, false);
        self.fill(world, clip, 11, 5, 11, 18, 5, 11, 1792, 1792, false);
        self.fill(world, clip, 7, 2, 0, 11, 2, 5, 1792, 1792, false);
        self.fill(world, clip, 7, 2, 13, 11, 2, 18, 1792, 1792, false);
        self.fill(world, clip, 7, 0, 0, 11, 1, 3, 1792, 1792, false);
        self.fill(world, clip, 7, 0, 15, 11, 1, 18, 1792, 1792, false);
        for n2 in (7..=11) {
            for n in (0..=2) {
                self.down(world, 1792, n2, -1, n, clip);
                self.down(world, 1792, n2, -1, 18 - n, clip);
            }
        }
        self.fill(world, clip, 0, 2, 7, 5, 2, 11, 1792, 1792, false);
        self.fill(world, clip, 13, 2, 7, 18, 2, 11, 1792, 1792, false);
        self.fill(world, clip, 0, 0, 7, 3, 1, 11, 1792, 1792, false);
        self.fill(world, clip, 15, 0, 7, 18, 1, 11, 1792, 1792, false);
        for n2 in (0..=2) {
            for n in (7..=11) {
                self.down(world, 1792, n2, -1, n, clip);
                self.down(world, 1792, 18 - n2, -1, n, clip);
            }
        }
    }
    pub(super) fn template_crossing(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 6, 1, 6, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 6, 7, 6, 0, 0, false);
        self.fill(world, clip, 0, 2, 0, 1, 6, 0, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 6, 1, 6, 6, 1792, 1792, false);
        self.fill(world, clip, 5, 2, 0, 6, 6, 0, 1792, 1792, false);
        self.fill(world, clip, 5, 2, 6, 6, 6, 6, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 0, 6, 1, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 5, 0, 6, 6, 1792, 1792, false);
        self.fill(world, clip, 6, 2, 0, 6, 6, 1, 1792, 1792, false);
        self.fill(world, clip, 6, 2, 5, 6, 6, 6, 1792, 1792, false);
        self.fill(world, clip, 2, 6, 0, 4, 6, 0, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 0, 4, 5, 0, 1808, 1808, false);
        self.fill(world, clip, 2, 6, 6, 4, 6, 6, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 6, 4, 5, 6, 1808, 1808, false);
        self.fill(world, clip, 0, 6, 2, 0, 6, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 5, 2, 0, 5, 4, 1808, 1808, false);
        self.fill(world, clip, 6, 6, 2, 6, 6, 4, 1792, 1792, false);
        self.fill(world, clip, 6, 5, 2, 6, 5, 4, 1808, 1808, false);
        for i in (0..=6) {
            for j in (0..=6) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_stairs(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 6, 1, 6, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 6, 10, 6, 0, 0, false);
        self.fill(world, clip, 0, 2, 0, 1, 8, 0, 1792, 1792, false);
        self.fill(world, clip, 5, 2, 0, 6, 8, 0, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 1, 0, 8, 6, 1792, 1792, false);
        self.fill(world, clip, 6, 2, 1, 6, 8, 6, 1792, 1792, false);
        self.fill(world, clip, 1, 2, 6, 5, 8, 6, 1792, 1792, false);
        self.fill(world, clip, 0, 3, 2, 0, 5, 4, 1808, 1808, false);
        self.fill(world, clip, 6, 3, 2, 6, 5, 2, 1808, 1808, false);
        self.fill(world, clip, 6, 3, 4, 6, 5, 4, 1808, 1808, false);
        self.place(world, 1792, 5, 2, 5, clip);
        self.fill(world, clip, 4, 2, 5, 4, 3, 5, 1792, 1792, false);
        self.fill(world, clip, 3, 2, 5, 3, 4, 5, 1792, 1792, false);
        self.fill(world, clip, 2, 2, 5, 2, 5, 5, 1792, 1792, false);
        self.fill(world, clip, 1, 2, 5, 1, 6, 5, 1792, 1792, false);
        self.fill(world, clip, 1, 7, 1, 5, 7, 4, 1792, 1792, false);
        self.fill(world, clip, 6, 8, 2, 6, 8, 4, 0, 0, false);
        self.fill(world, clip, 2, 6, 0, 4, 8, 0, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 0, 4, 5, 0, 1808, 1808, false);
        for i in (0..=6) {
            for j in (0..=6) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_throne(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 2, 0, 6, 7, 7, 0, 0, false);
        self.fill(world, clip, 1, 0, 0, 5, 1, 7, 1792, 1792, false);
        self.fill(world, clip, 1, 2, 1, 5, 2, 7, 1792, 1792, false);
        self.fill(world, clip, 1, 3, 2, 5, 3, 7, 1792, 1792, false);
        self.fill(world, clip, 1, 4, 3, 5, 4, 7, 1792, 1792, false);
        self.fill(world, clip, 1, 2, 0, 1, 4, 2, 1792, 1792, false);
        self.fill(world, clip, 5, 2, 0, 5, 4, 2, 1792, 1792, false);
        self.fill(world, clip, 1, 5, 2, 1, 5, 3, 1792, 1792, false);
        self.fill(world, clip, 5, 5, 2, 5, 5, 3, 1792, 1792, false);
        self.fill(world, clip, 0, 5, 3, 0, 5, 8, 1792, 1792, false);
        self.fill(world, clip, 6, 5, 3, 6, 5, 8, 1792, 1792, false);
        self.fill(world, clip, 1, 5, 8, 5, 5, 8, 1792, 1792, false);
        self.place(world, 1808, 1, 6, 3, clip);
        self.place(world, 1808, 5, 6, 3, clip);
        self.fill(world, clip, 0, 6, 3, 0, 6, 8, 1808, 1808, false);
        self.fill(world, clip, 6, 6, 3, 6, 6, 8, 1808, 1808, false);
        self.fill(world, clip, 1, 6, 8, 5, 7, 8, 1808, 1808, false);
        self.fill(world, clip, 2, 8, 8, 4, 8, 8, 1808, 1808, false);
        self.spawner(world, clip);
        for i in (0..=6) {
            for j in (0..=6) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_entrance(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        self.fill(world, clip, 0, 3, 0, 12, 4, 12, 1792, 1792, false);
        self.fill(world, clip, 0, 5, 0, 12, 13, 12, 0, 0, false);
        self.fill(world, clip, 0, 5, 0, 1, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 11, 5, 0, 12, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 11, 4, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 8, 5, 11, 10, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 5, 9, 11, 7, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 0, 4, 12, 1, 1792, 1792, false);
        self.fill(world, clip, 8, 5, 0, 10, 12, 1, 1792, 1792, false);
        self.fill(world, clip, 5, 9, 0, 7, 12, 1, 1792, 1792, false);
        self.fill(world, clip, 2, 11, 2, 10, 12, 10, 1792, 1792, false);
        self.fill(world, clip, 5, 8, 0, 7, 8, 0, 1808, 1808, false);
        for n2 in (1..=11).step_by((2) as usize) {
            self.fill(world, clip, n2, 10, 0, n2, 11, 0, 1808, 1808, false);
            self.fill(world, clip, n2, 10, 12, n2, 11, 12, 1808, 1808, false);
            self.fill(world, clip, 0, 10, n2, 0, 11, n2, 1808, 1808, false);
            self.fill(world, clip, 12, 10, n2, 12, 11, n2, 1808, 1808, false);
            self.place(world, 1792, n2, 13, 0, clip);
            self.place(world, 1792, n2, 13, 12, clip);
            self.place(world, 1792, 0, 13, n2, clip);
            self.place(world, 1792, 12, 13, n2, clip);
            self.place(world, 1808, n2 + 1, 13, 0, clip);
            self.place(world, 1808, n2 + 1, 13, 12, clip);
            self.place(world, 1808, 0, 13, n2 + 1, clip);
            self.place(world, 1808, 12, 13, n2 + 1, clip);
        }
        self.place(world, 1808, 0, 13, 0, clip);
        self.place(world, 1808, 0, 13, 12, clip);
        self.place(world, 1808, 0, 13, 0, clip);
        self.place(world, 1808, 12, 13, 0, clip);
        for n2 in (3..=9).step_by((2) as usize) {
            self.fill(world, clip, 1, 7, n2, 1, 8, n2, 1808, 1808, false);
            self.fill(world, clip, 11, 7, n2, 11, 8, n2, 1808, 1808, false);
        }
        self.fill(world, clip, 4, 2, 0, 8, 2, 12, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 4, 12, 2, 8, 1792, 1792, false);
        self.fill(world, clip, 4, 0, 0, 8, 1, 3, 1792, 1792, false);
        self.fill(world, clip, 4, 0, 9, 8, 1, 12, 1792, 1792, false);
        self.fill(world, clip, 0, 0, 4, 3, 1, 8, 1792, 1792, false);
        self.fill(world, clip, 9, 0, 4, 12, 1, 8, 1792, 1792, false);
        for n2 in (4..=8) {
            for n in (0..=2) {
                self.down(world, 1792, n2, -1, n, clip);
                self.down(world, 1792, n2, -1, 12 - n, clip);
            }
        }
        for n2 in (0..=2) {
            for n in (4..=8) {
                self.down(world, 1792, n2, -1, n, clip);
                self.down(world, 1792, 12 - n2, -1, n, clip);
            }
        }
        self.fill(world, clip, 5, 5, 5, 7, 5, 7, 1792, 1792, false);
        self.fill(world, clip, 6, 1, 6, 6, 4, 6, 0, 0, false);
        self.place(world, 1792, 6, 0, 6, clip);
        self.place(world, 160, 6, 5, 6, clip);
        self.force_lava(world, clip, random);
    }
    pub(super) fn template_corridor5(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 4, 1, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 4, 5, 4, 0, 0, false);
        self.fill(world, clip, 0, 2, 0, 0, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 4, 2, 0, 4, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 3, 1, 0, 4, 1, 1808, 1808, false);
        self.fill(world, clip, 0, 3, 3, 0, 4, 3, 1808, 1808, false);
        self.fill(world, clip, 4, 3, 1, 4, 4, 1, 1808, 1808, false);
        self.fill(world, clip, 4, 3, 3, 4, 4, 3, 1808, 1808, false);
        self.fill(world, clip, 0, 6, 0, 4, 6, 4, 1792, 1792, false);
        for i in (0..=4) {
            for j in (0..=4) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_crossing2(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 4, 1, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 4, 5, 4, 0, 0, false);
        self.fill(world, clip, 0, 2, 0, 0, 5, 0, 1792, 1792, false);
        self.fill(world, clip, 4, 2, 0, 4, 5, 0, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 4, 0, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 4, 2, 4, 4, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 6, 0, 4, 6, 4, 1792, 1792, false);
        for i in (0..=4) {
            for j in (0..=4) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_corridor2(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 4, 1, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 4, 5, 4, 0, 0, false);
        self.fill(world, clip, 0, 2, 0, 0, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 3, 1, 0, 4, 1, 1808, 1808, false);
        self.fill(world, clip, 0, 3, 3, 0, 4, 3, 1808, 1808, false);
        self.fill(world, clip, 4, 2, 0, 4, 5, 0, 1792, 1792, false);
        self.fill(world, clip, 1, 2, 4, 4, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 1, 3, 4, 1, 4, 4, 1808, 1792, false);
        self.fill(world, clip, 3, 3, 4, 3, 4, 4, 1808, 1792, false);
        self.chest(world, clip, random, [1, 2, 3]);
        self.fill(world, clip, 0, 6, 0, 4, 6, 4, 1792, 1792, false);
        for i in (0..=4) {
            for j in (0..=4) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_corridor(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 4, 1, 4, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 4, 5, 4, 0, 0, false);
        self.fill(world, clip, 4, 2, 0, 4, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 4, 3, 1, 4, 4, 1, 1808, 1808, false);
        self.fill(world, clip, 4, 3, 3, 4, 4, 3, 1808, 1808, false);
        self.fill(world, clip, 0, 2, 0, 0, 5, 0, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 4, 3, 5, 4, 1792, 1792, false);
        self.fill(world, clip, 1, 3, 4, 1, 4, 4, 1808, 1792, false);
        self.fill(world, clip, 3, 3, 4, 3, 4, 4, 1808, 1792, false);
        self.chest(world, clip, random, [3, 2, 3]);
        self.fill(world, clip, 0, 6, 0, 4, 6, 4, 1792, 1792, false);
        for i in (0..=4) {
            for j in (0..=4) {
                self.down(world, 1792, i, -1, j, clip);
            }
        }
    }
    pub(super) fn template_corridor3(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n = i32::from(rotate_metadata(114, (2) as u8, self.orientation));
        for i in (0..=9) {
            let mut n2 = std::cmp::max(1, 7 - i);
            let mut n3 = std::cmp::min(std::cmp::max(n2 + 5, 14 - i), 13);
            let mut n4 = i;
            self.fill(world, clip, 0, 0, n4, 4, n2, n4, 1792, 1792, false);
            self.fill(world, clip, 1, n2 + 1, n4, 3, n3 - 1, n4, 0, 0, false);
            if (i <= 6) {
                self.place(world, ((114 << 4) | (n) as u16), 1, n2 + 1, n4, clip);
                self.place(world, ((114 << 4) | (n) as u16), 2, n2 + 1, n4, clip);
                self.place(world, ((114 << 4) | (n) as u16), 3, n2 + 1, n4, clip);
            }
            self.fill(world, clip, 0, n3, n4, 4, n3, n4, 1792, 1792, false);
            self.fill(world, clip, 0, n2 + 1, n4, 0, n3 - 1, n4, 1792, 1792, false);
            self.fill(world, clip, 4, n2 + 1, n4, 4, n3 - 1, n4, 1792, 1792, false);
            if ((i & 1) == 0) {
                self.fill(world, clip, 0, n2 + 2, n4, 0, n2 + 3, n4, 1808, 1808, false);
                self.fill(world, clip, 4, n2 + 2, n4, 4, n2 + 3, n4, 1808, 1808, false);
            }
            for j in (0..=4) {
                self.down(world, 1792, j, -1, n4, clip);
            }
        }
    }
    pub(super) fn template_corridor4(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        self.fill(world, clip, 0, 0, 0, 8, 1, 8, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 8, 5, 8, 0, 0, false);
        self.fill(world, clip, 0, 6, 0, 8, 6, 5, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 0, 2, 5, 0, 1792, 1792, false);
        self.fill(world, clip, 6, 2, 0, 8, 5, 0, 1792, 1792, false);
        self.fill(world, clip, 1, 3, 0, 1, 4, 0, 1808, 1808, false);
        self.fill(world, clip, 7, 3, 0, 7, 4, 0, 1808, 1808, false);
        self.fill(world, clip, 0, 2, 4, 8, 2, 8, 1792, 1792, false);
        self.fill(world, clip, 1, 1, 4, 2, 2, 4, 0, 0, false);
        self.fill(world, clip, 6, 1, 4, 7, 2, 4, 0, 0, false);
        self.fill(world, clip, 0, 3, 8, 8, 3, 8, 1808, 1808, false);
        self.fill(world, clip, 0, 3, 6, 0, 3, 7, 1808, 1808, false);
        self.fill(world, clip, 8, 3, 6, 8, 3, 7, 1808, 1808, false);
        self.fill(world, clip, 0, 3, 4, 0, 5, 5, 1792, 1792, false);
        self.fill(world, clip, 8, 3, 4, 8, 5, 5, 1792, 1792, false);
        self.fill(world, clip, 1, 3, 5, 2, 5, 5, 1792, 1792, false);
        self.fill(world, clip, 6, 3, 5, 7, 5, 5, 1792, 1792, false);
        self.fill(world, clip, 1, 4, 5, 1, 5, 5, 1808, 1808, false);
        self.fill(world, clip, 7, 4, 5, 7, 5, 5, 1808, 1808, false);
        for i in (0..=5) {
            for j in (0..=8) {
                self.down(world, 1792, j, -1, i, clip);
            }
        }
    }
    pub(super) fn template_netherstalkroom(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        let mut n3: i32;
        let mut n4: i32;
        let mut n5: i32;
        self.fill(world, clip, 0, 3, 0, 12, 4, 12, 1792, 1792, false);
        self.fill(world, clip, 0, 5, 0, 12, 13, 12, 0, 0, false);
        self.fill(world, clip, 0, 5, 0, 1, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 11, 5, 0, 12, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 11, 4, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 8, 5, 11, 10, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 5, 9, 11, 7, 12, 12, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 0, 4, 12, 1, 1792, 1792, false);
        self.fill(world, clip, 8, 5, 0, 10, 12, 1, 1792, 1792, false);
        self.fill(world, clip, 5, 9, 0, 7, 12, 1, 1792, 1792, false);
        self.fill(world, clip, 2, 11, 2, 10, 12, 10, 1792, 1792, false);
        for n5 in (1..=11).step_by((2) as usize) {
            self.fill(world, clip, n5, 10, 0, n5, 11, 0, 1808, 1808, false);
            self.fill(world, clip, n5, 10, 12, n5, 11, 12, 1808, 1808, false);
            self.fill(world, clip, 0, 10, n5, 0, 11, n5, 1808, 1808, false);
            self.fill(world, clip, 12, 10, n5, 12, 11, n5, 1808, 1808, false);
            self.place(world, 1792, n5, 13, 0, clip);
            self.place(world, 1792, n5, 13, 12, clip);
            self.place(world, 1792, 0, 13, n5, clip);
            self.place(world, 1792, 12, 13, n5, clip);
            self.place(world, 1808, n5 + 1, 13, 0, clip);
            self.place(world, 1808, n5 + 1, 13, 12, clip);
            self.place(world, 1808, 0, 13, n5 + 1, clip);
            self.place(world, 1808, 12, 13, n5 + 1, clip);
        }
        self.place(world, 1808, 0, 13, 0, clip);
        self.place(world, 1808, 0, 13, 12, clip);
        self.place(world, 1808, 0, 13, 0, clip);
        self.place(world, 1808, 12, 13, 0, clip);
        for n5 in (3..=9).step_by((2) as usize) {
            self.fill(world, clip, 1, 7, n5, 1, 8, n5, 1808, 1808, false);
            self.fill(world, clip, 11, 7, n5, 11, 8, n5, 1808, 1808, false);
        }
        n5 = i32::from(rotate_metadata(114, (3) as u8, self.orientation));
        for n4 in (0..=6) {
            n3 = n4 + 4;
            for n2 in (5..=7) {
                self.place(world, ((114 << 4) | (n5) as u16), n2, 5 + n4, n3, clip);
            }
            if (5..=8).contains(&n3) {
                self.fill(world, clip, 5, 5, n3, 7, n4 + 4, n3, 1792, 1792, false);
            } else if (9..=10).contains(&n3) {
                self.fill(world, clip, 5, 8, n3, 7, n4 + 4, n3, 1792, 1792, false);
            }
            if n4 < 1 {
                continue;
            }
            self.fill(world, clip, 5, 6 + n4, n3, 7, 9 + n4, n3, 0, 0, false);
        }
        for n4 in (5..=7) {
            self.place(world, ((114 << 4) | (n5) as u16), n4, 12, 11, clip);
        }
        self.fill(world, clip, 5, 6, 7, 5, 7, 7, 1808, 1808, false);
        self.fill(world, clip, 7, 6, 7, 7, 7, 7, 1808, 1808, false);
        self.fill(world, clip, 5, 13, 12, 7, 13, 12, 0, 0, false);
        self.fill(world, clip, 2, 5, 2, 3, 5, 3, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 9, 3, 5, 10, 1792, 1792, false);
        self.fill(world, clip, 2, 5, 4, 2, 5, 8, 1792, 1792, false);
        self.fill(world, clip, 9, 5, 2, 10, 5, 3, 1792, 1792, false);
        self.fill(world, clip, 9, 5, 9, 10, 5, 10, 1792, 1792, false);
        self.fill(world, clip, 10, 5, 4, 10, 5, 8, 1792, 1792, false);
        n4 = i32::from(rotate_metadata(114, (0) as u8, self.orientation));
        n3 = i32::from(rotate_metadata(114, (1) as u8, self.orientation));
        self.place(world, ((114 << 4) | (n3) as u16), 4, 5, 2, clip);
        self.place(world, ((114 << 4) | (n3) as u16), 4, 5, 3, clip);
        self.place(world, ((114 << 4) | (n3) as u16), 4, 5, 9, clip);
        self.place(world, ((114 << 4) | (n3) as u16), 4, 5, 10, clip);
        self.place(world, ((114 << 4) | (n4) as u16), 8, 5, 2, clip);
        self.place(world, ((114 << 4) | (n4) as u16), 8, 5, 3, clip);
        self.place(world, ((114 << 4) | (n4) as u16), 8, 5, 9, clip);
        self.place(world, ((114 << 4) | (n4) as u16), 8, 5, 10, clip);
        self.fill(world, clip, 3, 4, 4, 4, 4, 8, 1408, 1408, false);
        self.fill(world, clip, 8, 4, 4, 9, 4, 8, 1408, 1408, false);
        self.fill(world, clip, 3, 5, 4, 4, 5, 8, 1840, 1840, false);
        self.fill(world, clip, 8, 5, 4, 9, 5, 8, 1840, 1840, false);
        self.fill(world, clip, 4, 2, 0, 8, 2, 12, 1792, 1792, false);
        self.fill(world, clip, 0, 2, 4, 12, 2, 8, 1792, 1792, false);
        self.fill(world, clip, 4, 0, 0, 8, 1, 3, 1792, 1792, false);
        self.fill(world, clip, 4, 0, 9, 8, 1, 12, 1792, 1792, false);
        self.fill(world, clip, 0, 0, 4, 3, 1, 8, 1792, 1792, false);
        self.fill(world, clip, 9, 0, 4, 12, 1, 8, 1792, 1792, false);
        for n2 in (4..=8) {
            for n in (0..=2) {
                self.down(world, 1792, n2, -1, n, clip);
                self.down(world, 1792, n2, -1, 12 - n, clip);
            }
        }
        for n2 in (0..=2) {
            for n in (4..=8) {
                self.down(world, 1792, n2, -1, n, clip);
                self.down(world, 1792, 12 - n2, -1, n, clip);
            }
        }
    }
    pub(super) fn template_end(
        &mut self,
        world: &mut impl FortressWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        let mut n3: i32;
        let mut random2 = JavaRandom::new(self.fill_seed as i64);
        for n3 in (0..=4) {
            for n2 in (3..=4) {
                n = random2.next_int(8);
                self.fill(world, clip, n3, n2, 0, n3, n2, n, 1792, 1792, false);
            }
        }
        n3 = random2.next_int(8);
        self.fill(world, clip, 0, 5, 0, 0, 5, n3, 1792, 1792, false);
        n3 = random2.next_int(8);
        self.fill(world, clip, 4, 5, 0, 4, 5, n3, 1792, 1792, false);
        for n3 in (0..=4) {
            n2 = random2.next_int(5);
            self.fill(world, clip, n3, 2, 0, n3, 2, n2, 1792, 1792, false);
        }
        for n3 in (0..=4) {
            for n2 in (0..=1) {
                n = random2.next_int(3);
                self.fill(world, clip, n3, n2, 0, n3, n2, n, 1792, 1792, false);
            }
        }
    }
}
