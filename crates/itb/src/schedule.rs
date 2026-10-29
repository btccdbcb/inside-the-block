//! Leader schedule: the 4-slot turns of each target, and the leader of every slot around them
//! (for the preceding leader at the hand-over and for attribution checks).

use crate::rpc::Rpc;
use anyhow::{anyhow, Result};
use std::collections::BTreeMap;

pub const TURN_SLOTS: u64 = 4;

pub fn turn_start(slot: u64) -> u64 {
    slot - slot % TURN_SLOTS
}

#[derive(Default, Clone)]
pub struct Schedule {
    /// slot0 of each target turn -> target identity
    pub turns: BTreeMap<u64, String>,
    /// slot -> leader identity, for a window around now
    pub leaders: BTreeMap<u64, String>,
    pub epoch: u64,
    pub epoch_first_slot: u64,
    pub slots_in_epoch: u64,
}

impl Schedule {
    /// Target turns for the current epoch (and the next one if `until_slot` reaches into it), plus slot leaders
    /// for [from_slot, until_slot].
    pub async fn load(rpc: &Rpc, targets: &[String], from_slot: u64, until_slot: u64) -> Result<Schedule> {
        let info = rpc.epoch_info().await?;
        let abs = info["absoluteSlot"].as_u64().ok_or_else(|| anyhow!("epochInfo"))?;
        let idx = info["slotIndex"].as_u64().unwrap_or(0);
        let n = info["slotsInEpoch"].as_u64().unwrap_or(432_000);
        let epoch = info["epoch"].as_u64().unwrap_or(0);
        let first = abs - idx;
        let mut s = Schedule { epoch, epoch_first_slot: first, slots_in_epoch: n, ..Default::default() };
        let mut epochs = vec![first];
        if until_slot >= first + n {
            epochs.push(first + n);
        }
        for e_first in epochs {
            for t in targets {
                match rpc.leader_slots(e_first, t, e_first).await {
                    Ok(slots) => {
                        for sl in slots {
                            s.turns.insert(turn_start(sl), t.clone());
                        }
                    }
                    Err(e) if e_first != first => {
                        crate::util::log("schedule_next_epoch_unavailable", serde_json::json!({"error": e.to_string()}))
                    }
                    Err(e) => return Err(e),
                }
            }
        }
        s.extend_leaders(rpc, from_slot, until_slot).await?;
        Ok(s)
    }

    pub async fn extend_leaders(&mut self, rpc: &Rpc, from_slot: u64, until_slot: u64) -> Result<()> {
        let mut start = from_slot;
        while start <= until_slot {
            let limit = (until_slot - start + 1).min(5_000);
            let ls = rpc.slot_leaders(start, limit).await?;
            if ls.is_empty() {
                break;
            }
            for (k, l) in ls.into_iter().enumerate() {
                self.leaders.insert(start + k as u64, l);
            }
            start += limit;
        }
        if let Some(&min_keep) = self.leaders.keys().next_back() {
            let cut = min_keep.saturating_sub(50_000);
            self.leaders = self.leaders.split_off(&cut);
        }
        Ok(())
    }

    pub fn leader(&self, slot: u64) -> Option<&str> {
        self.leaders.get(&slot).map(|s| s.as_str())
    }

    /// Turns of any target with slot0 in [from, to).
    pub fn turns_between(&self, from: u64, to: u64) -> Vec<(u64, String)> {
        self.turns.range(from..to).map(|(s, t)| (*s, t.clone())).collect()
    }

    pub fn leaders_cover(&self, slot: u64) -> bool {
        self.leaders.contains_key(&slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_boundaries() {
        assert_eq!(turn_start(462_683_067), 462_683_064);
        assert_eq!(turn_start(462_683_064), 462_683_064);
        // epochs are 432,000 slots, a multiple of 4, so turns never straddle an epoch boundary
        assert_eq!(432_000 % TURN_SLOTS, 0);
    }
}
