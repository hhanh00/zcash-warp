use super::{
    fee::FeeManager, ExtendedPayment, OutputNote, Payment, PaymentBuilder, PaymentItem, TxInput,
    TxOutput, UnsignedTransaction,
};
use anyhow::Result;
use rusqlite::Connection;
use zcash_primitives::{consensus::Network, memo::MemoBytes};

use crate::{
    db::{get_account_info, list_received_notes, list_utxos},
    lwd::get_tree_state,
    types::PoolMask,
    warp::hasher::{OrchardHasher, SaplingHasher},
    Client,
};

impl PaymentBuilder {
    pub async fn new(
        network: &Network,
        connection: &Connection,
        client: &mut Client,
        account: u32,
        height: u32,
        payment: Payment,
    ) -> Result<Self> {
        let (s_tree, o_tree) = get_tree_state(client, height).await?;
        let s_edge = s_tree.to_edge(&SaplingHasher::default());
        let o_edge = o_tree.to_edge(&OrchardHasher::default());

        let ai = get_account_info(network, connection, account)?;
        let account_id = ai.to_account_unique_id();
        let account_name = ai.name.clone();
        let account_pools = match ai.account_type() {
            crate::types::AccountType::Seed => 6, // S + O: do not use transparent
            crate::types::AccountType::SaplingSK => 2,
            crate::types::AccountType::SaplingVK => 6,
            crate::types::AccountType::UnifiedVK => 6,
        } as u8;

        let transparent_inputs = list_utxos(connection, height)?;
        let sapling_inputs = list_received_notes(connection, height, false)?;
        let orchard_inputs = list_received_notes(connection, height, true)?;

        let transparents = transparent_inputs
            .iter()
            .map(|utxo| TxInput::from_utxo(utxo))
            .collect::<Vec<_>>();
        let saplings = sapling_inputs
            .iter()
            .map(|note| TxInput::from_sapling(note))
            .collect::<Vec<_>>();
        let orchards = orchard_inputs
            .iter()
            .map(|note| TxInput::from_orchard(note))
            .collect::<Vec<_>>();

        let inputs = [transparents, saplings, orchards];
        let outputs = payment
            .recipients
            .into_iter()
            .map(|p| ExtendedPayment::to_extended(network, p))
            .collect::<Result<Vec<_>>>()?;

        // Determine which pool to use for the change output
        // 1. pick one of the output pools if they are supported by our account
        let o_pools = outputs.iter().map(|o| 1 << o.pool).fold(0, |a, b| a | b);
        let change_pools = account_pools & o_pools;
        // otherwise use whatever pool we
        let p = PoolMask(change_pools)
            .to_pool()
            .or_else(|| PoolMask(account_pools).to_pool());
        let change_pool = p.unwrap();
        let change_address = ai.to_address(network, PoolMask(change_pool)).unwrap();
        let change_note = OutputNote::from_address(network, &change_address, MemoBytes::empty())?;

        let mut fee_manager = FeeManager::default();
        let mut fee = fee_manager.fee();
        fee += fee_manager.add_output(change_pool);

        let b = PaymentBuilder {
            network: network.clone(),
            height,
            account,
            account_name,
            account_id,
            inputs,
            outputs,
            fee_manager,
            fee,
            available: [0u64; 3],
            change_pool,
            change_address,
            change_note,
            s_edge,
            o_edge,
        };
        Ok(b)
    }

    fn select_pool(used: &[bool], available: &[u64]) -> u8 {
        // if we used sapling but not orchard, assign to sapling
        if used[1] && !used[2] && available[1] > 0 {
            1
        }
        // if we used orchard but not sapling, assign to orchard
        else if !used[1] && used[2] && available[2] > 0 {
            2
        }
        // otherwise assign to the pool with the highest amount of funds
        else {
            if available[1] >= available[2] {
                1
            } else {
                2
            }
        }
    }

