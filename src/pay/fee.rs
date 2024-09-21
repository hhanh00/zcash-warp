use serde::{Serialize, Deserialize};

use crate::utils::pay::COST_PER_ACTION;

#[derive(Clone, Serialize, Deserialize, Default, Debug)]
pub struct FeeManager {
    pub num_inputs: [u8; 3],
    pub num_outputs: [u8; 3],
}

impl FeeManager {
    pub fn add_input(&mut self, pool: u8) -> u64 {
        let fee = self.fee();
        self.num_inputs[pool as usize] += 1;
        self.fee() - fee
    }

    pub fn add_output(&mut self, pool: u8) -> u64 {
        let fee = self.fee();
        self.num_outputs[pool as usize] += 1;
        self.fee() - fee
    }

    pub fn fee(&self) -> u64 {
        let t = self.num_inputs[0].max(self.num_outputs[0]);
        let s = if self.num_inputs[1] > 0 || self.num_outputs[1] > 0 {
            // if any sapling, # bundle outputs = max(2, # outputs)
            // if any input, # bundle inputs = max(1, # inputs)
            // # logical sapling = max(# bundle in, bundle out) = 
            // max(2, # inputs, # outputs)
            // I O -> BI BO -> L
            // 0 0 -> 0  0  -> 0
            // 1 0 -> 1  2  -> 2
            // 0 1 -> 0  2  -> 2
            // 1 1 -> 1  2  -> 2
            // 2 1 -> 2  1  -> 2
            // etc.
            self.num_inputs[1].max(self.num_outputs[1]).max(2)
        } else {
            0
        };
        let o = if self.num_inputs[2] > 0 || self.num_outputs[2] > 0 {
            // padding min 2 actions
            self.num_inputs[2].max(self.num_outputs[2]).max(2)
        } else {
            0
        };
        let f = t + s + o;
        tracing::debug!("fee: {}:{} {}:{} {}:{}", 
            self.num_inputs[0], self.num_outputs[0],
            self.num_inputs[1], self.num_outputs[1],
            self.num_inputs[2], self.num_outputs[2],
        );
        tracing::debug!("fee: {t} {s} {o} -> {f}");
        f as u64 * COST_PER_ACTION
    }

    #[allow(dead_code)]
    fn min_actions_padding(a: u8) -> u8 {
        if a == 0 {
            0
        } else {
            a.max(2)
        }
    }
}
