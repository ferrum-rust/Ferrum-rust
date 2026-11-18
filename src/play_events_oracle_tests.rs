use super::*;

fn bytes(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}

#[test]
fn original_world_packets_and_serverbound_sign() {
    let cases: Vec<(String, i32, String)> = serde_json::from_str(include_str!(
        "../tests/fixtures/vanilla189-play-events.json"
    ))
    .unwrap();
    let pos = IVec3::new(-12345, 255, 56789);
    let mut ids = std::collections::BTreeSet::new();
    for (name, id, hex) in cases {
        let payload = bytes(&hex);
        if name == "update-sign" {
            let mut writer = crate::protocol::Writer::new();
            write_sign(
                &mut writer,
                pos.to_array(),
                ["1", "서버", "&<'>=", "\""].map(String::from),
            )
            .unwrap();
            assert_eq!(writer.0, payload);
            continue;
        }
        ids.insert(id);
        let decoded = decode(id, &mut Reader::new(&payload)).unwrap();
        match (name.as_str(), decoded) {
            ("bed", Update::Bed(16384, p))
            | ("sign", Update::Sign(p))
            | ("block", Update::BlockAction(p, 1, 5, 33))
            | ("break", Update::Break(333, p, 255)) => assert_eq!(p, pos),
            ("animation", Update::Animation(300, 5))
            | ("combat-start", Update::Combat(Combat::Start))
            | ("property", Update::Property(254, -32768, 32767))
            | ("combat-end", Update::Combat(Combat::End(301, -1)))
            | ("camera", Update::Camera(100000))
            | ("entity-nbt", Update::EntityNbt(193))
            | ("book", Update::Plugin(Plugin::Book))
            | ("other", Update::Plugin(Plugin::Other))
            | ("difficulty", Update::Difficulty(3)) => {}
            ("painting", Update::Painting(31, title, p, 3)) => {
                assert_eq!(title, "SkullAndRoses");
                assert_eq!(p, pos);
            }
            ("orb", Update::Orb(128, p, 2477)) => assert_eq!(p, [-65., 8191., 1.].map(|v| v / 32.)),
            ("global", Update::Global(170, 1, p)) => {
                assert_eq!(p, [65., -8191., -1.].map(|v| v / 32.))
            }
            (
                "explosion",
                Update::Explosion {
                    center,
                    strength,
                    blocks,
                    push,
                },
            ) => {
                assert_eq!(center, [-10.75, 22.75, -93.25]);
                assert_eq!(strength, 4.5);
                assert_eq!(push, [-0.25, 1.5, 0.75]);
                assert_eq!(
                    blocks,
                    [IVec3::new(-138, 149, -92), IVec3::new(117, -106, -94)]
                );
            }
            (kind @ ("map" | "icons"), Update::Map(m)) => {
                assert_eq!(m.icons, [[0xdf, 128, 127]]);
                if kind == "map" {
                    assert_eq!((m.id, m.scale, m.rect), (150, 3, [126, 125, 2, 3]));
                    assert_eq!(m.colors, [62, 63, 190, 191, 67, 68]);
                } else {
                    assert_eq!((m.id, m.scale, m.rect), (151, 0, [0; 4]));
                    assert!(m.colors.is_empty());
                }
            }
            ("brand", Update::Plugin(Plugin::Brand(brand))) => assert_eq!(brand, "원본 1.8.9"),
            ("offers", Update::Plugin(Plugin::Offers(513, offers))) => {
                assert_eq!(offers.len(), 2);
                let (first, second) = (&offers[0], &offers[1]);
                assert_eq!(
                    (
                        first.buy.id,
                        first.buy.count,
                        first.sell.id,
                        first.sell.damage
                    ),
                    (388, 7, 276, 23)
                );
                assert_eq!((first.uses, first.limit, first.disabled), (3, 3, true));
                assert!(first.second.is_none());
                assert!(first.sell.nbt.windows(3).any(|b| b == "칼".as_bytes()));
                assert_eq!(
                    (
                        second.buy.id,
                        second.second.as_ref().unwrap().id,
                        second.sell.id,
                        second.sell.count
                    ),
                    (264, 340, 388, 9)
                );
                assert_eq!((second.uses, second.limit, second.disabled), (1, 10, false));
            }
            ("stats", Update::Stats(mut values)) => {
                values.sort();
                assert_eq!(
                    values,
                    [
                        ("stat.deaths".to_owned(), 0),
                        ("stat.jump".to_owned(), 32769)
                    ]
                );
            }
            ("empty-stats", Update::Stats(values)) => assert!(values.is_empty()),
            ("combat-death", Update::Combat(Combat::Died(301, -1, message))) => {
                assert_eq!(message, "\"사망\"")
            }
            (name, value) => panic!("Unexpected {name} decoding: {value:?}"),
        }
        // The payload of an unknown plugin channel can legitimately be empty.
        let required = payload.len() - usize::from(name == "other");
        for len in 0..required {
            assert!(
                decode(id, &mut Reader::new(&payload[..len])).is_err(),
                "{name} accepted prefix of {len} bytes"
            );
        }
    }
    assert_eq!(ids.len(), 17);
}

#[test]
fn world_packets_reject_invalid_lengths_and_coordinates() {
    for (id, hex) in [
        (
            0x27,
            "00000000000000000000000040000000ffffffff000000000000000000000000",
        ),
        (
            0x27,
            "7fc0000000000000000000004000000000000000000000000000000000000000",
        ),
        (0x34, "00000002017f00020000"), // x=127, width=2 crosses the map.
        (0x34, "000000020100000100"),   // One color for a two-pixel rectangle.
        (0x42, "03"),
        (0x49, "000a0000ff"),
    ] {
        assert!(decode(id, &mut Reader::new(&bytes(hex))).is_err(), "{hex}");
    }
    let mut writer = crate::protocol::Writer::new();
    assert!(write_sign(
        &mut writer,
        [0; 3],
        ["&".repeat(64), String::new(), String::new(), String::new()]
    )
    .is_err());
}
