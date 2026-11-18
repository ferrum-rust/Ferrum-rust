use super::*;
use crate::protocol::Reader;
use serde_json::{json, Value};
fn bits(value: &Value) -> u64 {
    u64::from_str_radix(value.as_str().unwrap(), 16).unwrap()
}
fn uuid(value: &Value) -> [u64; 2] {
    let s = value.as_str().unwrap().replace('-', "");
    [
        u64::from_str_radix(&s[..16], 16).unwrap(),
        u64::from_str_radix(&s[16..], 16).unwrap(),
    ]
}
fn expect_modifier(actual: &Modifier, expected: &Value) {
    assert_eq!(actual.uuid, uuid(&expected["uuid"]));
    assert_eq!(actual.amount.to_bits(), bits(&expected["amount"]));
    assert_eq!(
        actual.operation as u64,
        expected["operation"].as_u64().unwrap()
    );
}
#[test]
fn original_s20_197_snapshots_match_operation_order_sprint_uuid_and_errors() {
    let data: Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-movement-attributes.json"
    ))
    .unwrap();
    let mut count = 0;
    for (sequence, rows) in data.as_array().unwrap().iter().enumerate() {
        let mut attribute = Attribute::default();
        for (step, row) in rows.as_array().unwrap().iter().enumerate() {
            count += 1;
            let s = row["packet"].as_str().unwrap();
            let bytes = (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let read = read_properties(&mut Reader::new(&bytes));
            assert_eq!(
                read.is_err(),
                row["read_error"].as_bool().unwrap(),
                "read sequence={sequence} step={step}"
            );
            let Ok((id, snapshots)) = read else {
                continue;
            };
            assert_eq!(id as i64, row["entity_id"].as_i64().unwrap());
            assert_eq!(snapshots.len(), row["snapshots"].as_array().unwrap().len());
            for (snapshot, expected) in snapshots.iter().zip(row["snapshots"].as_array().unwrap()) {
                assert_eq!(json!(snapshot.name), expected["name"]);
                assert_eq!(snapshot.base.to_bits(), bits(&expected["base"]));
                assert_eq!(
                    snapshot.modifiers.len(),
                    expected["modifiers"].as_array().unwrap().len()
                );
                for (m, e) in snapshot
                    .modifiers
                    .iter()
                    .zip(expected["modifiers"].as_array().unwrap())
                {
                    expect_modifier(m, e);
                }
            }
            let apply = snapshots.iter().try_for_each(|s| attribute.replace(s));
            assert_eq!(
                apply.is_err(),
                row["apply_error"].as_bool().unwrap(),
                "apply sequence={sequence} step={step}"
            );
            if apply.is_err() {
                continue;
            }
            assert_eq!(
                attribute.value().to_bits(),
                bits(&row["value"]),
                "value sequence={sequence} step={step}"
            );
            for op in 0..3 {
                let actual = attribute.ordered(op);
                let expected = row["ordered"][op].as_array().unwrap();
                assert_eq!(actual.len(), expected.len());
                for (a, e) in actual.iter().zip(expected) {
                    expect_modifier(a, e);
                }
            }
            if let Some(sprint) = row["sprint"].as_bool() {
                attribute.set_modifier(SPRINT, sprint.then_some(0.3f32 as f64));
                assert_eq!(
                    attribute.value().to_bits(),
                    bits(&row["after_sprint"]),
                    "sprint sequence={sequence} step={step}"
                );
            }
        }
    }
    assert_eq!(count, 197);
}