    pub fn build(mut self) -> Result<UnsignedTransaction> {
        let mut used = [false; 3];
        used[self.change_pool as usize] = true;

        for phase in 0..8 {
            for i in 0..3 {
                self.available[i] = self.inputs[i].iter().map(|n| n.remaining).sum::<u64>();
            }

            for output in self.outputs.iter_mut() {
                tracing::info!("phase {} output {:?}", phase, output);
                if phase > 0 && output.remaining == 0 {
                    continue;
                }
                let src_pool: u8;
                let out_pool = output.pool;
                assert!(out_pool < 4);
                match phase {
                    0 => {
                        if out_pool != 3 {
                            self.fee += self.fee_manager.add_output(output.pool);
                        }
                        continue;
                    }
                    // pay shielded outputs from the same source pool
                    // s -> s, o -> o
                    1 => {
                        if out_pool == 0 || out_pool == 3 {
                            continue;
                        }
                        assert!(out_pool == 1 || out_pool == 2);
                        src_pool = output.pool;
                    }
                    // handle S+O
                    2 => {
                        if out_pool != 3 {
                            continue;
                        }
                        assert!(out_pool == 3);

                        src_pool = Self::select_pool(&used, &self.available);
                        output.pool = src_pool;
                        self.fee += self.fee_manager.add_output(output.pool);
                    }
                    // use the other shielded pool
                    // s -> o, o -> s
                    3 => {
                        assert!(out_pool != 3);
                        if out_pool == 0 {
                            continue;
                        }
                        src_pool = 3 - out_pool;
                    }
                    // use t -> s/o
                    4 => {
                        if out_pool == 0 {
                            continue;
                        }
                        assert!(out_pool == 0);
                        src_pool = 0;
                    }
                    // handle transparent payments
                    // s/o -> t, using the select_pool algorithm
                    5 | 6 => {
                        if out_pool != 0 {
                            continue;
                        }
                        src_pool = Self::select_pool(&used, &self.available);
                    }
                    // finally
                    // t -> t
                    7 => {
                        if out_pool != 0 {
                            continue;
                        }
                        src_pool = 0;
                    }

                    _ => unreachable!(),
                }

                tracing::info!(
                    "src {} out {} amount {}",
                    src_pool,
                    output.pool,
                    output.remaining
                );
                for n in self.inputs[src_pool as usize].iter_mut() {
                    if n.remaining > 0 {
                        used[src_pool as usize] = true;
                        if n.remaining == n.amount {
                            self.fee += self.fee_manager.add_input(src_pool);
                        }
                    }
                    tracing::info!("FEE {}", self.fee);
                    let r = n.remaining.min(output.remaining + self.fee);
                    n.remaining -= r;
                    let r2 = r.min(self.fee);
                    self.fee -= r2;
                    output.remaining -= r - r2;

                    if output.remaining == 0 {
                        break;
                    }
                }
            }
        }

        assert!(self.fee == 0, "{}", self.fee);
        let mut tx_notes = vec![];
        let mut tx_outputs = vec![];
        for i in 0..3 {
            for n in self.inputs[i].iter() {
                if n.remaining == n.amount {
                    continue;
                }
                tx_notes.push(n.clone());
                if n.remaining != 0 {
                    tx_outputs.push(TxOutput {
                        address_string: self.change_address.clone(),
                        value: n.remaining,
                        note: self.change_note.clone(),
                    });
                }
            }
        }

        for n in self.outputs.into_iter() {
            if n.remaining != 0 {
                anyhow::bail!("Not Enough Funds");
            }
            let pi = n.to_inner();
            let PaymentItem {
                address,
                memo,
                amount,
                ..
            } = pi;
            let note = OutputNote::from_address(&self.network, &address, memo)?;
            tx_outputs.push(TxOutput {
                address_string: address,
                value: amount,
                note,
            });
        }

        tracing::info!("{:?}", tx_notes);
        tracing::info!("{:?}", tx_outputs);
        tracing::info!("{:?}", self.fee_manager);

        let transaction = UnsignedTransaction {
            account: self.account,
            account_id: self.account_id,
            account_name: self.account_name.clone(),
            height: self.height,
            edges: [
                self.s_edge.to_auth_path(&SaplingHasher::default()),
                self.o_edge.to_auth_path(&OrchardHasher::default()),
            ],
            roots: [
                self.s_edge.root(&SaplingHasher::default()),
                self.o_edge.root(&OrchardHasher::default()),
            ],
            tx_notes,
            tx_outputs,
        };
        Ok(transaction)
    }
}
