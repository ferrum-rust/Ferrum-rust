// Original 1.8.9 trade registry, grouped by profession, career and level.
const CAREERS:&[&[&[&[Rule]]]]=&[
    &[
        &[
            &[Rule::Buy(296,0,18,22), Rule::Buy(392,0,15,19), Rule::Buy(391,0,15,19), Rule::Sell(297,0,-4,-2)],
            &[Rule::Buy(86,0,8,13), Rule::Sell(400,0,-3,-2)],
            &[Rule::Buy(103,0,7,12), Rule::Sell(260,0,-5,-7)],
            &[Rule::Sell(357,0,-6,-10), Rule::Sell(354,0,1,1)],
        ],
        &[
            &[Rule::Buy(287,0,15,20), Rule::Buy(263,0,16,24), Rule::Exchange(349,0,6,6,350,0,6,6)],
            &[Rule::Gear(346,0,7,8)],
        ],
        &[
            &[Rule::Buy(35,0,16,22), Rule::Sell(359,0,3,4)],
            &[Rule::Sell(35,0,1,2), Rule::Sell(35,1,1,2), Rule::Sell(35,2,1,2), Rule::Sell(35,3,1,2), Rule::Sell(35,4,1,2), Rule::Sell(35,5,1,2), Rule::Sell(35,6,1,2), Rule::Sell(35,7,1,2), Rule::Sell(35,8,1,2), Rule::Sell(35,9,1,2), Rule::Sell(35,10,1,2), Rule::Sell(35,11,1,2), Rule::Sell(35,12,1,2), Rule::Sell(35,13,1,2), Rule::Sell(35,14,1,2), Rule::Sell(35,15,1,2)],
        ],
        &[
            &[Rule::Buy(287,0,15,20), Rule::Sell(262,0,-12,-8)],
            &[Rule::Sell(261,0,2,3), Rule::Exchange(13,0,10,10,318,0,6,10)],
        ],
    ],
    &[
        &[
            &[Rule::Buy(339,0,24,36), Rule::Book],
            &[Rule::Buy(340,0,8,10), Rule::Sell(345,0,10,12), Rule::Sell(47,0,3,4)],
            &[Rule::Buy(387,0,2,2), Rule::Sell(347,0,10,12), Rule::Sell(20,0,-5,-3)],
            &[Rule::Book],
            &[Rule::Book],
            &[Rule::Sell(421,0,20,22)],
        ],
    ],
    &[
        &[
            &[Rule::Buy(367,0,36,40), Rule::Buy(266,0,8,10)],
            &[Rule::Sell(331,0,-4,-1), Rule::Sell(351,4,-2,-1)],
            &[Rule::Sell(381,0,7,11), Rule::Sell(89,0,-3,-1)],
            &[Rule::Sell(384,0,3,11)],
        ],
    ],
    &[
        &[
            &[Rule::Buy(263,0,16,24), Rule::Sell(306,0,4,6)],
            &[Rule::Buy(265,0,7,9), Rule::Sell(307,0,10,14)],
            &[Rule::Buy(264,0,3,4), Rule::Gear(311,0,16,19)],
            &[Rule::Sell(305,0,5,7), Rule::Sell(304,0,9,11), Rule::Sell(302,0,5,7), Rule::Sell(303,0,11,15)],
        ],
        &[
            &[Rule::Buy(263,0,16,24), Rule::Sell(258,0,6,8)],
            &[Rule::Buy(265,0,7,9), Rule::Gear(267,0,9,10)],
            &[Rule::Buy(264,0,3,4), Rule::Gear(276,0,12,15), Rule::Gear(279,0,9,12)],
        ],
        &[
            &[Rule::Buy(263,0,16,24), Rule::Gear(256,0,5,7)],
            &[Rule::Buy(265,0,7,9), Rule::Gear(257,0,9,11)],
            &[Rule::Buy(264,0,3,4), Rule::Gear(278,0,12,15)],
        ],
    ],
    &[
        &[
            &[Rule::Buy(319,0,14,18), Rule::Buy(365,0,14,18)],
            &[Rule::Buy(263,0,16,24), Rule::Sell(320,0,-7,-5), Rule::Sell(366,0,-8,-6)],
        ],
        &[
            &[Rule::Buy(334,0,9,12), Rule::Sell(300,0,2,4)],
            &[Rule::Gear(299,0,7,12)],
            &[Rule::Sell(329,0,8,10)],
        ],
    ],
];
const BOOK_POOL:&[(u16,i32,i32)]=&[(0,1,4),(1,1,4),(2,1,4),(3,1,4),(4,1,4),(5,1,3),(6,1,1),(7,1,3),(8,1,3),(16,1,5),(17,1,5),(18,1,5),(19,1,2),(20,1,2),(21,1,3),(32,1,5),(33,1,1),(34,1,3),(35,1,3),(48,1,5),(49,1,2),(50,1,1),(51,1,1),(61,1,3),(62,1,3)];
