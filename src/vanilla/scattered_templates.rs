//! Original scattered-feature block template transcription.
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
    pub(super) fn template_desertpyramid(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        let mut n3: i32;
        let mut n4: i32;
        self.fill(
            world,
            clip,
            0,
            -4,
            0,
            self.size[0] - 1,
            0,
            self.size[2] - 1,
            384,
            384,
            false,
        );
        for n4 in (1..=9) {
            self.fill(
                world,
                clip,
                n4,
                n4,
                n4,
                self.size[0] - 1 - n4,
                n4,
                self.size[2] - 1 - n4,
                384,
                384,
                false,
            );
            self.fill(
                world,
                clip,
                n4 + 1,
                n4,
                n4 + 1,
                self.size[0] - 2 - n4,
                n4,
                self.size[2] - 2 - n4,
                0,
                0,
                false,
            );
        }
        for n4 in (0..self.size[0]) {
            for n3 in (0..self.size[2]) {
                n2 = -5;
                self.down(world, 384, n4, n2, n3, clip);
            }
        }
        n4 = i32::from(rotate_metadata(128, (3) as u8, self.orientation));
        n3 = i32::from(rotate_metadata(128, (2) as u8, self.orientation));
        n2 = i32::from(rotate_metadata(128, (0) as u8, self.orientation));
        let mut n5 = i32::from(rotate_metadata(128, (1) as u8, self.orientation));
        let mut n6 = 1;
        let mut n7 = 11;
        self.fill(world, clip, 0, 0, 0, 4, 9, 4, 384, 0, false);
        self.fill(world, clip, 1, 10, 1, 3, 10, 3, 384, 384, false);
        self.place(world, ((128 << 4) | (n4) as u16), 2, 10, 0, clip);
        self.place(world, ((128 << 4) | (n3) as u16), 2, 10, 4, clip);
        self.place(world, ((128 << 4) | (n2) as u16), 0, 10, 2, clip);
        self.place(world, ((128 << 4) | (n5) as u16), 4, 10, 2, clip);
        self.fill(
            world,
            clip,
            self.size[0] - 5,
            0,
            0,
            self.size[0] - 1,
            9,
            4,
            384,
            0,
            false,
        );
        self.fill(
            world,
            clip,
            self.size[0] - 4,
            10,
            1,
            self.size[0] - 2,
            10,
            3,
            384,
            384,
            false,
        );
        self.place(
            world,
            ((128 << 4) | (n4) as u16),
            self.size[0] - 3,
            10,
            0,
            clip,
        );
        self.place(
            world,
            ((128 << 4) | (n3) as u16),
            self.size[0] - 3,
            10,
            4,
            clip,
        );
        self.place(
            world,
            ((128 << 4) | (n2) as u16),
            self.size[0] - 5,
            10,
            2,
            clip,
        );
        self.place(
            world,
            ((128 << 4) | (n5) as u16),
            self.size[0] - 1,
            10,
            2,
            clip,
        );
        self.fill(world, clip, 8, 0, 0, 12, 4, 4, 384, 0, false);
        self.fill(world, clip, 9, 1, 0, 11, 3, 4, 0, 0, false);
        self.place(world, ((24 << 4) | (2) as u16), 9, 1, 1, clip);
        self.place(world, ((24 << 4) | (2) as u16), 9, 2, 1, clip);
        self.place(world, ((24 << 4) | (2) as u16), 9, 3, 1, clip);
        self.place(world, ((24 << 4) | (2) as u16), 10, 3, 1, clip);
        self.place(world, ((24 << 4) | (2) as u16), 11, 3, 1, clip);
        self.place(world, ((24 << 4) | (2) as u16), 11, 2, 1, clip);
        self.place(world, ((24 << 4) | (2) as u16), 11, 1, 1, clip);
        self.fill(world, clip, 4, 1, 1, 8, 3, 3, 384, 0, false);
        self.fill(world, clip, 4, 1, 2, 8, 2, 2, 0, 0, false);
        self.fill(world, clip, 12, 1, 1, 16, 3, 3, 384, 0, false);
        self.fill(world, clip, 12, 1, 2, 16, 2, 2, 0, 0, false);
        self.fill(
            world,
            clip,
            5,
            4,
            5,
            self.size[0] - 6,
            4,
            self.size[2] - 6,
            384,
            384,
            false,
        );
        self.fill(world, clip, 9, 4, 9, 11, 4, 11, 0, 0, false);
        self.fill(
            world,
            clip,
            8,
            1,
            8,
            8,
            3,
            8,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            12,
            1,
            8,
            12,
            3,
            8,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            8,
            1,
            12,
            8,
            3,
            12,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            12,
            1,
            12,
            12,
            3,
            12,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(world, clip, 1, 1, 5, 4, 4, 11, 384, 384, false);
        self.fill(
            world,
            clip,
            self.size[0] - 5,
            1,
            5,
            self.size[0] - 2,
            4,
            11,
            384,
            384,
            false,
        );
        self.fill(world, clip, 6, 7, 9, 6, 7, 11, 384, 384, false);
        self.fill(
            world,
            clip,
            self.size[0] - 7,
            7,
            9,
            self.size[0] - 7,
            7,
            11,
            384,
            384,
            false,
        );
        self.fill(
            world,
            clip,
            5,
            5,
            9,
            5,
            7,
            11,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            self.size[0] - 6,
            5,
            9,
            self.size[0] - 6,
            7,
            11,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.place(world, 0, 5, 5, 10, clip);
        self.place(world, 0, 5, 6, 10, clip);
        self.place(world, 0, 6, 6, 10, clip);
        self.place(world, 0, self.size[0] - 6, 5, 10, clip);
        self.place(world, 0, self.size[0] - 6, 6, 10, clip);
        self.place(world, 0, self.size[0] - 7, 6, 10, clip);
        self.fill(world, clip, 2, 4, 4, 2, 6, 4, 0, 0, false);
        self.fill(
            world,
            clip,
            self.size[0] - 3,
            4,
            4,
            self.size[0] - 3,
            6,
            4,
            0,
            0,
            false,
        );
        self.place(world, ((128 << 4) | (n4) as u16), 2, 4, 5, clip);
        self.place(world, ((128 << 4) | (n4) as u16), 2, 3, 4, clip);
        self.place(
            world,
            ((128 << 4) | (n4) as u16),
            self.size[0] - 3,
            4,
            5,
            clip,
        );
        self.place(
            world,
            ((128 << 4) | (n4) as u16),
            self.size[0] - 3,
            3,
            4,
            clip,
        );
        self.fill(world, clip, 1, 1, 3, 2, 2, 3, 384, 384, false);
        self.fill(
            world,
            clip,
            self.size[0] - 3,
            1,
            3,
            self.size[0] - 2,
            2,
            3,
            384,
            384,
            false,
        );
        self.place(world, 2051, 1, 1, 2, clip);
        self.place(world, 2051, self.size[0] - 2, 1, 2, clip);
        self.place(world, ((44 << 4) | (1) as u16), 1, 2, 2, clip);
        self.place(
            world,
            ((44 << 4) | (1) as u16),
            self.size[0] - 2,
            2,
            2,
            clip,
        );
        self.place(world, ((128 << 4) | (n5) as u16), 2, 1, 2, clip);
        self.place(
            world,
            ((128 << 4) | (n2) as u16),
            self.size[0] - 3,
            1,
            2,
            clip,
        );
        self.fill(world, clip, 4, 3, 5, 4, 3, 18, 384, 384, false);
        self.fill(
            world,
            clip,
            self.size[0] - 5,
            3,
            5,
            self.size[0] - 5,
            3,
            17,
            384,
            384,
            false,
        );
        self.fill(world, clip, 3, 1, 5, 4, 2, 16, 0, 0, false);
        self.fill(
            world,
            clip,
            self.size[0] - 6,
            1,
            5,
            self.size[0] - 5,
            2,
            16,
            0,
            0,
            false,
        );
        for n in (5..=17).step_by((2) as usize) {
            self.place(world, ((24 << 4) | (2) as u16), 4, 1, n, clip);
            self.place(world, ((24 << 4) | (1) as u16), 4, 2, n, clip);
            self.place(
                world,
                ((24 << 4) | (2) as u16),
                self.size[0] - 5,
                1,
                n,
                clip,
            );
            self.place(
                world,
                ((24 << 4) | (1) as u16),
                self.size[0] - 5,
                2,
                n,
                clip,
            );
        }
        self.place(world, ((159 << 4) | (n6) as u16), 10, 0, 7, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 10, 0, 8, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 9, 0, 9, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 11, 0, 9, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 8, 0, 10, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 12, 0, 10, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 7, 0, 10, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 13, 0, 10, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 9, 0, 11, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 11, 0, 11, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 10, 0, 12, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 10, 0, 13, clip);
        self.place(world, ((159 << 4) | (n7) as u16), 10, 0, 10, clip);
        for n in (0..=self.size[0] - 1).step_by((self.size[0] - 1) as usize) {
            self.place(world, ((24 << 4) | (2) as u16), n, 2, 1, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 2, 2, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 2, 3, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 3, 1, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 3, 2, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 3, 3, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 4, 1, clip);
            self.place(world, ((24 << 4) | (1) as u16), n, 4, 2, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 4, 3, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 5, 1, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 5, 2, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 5, 3, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 6, 1, clip);
            self.place(world, ((24 << 4) | (1) as u16), n, 6, 2, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 6, 3, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 7, 1, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 7, 2, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 7, 3, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 8, 1, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 8, 2, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 8, 3, clip);
        }
        for n in (2..=self.size[0] - 3).step_by((self.size[0] - 3 - 2) as usize) {
            self.place(world, ((24 << 4) | (2) as u16), n - 1, 2, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 2, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n + 1, 2, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n - 1, 3, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 3, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n + 1, 3, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n - 1, 4, 0, clip);
            self.place(world, ((24 << 4) | (1) as u16), n, 4, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n + 1, 4, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n - 1, 5, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 5, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n + 1, 5, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n - 1, 6, 0, clip);
            self.place(world, ((24 << 4) | (1) as u16), n, 6, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n + 1, 6, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n - 1, 7, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n, 7, 0, clip);
            self.place(world, ((159 << 4) | (n6) as u16), n + 1, 7, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n - 1, 8, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n, 8, 0, clip);
            self.place(world, ((24 << 4) | (2) as u16), n + 1, 8, 0, clip);
        }
        self.fill(
            world,
            clip,
            8,
            4,
            0,
            12,
            6,
            0,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.place(world, 0, 8, 6, 0, clip);
        self.place(world, 0, 12, 6, 0, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 9, 5, 0, clip);
        self.place(world, ((24 << 4) | (1) as u16), 10, 5, 0, clip);
        self.place(world, ((159 << 4) | (n6) as u16), 11, 5, 0, clip);
        self.fill(
            world,
            clip,
            8,
            -14,
            8,
            12,
            -11,
            12,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            8,
            -10,
            8,
            12,
            -10,
            12,
            ((24 << 4) | (1) as u16),
            ((24 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            8,
            -9,
            8,
            12,
            -9,
            12,
            ((24 << 4) | (2) as u16),
            ((24 << 4) | (2) as u16),
            false,
        );
        self.fill(world, clip, 8, -8, 8, 12, -1, 12, 384, 384, false);
        self.fill(world, clip, 9, -11, 9, 11, -1, 11, 0, 0, false);
        self.place(world, 1120, 10, -11, 10, clip);
        self.fill(world, clip, 9, -13, 9, 11, -13, 11, 736, 0, false);
        self.place(world, 0, 8, -11, 10, clip);
        self.place(world, 0, 8, -10, 10, clip);
        self.place(world, ((24 << 4) | (1) as u16), 7, -10, 10, clip);
        self.place(world, ((24 << 4) | (2) as u16), 7, -11, 10, clip);
        self.place(world, 0, 12, -11, 10, clip);
        self.place(world, 0, 12, -10, 10, clip);
        self.place(world, ((24 << 4) | (1) as u16), 13, -10, 10, clip);
        self.place(world, ((24 << 4) | (2) as u16), 13, -11, 10, clip);
        self.place(world, 0, 10, -11, 8, clip);
        self.place(world, 0, 10, -10, 8, clip);
        self.place(world, ((24 << 4) | (1) as u16), 10, -10, 7, clip);
        self.place(world, ((24 << 4) | (2) as u16), 10, -11, 7, clip);
        self.place(world, 0, 10, -11, 12, clip);
        self.place(world, 0, 10, -10, 12, clip);
        self.place(world, ((24 << 4) | (1) as u16), 10, -10, 13, clip);
        self.place(world, ((24 << 4) | (2) as u16), 10, -11, 13, clip);
        for (index, dx, dz) in [(2, 0, -2), (3, 2, 0), (0, 0, 2), (1, -2, 0)] {
            self.temple_chest(world, clip, random, index, [10 + dx, -11, 10 + dz]);
        }
    }
    pub(super) fn template_junglepyramid(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2 = i32::from(rotate_metadata(67, (3) as u8, self.orientation));
        let mut n3 = i32::from(rotate_metadata(67, (2) as u8, self.orientation));
        let mut n4 = i32::from(rotate_metadata(67, (0) as u8, self.orientation));
        let mut n5 = i32::from(rotate_metadata(67, (1) as u8, self.orientation));
        self.stones(
            world,
            clip,
            0,
            -4,
            0,
            self.size[0] - 1,
            0,
            self.size[2] - 1,
            false,
            random,
        );
        self.stones(world, clip, 2, 1, 2, 9, 2, 2, false, random);
        self.stones(world, clip, 2, 1, 12, 9, 2, 12, false, random);
        self.stones(world, clip, 2, 1, 3, 2, 2, 11, false, random);
        self.stones(world, clip, 9, 1, 3, 9, 2, 11, false, random);
        self.stones(world, clip, 1, 3, 1, 10, 6, 1, false, random);
        self.stones(world, clip, 1, 3, 13, 10, 6, 13, false, random);
        self.stones(world, clip, 1, 3, 2, 1, 6, 12, false, random);
        self.stones(world, clip, 10, 3, 2, 10, 6, 12, false, random);
        self.stones(world, clip, 2, 3, 2, 9, 3, 12, false, random);
        self.stones(world, clip, 2, 6, 2, 9, 6, 12, false, random);
        self.stones(world, clip, 3, 7, 3, 8, 7, 11, false, random);
        self.stones(world, clip, 4, 8, 4, 7, 8, 10, false, random);
        self.air(world, clip, 3, 1, 3, 8, 2, 11);
        self.air(world, clip, 4, 3, 6, 7, 3, 9);
        self.air(world, clip, 2, 4, 2, 9, 5, 12);
        self.air(world, clip, 4, 6, 5, 7, 6, 9);
        self.air(world, clip, 5, 7, 6, 6, 7, 8);
        self.air(world, clip, 5, 1, 2, 6, 2, 2);
        self.air(world, clip, 5, 2, 12, 6, 2, 12);
        self.air(world, clip, 5, 5, 1, 6, 5, 1);
        self.air(world, clip, 5, 5, 13, 6, 5, 13);
        self.place(world, 0, 1, 5, 5, clip);
        self.place(world, 0, 10, 5, 5, clip);
        self.place(world, 0, 1, 5, 9, clip);
        self.place(world, 0, 10, 5, 9, clip);
        for n in (0..=14).step_by((14) as usize) {
            self.stones(world, clip, 2, 4, n, 2, 5, n, false, random);
            self.stones(world, clip, 4, 4, n, 4, 5, n, false, random);
            self.stones(world, clip, 7, 4, n, 7, 5, n, false, random);
            self.stones(world, clip, 9, 4, n, 9, 5, n, false, random);
        }
        self.stones(world, clip, 5, 6, 0, 6, 6, 0, false, random);
        for n in (0..=11).step_by((11) as usize) {
            for i in (2..=12).step_by((2) as usize) {
                self.stones(world, clip, n, 4, i, n, 5, i, false, random);
            }
            self.stones(world, clip, n, 6, 5, n, 6, 5, false, random);
            self.stones(world, clip, n, 6, 9, n, 6, 9, false, random);
        }
        self.stones(world, clip, 2, 7, 2, 2, 9, 2, false, random);
        self.stones(world, clip, 9, 7, 2, 9, 9, 2, false, random);
        self.stones(world, clip, 2, 7, 12, 2, 9, 12, false, random);
        self.stones(world, clip, 9, 7, 12, 9, 9, 12, false, random);
        self.stones(world, clip, 4, 9, 4, 4, 9, 4, false, random);
        self.stones(world, clip, 7, 9, 4, 7, 9, 4, false, random);
        self.stones(world, clip, 4, 9, 10, 4, 9, 10, false, random);
        self.stones(world, clip, 7, 9, 10, 7, 9, 10, false, random);
        self.stones(world, clip, 5, 9, 7, 6, 9, 7, false, random);
        self.place(world, ((67 << 4) | (n2) as u16), 5, 9, 6, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 6, 9, 6, clip);
        self.place(world, ((67 << 4) | (n3) as u16), 5, 9, 8, clip);
        self.place(world, ((67 << 4) | (n3) as u16), 6, 9, 8, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 4, 0, 0, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 5, 0, 0, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 6, 0, 0, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 7, 0, 0, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 4, 1, 8, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 4, 2, 9, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 4, 3, 10, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 7, 1, 8, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 7, 2, 9, clip);
        self.place(world, ((67 << 4) | (n2) as u16), 7, 3, 10, clip);
        self.stones(world, clip, 4, 1, 9, 4, 1, 9, false, random);
        self.stones(world, clip, 7, 1, 9, 7, 1, 9, false, random);
        self.stones(world, clip, 4, 1, 10, 7, 2, 10, false, random);
        self.stones(world, clip, 5, 4, 5, 6, 4, 5, false, random);
        self.place(world, ((67 << 4) | (n4) as u16), 4, 4, 5, clip);
        self.place(world, ((67 << 4) | (n5) as u16), 7, 4, 5, clip);
        for n in (0..4) {
            self.place(world, ((67 << 4) | (n3) as u16), 5, 0 - n, 6 + n, clip);
            self.place(world, ((67 << 4) | (n3) as u16), 6, 0 - n, 6 + n, clip);
            self.air(world, clip, 5, 0 - n, 7 + n, 6, 0 - n, 9 + n);
        }
        self.air(world, clip, 1, -3, 12, 10, -1, 13);
        self.air(world, clip, 1, -3, 1, 3, -1, 13);
        self.air(world, clip, 1, -3, 1, 9, -1, 5);
        for n in (1..=13).step_by((2) as usize) {
            self.stones(world, clip, 1, -3, n, 1, -2, n, false, random);
        }
        for n in (2..=12).step_by((2) as usize) {
            self.stones(world, clip, 1, -1, n, 3, -1, n, false, random);
        }
        self.stones(world, clip, 2, -2, 1, 5, -2, 1, false, random);
        self.stones(world, clip, 7, -2, 1, 9, -2, 1, false, random);
        self.stones(world, clip, 6, -3, 1, 6, -3, 1, false, random);
        self.stones(world, clip, 6, -1, 1, 6, -1, 1, false, random);
        self.place(
            world,
            ((131 << 4) | (i32::from(rotate_metadata(131, (3) as u8, self.orientation))) as u16)
                | 4,
            1,
            -3,
            8,
            clip,
        );
        self.place(
            world,
            ((131 << 4) | (i32::from(rotate_metadata(131, (1) as u8, self.orientation))) as u16)
                | 4,
            4,
            -3,
            8,
            clip,
        );
        self.place(world, 2112 | 4, 2, -3, 8, clip);
        self.place(world, 2112 | 4, 3, -3, 8, clip);
        self.place(world, 880, 5, -3, 7, clip);
        self.place(world, 880, 5, -3, 6, clip);
        self.place(world, 880, 5, -3, 5, clip);
        self.place(world, 880, 5, -3, 4, clip);
        self.place(world, 880, 5, -3, 3, clip);
        self.place(world, 880, 5, -3, 2, clip);
        self.place(world, 880, 5, -3, 1, clip);
        self.place(world, 880, 4, -3, 1, clip);
        self.place(world, 768, 3, -3, 1, clip);
        self.trap(world, clip, random, 2, [3, -2, 1], 2);
        self.place(world, ((106 << 4) | (15) as u16), 3, -2, 2, clip);
        self.place(
            world,
            ((131 << 4) | (i32::from(rotate_metadata(131, (2) as u8, self.orientation))) as u16)
                | 4,
            7,
            -3,
            1,
            clip,
        );
        self.place(
            world,
            ((131 << 4) | (i32::from(rotate_metadata(131, (0) as u8, self.orientation))) as u16)
                | 4,
            7,
            -3,
            5,
            clip,
        );
        self.place(world, 2112 | 4, 7, -3, 2, clip);
        self.place(world, 2112 | 4, 7, -3, 3, clip);
        self.place(world, 2112 | 4, 7, -3, 4, clip);
        self.place(world, 880, 8, -3, 6, clip);
        self.place(world, 880, 9, -3, 6, clip);
        self.place(world, 880, 9, -3, 5, clip);
        self.place(world, 768, 9, -3, 4, clip);
        self.place(world, 880, 9, -2, 4, clip);
        self.trap(world, clip, random, 3, [9, -2, 3], 4);
        self.place(world, ((106 << 4) | (15) as u16), 8, -1, 3, clip);
        self.place(world, ((106 << 4) | (15) as u16), 8, -2, 3, clip);
        self.temple_chest(world, clip, random, 0, [8, -3, 3]);
        self.place(world, 768, 9, -3, 2, clip);
        self.place(world, 768, 8, -3, 1, clip);
        self.place(world, 768, 4, -3, 5, clip);
        self.place(world, 768, 5, -2, 5, clip);
        self.place(world, 768, 5, -1, 5, clip);
        self.place(world, 768, 6, -3, 5, clip);
        self.place(world, 768, 7, -2, 5, clip);
        self.place(world, 768, 7, -1, 5, clip);
        self.place(world, 768, 8, -3, 5, clip);
        self.stones(world, clip, 9, -1, 1, 9, -1, 5, false, random);
        self.air(world, clip, 8, -3, 8, 10, -1, 10);
        self.place(world, ((98 << 4) | (3) as u16), 8, -2, 11, clip);
        self.place(world, ((98 << 4) | (3) as u16), 9, -2, 11, clip);
        self.place(world, ((98 << 4) | (3) as u16), 10, -2, 11, clip);
        self.place(
            world,
            ((69 << 4)
                | (lever((i32::from(rotate_metadata(69, (2) as u8, self.orientation))) as u8))
                    as u16),
            8,
            -2,
            12,
            clip,
        );
        self.place(
            world,
            ((69 << 4)
                | (lever((i32::from(rotate_metadata(69, (2) as u8, self.orientation))) as u8))
                    as u16),
            9,
            -2,
            12,
            clip,
        );
        self.place(
            world,
            ((69 << 4)
                | (lever((i32::from(rotate_metadata(69, (2) as u8, self.orientation))) as u8))
                    as u16),
            10,
            -2,
            12,
            clip,
        );
        self.stones(world, clip, 8, -3, 8, 8, -3, 10, false, random);
        self.stones(world, clip, 10, -3, 8, 10, -3, 10, false, random);
        self.place(world, 768, 10, -2, 9, clip);
        self.place(world, 880, 8, -2, 9, clip);
        self.place(world, 880, 8, -2, 10, clip);
        self.place(world, 880, 10, -1, 9, clip);
        self.place(world, ((29 << 4) | (1) as u16), 9, -2, 8, clip);
        self.place(
            world,
            ((29 << 4) | (i32::from(rotate_metadata(29, (4) as u8, self.orientation))) as u16),
            10,
            -2,
            8,
            clip,
        );
        self.place(
            world,
            ((29 << 4) | (i32::from(rotate_metadata(29, (4) as u8, self.orientation))) as u16),
            10,
            -1,
            8,
            clip,
        );
        self.place(
            world,
            ((93 << 4) | (i32::from(rotate_metadata(93, (2) as u8, self.orientation))) as u16),
            10,
            -2,
            10,
            clip,
        );
        self.temple_chest(world, clip, random, 1, [9, -3, 10]);
    }
    pub(super) fn template_swamphut(
        &mut self,
        world: &mut impl ScatteredWorld,
        clip: BoundingBox,
        random: &mut JavaRandom,
    ) {
        let mut n: i32;
        let mut n2: i32;
        let mut n3: i32;
        self.fill(
            world,
            clip,
            1,
            1,
            1,
            5,
            1,
            7,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            1,
            4,
            2,
            5,
            4,
            7,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            2,
            1,
            0,
            4,
            1,
            0,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            2,
            2,
            2,
            3,
            3,
            2,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            1,
            2,
            3,
            1,
            3,
            6,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            5,
            2,
            3,
            5,
            3,
            6,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            2,
            2,
            7,
            4,
            3,
            7,
            ((5 << 4) | (1) as u16),
            ((5 << 4) | (1) as u16),
            false,
        );
        self.fill(world, clip, 1, 0, 2, 1, 3, 2, 272, 272, false);
        self.fill(world, clip, 5, 0, 2, 5, 3, 2, 272, 272, false);
        self.fill(world, clip, 1, 0, 7, 1, 3, 7, 272, 272, false);
        self.fill(world, clip, 5, 0, 7, 5, 3, 7, 272, 272, false);
        self.place(world, 1360, 2, 3, 2, clip);
        self.place(world, 1360, 3, 3, 7, clip);
        self.place(world, 0, 1, 3, 4, clip);
        self.place(world, 0, 5, 3, 4, clip);
        self.place(world, 0, 5, 3, 5, clip);
        self.place(world, 2240, 1, 3, 5, clip);
        self.place(world, 928, 3, 2, 6, clip);
        self.place(world, 1888, 4, 2, 6, clip);
        self.place(world, 1360, 1, 2, 1, clip);
        self.place(world, 1360, 5, 2, 1, clip);
        let mut n4 = i32::from(rotate_metadata(53, (3) as u8, self.orientation));
        let mut n5 = i32::from(rotate_metadata(53, (1) as u8, self.orientation));
        let mut n6 = i32::from(rotate_metadata(53, (0) as u8, self.orientation));
        let mut n7 = i32::from(rotate_metadata(53, (2) as u8, self.orientation));
        self.fill(
            world,
            clip,
            0,
            4,
            1,
            6,
            4,
            1,
            ((134 << 4) | (n4) as u16),
            ((134 << 4) | (n4) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            0,
            4,
            2,
            0,
            4,
            7,
            ((134 << 4) | (n6) as u16),
            ((134 << 4) | (n6) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            6,
            4,
            2,
            6,
            4,
            7,
            ((134 << 4) | (n5) as u16),
            ((134 << 4) | (n5) as u16),
            false,
        );
        self.fill(
            world,
            clip,
            0,
            4,
            8,
            6,
            4,
            8,
            ((134 << 4) | (n7) as u16),
            ((134 << 4) | (n7) as u16),
            false,
        );
        for n3 in (2..=7).step_by((5) as usize) {
            for n2 in (1..=5).step_by((4) as usize) {
                self.down(world, 272, n2, -1, n3, clip);
            }
        }
        self.spawn_witch(world, clip);
    }
}
