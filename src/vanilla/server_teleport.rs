//! The server's S08 correction and the C03 admission stage before the live MP
//! update. Admitted packets still require server movement/collision processing.
use crate::protocol::Writer;
use anyhow::{ensure, Result};
use glam::DVec3;

#[derive(Clone, Copy, Debug)]
pub struct Location {
    pub position: DVec3,
    pub rotation: [f32; 2],
}
#[derive(Clone, Copy, Debug)]
pub struct Correction {
    pub location: Location,
    pub flags: u8,
}
impl Correction {
    pub fn validate(self) -> Result<()> {
        ensure!(
            self.flags <= 31
                && self.location.position.is_finite()
                && self.location.rotation.iter().all(|v| v.is_finite()),
            "Invalid S08 correction"
        );
        Ok(())
    }
    pub fn payload(self) -> Vec<u8> {
        let mut w = Writer::new();
        for v in self.location.position.to_array() {
            w.f64(v);
        }
        for v in self.location.rotation {
            w.f32(v);
        }
        w.u8(self.flags);
        w.0
    }
    pub fn resolve(self, current: Location) -> Location {
        let mut dest = self.location;
        for axis in 0..3 {
            if self.flags & (1 << axis) != 0 {
                dest.position[axis] += current.position[axis];
            }
        }
        for axis in 0..2 {
            if self.flags & (8 << axis) != 0 {
                dest.rotation[axis] += current.rotation[axis];
            }
            dest.rotation[axis] %= 360.;
        }
        dest
    }
}
#[derive(Clone, Copy, Debug)]
pub struct MovementPacket {
    pub position: Option<DVec3>,
    pub rotation: Option<[f32; 2]>,
}
#[derive(Clone, Copy, Debug)]
pub enum Admission {
    Pending,
    Admitted,
    Resend(Correction),
    Disconnect(&'static str),
}
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    last_position: DVec3,
    last_move_tick: i32,
    pub acknowledged: bool,
}
impl Default for Gate {
    fn default() -> Self {
        Self {
            last_position: DVec3::ZERO,
            last_move_tick: 0,
            acknowledged: true,
        }
    }
}
impl Gate {
    pub fn correct(&mut self, current: Location, correction: Correction) -> Location {
        let dest = correction.resolve(current);
        self.last_position = dest.position;
        self.acknowledged = false;
        dest
    }
    pub fn admit(
        &mut self,
        packet: MovementPacket,
        tick: i32,
        current: Location,
        conquered_end: bool,
    ) -> Admission {
        if packet.position.is_some_and(|p| !p.is_finite())
            || packet
                .rotation
                .is_some_and(|r| r.iter().any(|v| !v.is_finite()))
        {
            return Admission::Disconnect("Invalid move packet received");
        }
        if conquered_end {
            return Admission::Pending;
        }
        if let Some(position) = packet.position {
            let d = position - self.last_position;
            if !self.acknowledged && d.x * d.x + d.y * d.y + d.z * d.z < 0.25 {
                self.acknowledged = true;
            }
        }
        if self.acknowledged {
            self.last_move_tick = tick;
            return Admission::Admitted;
        }
        if tick.wrapping_sub(self.last_move_tick) > 20 {
            return Admission::Resend(Correction {
                location: Location {
                    position: self.last_position,
                    rotation: current.rotation,
                },
                flags: 0,
            });
        }
        Admission::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    #[derive(Deserialize)]
    struct Row {
        flags: u8,
        from: [String; 3],
        from_rot: [f32; 2],
        request: [String; 3],
        request_rot: [f32; 2],
        position: [String; 3],
        rotation: [f32; 2],
        sent: Vec<String>,
        #[serde(rename = "type")]
        kind: u8,
        packet: [String; 3],
        packet_rot: [u32; 2],
        now: i32,
        end: bool,
        admitted: bool,
        ack: bool,
        last_tick: i32,
        kick: Option<String>,
    }
    fn point(input: &[String; 3]) -> DVec3 {
        DVec3::from_array(std::array::from_fn(|i| {
            f64::from_bits(input[i].parse().unwrap())
        }))
    }
    #[test]
    fn original_s08_bytes_and_c03_teleport_acknowledgements_across_all_flags() {
        let rows: Vec<Row> = serde_json::from_str(include_str!(
            "../../tests/fixtures/vanilla189-server-teleport.json"
        ))
        .unwrap();
        assert_eq!(rows.len(), 640);
        for (i, r) in rows.into_iter().enumerate() {
            let mut gate = Gate::default();
            let from = Location {
                position: point(&r.from),
                rotation: r.from_rot,
            };
            let request = Correction {
                location: Location {
                    position: point(&r.request),
                    rotation: r.request_rot,
                },
                flags: r.flags,
            };
            let dst = gate.correct(from, request);
            assert_eq!(dst.position, point(&r.position), "case {i} position");
            assert_eq!(
                dst.rotation.map(f32::to_bits),
                r.rotation.map(f32::to_bits),
                "case {i} rotation"
            );
            let packet = MovementPacket {
                position: matches!(r.kind, 4 | 6).then(|| point(&r.packet)),
                rotation: matches!(r.kind, 5 | 6).then(|| r.packet_rot.map(f32::from_bits)),
            };
            let result = gate.admit(packet, r.now, dst, r.end);
            let mut packets = vec![request.payload()];
            if let Admission::Resend(s08) = result {
                packets.push(s08.payload());
            }
            let kick = if let Admission::Disconnect(reason) = result {
                Some(reason)
            } else {
                None
            };
            assert_eq!(kick, r.kick.as_deref(), "case {i} disconnect");
            assert_eq!(
                matches!(result, Admission::Admitted),
                r.admitted,
                "case {i} MP update"
            );
            assert_eq!(gate.acknowledged, r.ack, "case {i} gate");
            assert_eq!(gate.last_move_tick, r.last_tick, "case {i} timer");
            let bytes: Vec<_> = packets
                .iter()
                .map(|p| p.iter().map(|v| format!("{v:02x}")).collect::<String>())
                .collect();
            assert_eq!(bytes, r.sent, "case {i} S08 wire payload");
        }
    }
}
