//! Explicit, non-retried timer commands and read-only AJTM status.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Status {
    pub mode: u8,
    pub running: bool,
    pub done: bool,
    pub elapsed: u32,
    pub duration: u32,
}
impl Status {
    pub fn parse(r: &[u8]) -> Result<Self> {
        ensure!(
            r.len() == 65 && r[..7] == [0, 0x2c, b'A', b'J', b'T', b'M', 1],
            "Нужна прошивка с автономными таймерами"
        );
        let b = &r[1..];
        ensure!(
            b[..8].iter().fold(0u8, |a, &v| a.wrapping_add(v)) == 255
                && b[6] == 0
                && b[63] & 128 != 0
                && b[8] <= 2
                && b[9] <= 1
                && b[10] <= 1
                && b[11] == 0
                && b[20..63].iter().all(|&v| v == 0),
            "Повреждён ответ таймера"
        );
        let elapsed = u32::from_le_bytes(b[12..16].try_into()?);
        let duration = u32::from_le_bytes(b[16..20].try_into()?);
        ensure!(
            b[10] == u8::from(b[8] == 1 && elapsed == duration)
                && !(b[10] == 1 && b[9] == 1)
                && (b[8] != 0 || (elapsed == 0 && b[9] == 0)),
            "Неверное состояние таймера"
        );
        ensure!(
            if b[8] == 1 {
                (1..=10800).contains(&duration) && elapsed <= duration
            } else {
                duration == 0
            },
            "Неверная длительность таймера"
        );
        Ok(Self {
            mode: b[8],
            running: b[9] != 0,
            done: b[10] != 0,
            elapsed,
            duration,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timer_reply_checks_identity_checksum_and_state_invariants() {
        let mut r = [0; 65];
        r[..7].copy_from_slice(&[0, 0x2c, b'A', b'J', b'T', b'M', 1]);
        r[8] = 255 - r[1..8].iter().fold(0u8, |a, &v| a.wrapping_add(v));
        r[64] = 255;
        assert_eq!(Status::parse(&r).unwrap().mode, 0);
        r[9] = 1;
        r[10] = 1;
        r[13..17].copy_from_slice(&45u32.to_le_bytes());
        r[17..21].copy_from_slice(&1500u32.to_le_bytes());
        let s = Status::parse(&r).unwrap();
        assert!(s.running);
        assert_eq!(s.elapsed, 45);
        for at in [2, 8, 12, 22] {
            let mut bad = r;
            bad[at] ^= 1;
            assert!(Status::parse(&bad).is_err(), "byte{at}");
        }
        r[11] = 1;
        assert!(Status::parse(&r).is_err());
        assert!(Status::parse(&r[..64]).is_err());
    }
}
