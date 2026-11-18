use super::Trader;
use crate::{
    gameplay::ai::goals::{Goals, Selector},
    vanilla::random::JavaRandom,
};

const TRADE: u16 = 0;
const WATCH_CUSTOMER: u16 = 1;
#[derive(Clone, Debug)]
pub struct MerchantGoals {
    tasks: Selector,
    look_time: i32,
}
impl Default for MerchantGoals {
    fn default() -> Self {
        let mut tasks = Selector::default();
        tasks.add(TRADE, 1, 5, true);
        tasks.add(WATCH_CUSTOMER, 1, 2, true);
        Self {
            tasks,
            look_time: 0,
        }
    }
}
#[derive(Clone, Copy)]
pub struct Conditions {
    pub alive: bool,
    pub grounded: bool,
    pub in_water: bool,
    pub velocity_changed: bool,
    pub customer_alive: bool,
    pub distance_squared: f64,
}
struct Owner<'a> {
    trader: &'a mut Trader,
    look_time: &'a mut i32,
    state: Conditions,
}
impl Goals for Owner<'_> {
    fn should_start(&mut self, action: u16) -> bool {
        match action {
            TRADE => {
                self.state.alive
                    && !self.state.in_water
                    && self.state.grounded
                    && !self.state.velocity_changed
                    && self.trader.customer.is_some()
                    && self.state.distance_squared <= 16.
            }
            WATCH_CUSTOMER => self.trader.customer.is_some(),
            _ => unreachable!(),
        }
    }
    fn should_continue(&mut self, action: u16) -> bool {
        match action {
            TRADE => self.should_start(TRADE),
            WATCH_CUSTOMER => {
                self.state.customer_alive
                    && self.state.distance_squared <= 64.
                    && *self.look_time > 0
            }
            _ => unreachable!(),
        }
    }
    fn start(&mut self, action: u16) {
        if action == WATCH_CUSTOMER {
            let mut random = JavaRandom::from_state(self.trader.career.random_state);
            *self.look_time = 40 + random.next_int(40);
            self.trader.career.random_state = random.state();
        }
    }
    fn reset(&mut self, action: u16) {
        if action == TRADE {
            self.trader.customer = None;
        }
    }
    fn update(&mut self, action: u16) {
        if action == WATCH_CUSTOMER {
            *self.look_time = self.look_time.wrapping_sub(1);
        }
    }
}
impl MerchantGoals {
    pub fn tick(&mut self, trader: &mut Trader, state: Conditions) -> bool {
        self.tasks.tick(&mut Owner {
            trader,
            look_time: &mut self.look_time,
            state,
        });
        self.tasks.active(TRADE)
    }
    pub fn watching(&self) -> bool {
        self.tasks.active(WATCH_CUSTOMER)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use std::io::Read;
    #[test]
    fn merchant_trade_and_watch_goal_cancellation_matches_original_13248_ticks() {
        let mut bytes = Vec::new();
        flate2::read::GzDecoder::new(
            include_bytes!("../tests/fixtures/vanilla189-villager-goals.json.gz").as_slice(),
        )
        .read_to_end(&mut bytes)
        .unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        let mut count = 0;
        for (i, case) in data.as_array().unwrap().iter().enumerate() {
            let mut trader = Trader::seeded(0, case[0].as_str().unwrap().parse().unwrap());
            let mut goals = MerchantGoals::default();
            for (t, frame) in case[1].as_array().unwrap().iter().enumerate() {
                match frame[4].as_i64().unwrap() {
                    1 => trader.customer = Some("Player".to_owned()),
                    -1 => trader.customer = None,
                    _ => {}
                }
                let distance = frame[3].as_f64().unwrap();
                let trade = goals.tick(
                    &mut trader,
                    Conditions {
                        alive: true,
                        grounded: frame[0].as_bool().unwrap(),
                        in_water: frame[1].as_bool().unwrap(),
                        velocity_changed: frame[2].as_bool().unwrap(),
                        customer_alive: true,
                        distance_squared: distance * distance,
                    },
                );
                let active = frame[5].as_array().unwrap();
                assert_eq!(
                    json!([
                        trade,
                        goals.watching(),
                        trader.customer.is_some(),
                        goals.look_time,
                        trader.career.random_state.to_string()
                    ]),
                    json!([
                        active.contains(&json!(0)),
                        active.contains(&json!(1)),
                        frame[6],
                        frame[7],
                        frame[8]
                    ]),
                    "merchant session{i}, tick{t}"
                );
                count += 1;
            }
        }
        assert_eq!(count, 13248);
    }
}
