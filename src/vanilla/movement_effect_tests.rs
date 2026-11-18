use crate::vanilla::{item_food::Effect, potions::Effects};
#[test]
fn original_entityplayersp_effect_lifecycle_retains_expired_entries_until_remove_packet() {
    let rows: serde_json::Value = serde_json::from_str(include_str!(
        "../../tests/fixtures/vanilla189-player-effect-packets.json"
    ))
    .unwrap();
    let mut count = 0;
    for row in rows["sequences"].as_array().unwrap() {
        let mut effects = Effects::default();
        let id = row["id"].as_u64().unwrap() as u8;
        for step in row["steps"].as_array().unwrap() {
            match step["action"].as_str().unwrap() {
                "add" => {
                    let effect: Effect = serde_json::from_value(step["effect"].clone()).unwrap();
                    effects.add_client(effect).unwrap();
                }
                "tick" => effects.tick_client(),
                "remove" => effects.remove_client(id),
                _ => unreachable!(),
            }
            assert_eq!(
                serde_json::json!(effects.ordered().collect::<Vec<_>>()),
                step["effects"],
                "effect {id} step{count}"
            );
            // Actual EntityPlayerSP overrides heal/attack, and remote player
            // exhaustion is inert. The native client API likewise cannot
            // mutate server-owned health/food because it receives neither.
            assert_eq!(step["health"].as_f64().unwrap(), 10.);
            assert_eq!(step["exhaustion"].as_f64().unwrap(), 0.);
            count += 1;
        }
    }
    assert_eq!(count, 120);
}
