use super::{
    fee::FeeManager, AdjustableUnsignedTransaction, ChangeOutput, Error, ExtendedPayment,
    OutputNote, Payment, PaymentBuilder, PaymentItem, Result, TxInput, TxOutput,
    UnsignedTransaction,
};
use rusqlite::Connection;
use zcash_primitives::{consensus::Network, memo::MemoBytes};

use crate::{
    db::{account::get_account_info, notes:: {list_received_notes, list_utxos}},
    types::PoolMask,
    warp::{
        hasher::{OrchardHasher, SaplingHasher},
        legacy::CommitmentTreeFrontier,
        UTXO,
    },
};

/*
    Use the Payment Builder to make outgoing transactions.
    Follow the steps outlined below:

    1. Create with the usual network, connection, account, etc
    + payment which is a collection of recipients (address, amount, memo)
    + sapling/orchard commitment tree (you get these from lwd with `get_tree_state`)
    The builder records the *outputs* but has no funds yet
    2. add funds to use; either directly with `add_utxos`
    or by using the notes that the account contains with `add_account_funds`
    3. call `set_use_change` with true/false to indicate if the transaction
    should have a change output or not. Fees depends on the number and types
    of inputs/outputs, therefore having a change output may affect the fees
    If you set_use_change to false and the transaction needs some change,
    it will fail later
    3. `prepare` a transaction plan. This picks up enough
    funds to cover the outputs and pay for the fees.
    We have: Inputs = Outputs + Change + Fees (by amount)
    Fees are calculated based on ZIP-317 and Outputs are not modified.
    But, we may run out of Input funds. In this case, Change MAY BE negative!
    4. prepare returns an AdjustableUnsignedTransaction.
    Inputs and fees are frozen, but you can move funds from the Change
    to the *first* output/recipient by calling `add_to_change`.
    This allows you to adjust the amount paid without modifying the rest
    of the transaction.
    For example, if you want the recipient to pay for the fees, you can
    add the amount of fees to the Change (and it automatically decreases
    the amount paid to the recipient)
    Even if set_use_change is false, the Change amount can be non zero.
    But then you must adjust the transaction before it can be finalized.
    For example, if we want to move *all* the funds to another address,
    the recipient amount is initially the sum of all the notes. However, this
    leaves no room for the fees. The transaction ends up with a negative
    change equal to -fees. To make the transaction work, you must
    add +fees to the change to make it 0, and it decreases the amount
    received by the recipient by -fees.
    Note that if we created the transaction differently, we would have
    a change output that increases the fees unnecessarily.
    5. `finalize` the AdjustableUnsignedTransaction into a
    UnsignedTransaction. This checks the change output and creates
    an output if needed
    6. `build` collects the secret keys from the account and from
    the TSKStore (Transparent Secret Key Store) and builds
    the signed raw binary transaction.
    7. `broadcast` sends a binary transaction to the network
*/

impl PaymentBuilder {
    pub fn new(
        network: &Network,
        connection: &Connection,
        account: u32,
        height: u32,
        payment: Payment,
        src_pools: PoolMask,
        s_tree: &CommitmentTreeFrontier,
        o_tree: &CommitmentTreeFrontier,
    ) -> Result<Self> {
        let ai = get_account_info(network, connection, account)?;
        let outputs = payment
            .recipients
            .into_iter()
            .map(|p| ExtendedPayment::to_extended(network, p))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            network: network.clone(),
            height,
            account,
            ai,
            inputs: [vec![], vec![], vec![]],
            outputs,
            account_pools: PoolMask::default(),
            src_pools,
            fee_manager: FeeManager::default(),
            fee: 0,
            available: [0; 3],
            change: ChangeOutput::default(),
            s_edge: s_tree.to_edge(&SaplingHasher::default()),
            o_edge: o_tree.to_edge(&OrchardHasher::default()),
        })
    }

    pub fn add_account_funds(&mut self, connection: &Connection) -> Result<()> {
        let account_pools = match self.ai.account_type()? {
            crate::types::AccountType::Seed { .. } => 7, // T + S + O
            crate::types::AccountType::SaplingSK { .. } => 2,
            crate::types::AccountType::SaplingVK { .. } => 7,
            crate::types::AccountType::UnifiedVK { .. } => 7,
        } as u8;
        let account_pools = account_pools & self.src_pools.0; // exclude pools
        self.account_pools = PoolMask(account_pools);

        let transparent_inputs = if account_pools & 1 != 0 {
            list_utxos(connection, self.height)?
        } else {
            vec![]
        };
        let sapling_inputs = if account_pools & 2 != 0 {
            list_received_notes(connection, self.height, false)?
        } else {
            vec![]
        };
        let orchard_inputs = if account_pools & 4 != 0 {
            list_received_notes(connection, self.height, true)?
        } else {
            vec![]
        };

        self.inputs[0].extend(
            transparent_inputs
                .iter()
                .map(|utxo| TxInput::from_utxo(utxo)),
        );
        self.inputs[1].extend(
            sapling_inputs
                .iter()
                .map(|note| TxInput::from_sapling(note)),
        );
        self.inputs[2].extend(
            orchard_inputs
                .iter()
                .map(|note| TxInput::from_orchard(note)),
        );

        Ok(())
    }

    pub fn set_use_change(&mut self, use_change: bool) -> Result<()> {
        // Determine which pool to use for the change output
        // 1. pick one of the output pools if they are supported by our account
        let o_pools = self
            .outputs
            .iter()
            .map(|o| 1 << o.pool)
            .fold(0, |a, b| a | b);
        // Use a pool in common between our account's and the recipients
        let change_pools = self.account_pools.0 & o_pools & 6; // but not the transparent pool
                                                               // fallback to the account's best pool if there is nothing
        let change_pool = use_change
            .then_some(
                PoolMask(change_pools)
                    .to_pool()
                    .or(self.account_pools.to_pool()),
            )
            .flatten();

        let note = change_pool
            .map(|p| {
                self.fee += self.fee_manager.add_output(p);
                let change_address = self.ai.to_address(&self.network, PoolMask(1 << p)).unwrap();
                OutputNote::from_address(&self.network, &change_address, MemoBytes::empty())
            })
            .transpose()?;
        let change = ChangeOutput {
            pools: change_pool.into(),
            value: 0,
            note,
        };

        self.change = change;

        Ok(())
    }

    pub fn add_utxos(&mut self, utxos: &[UTXO]) -> Result<()> {
        let mut utxos = utxos
            .iter()
            .map(|utxo| TxInput::from_utxo(utxo))
            .collect::<Vec<_>>();
        self.inputs[0].append(&mut utxos);
        Ok(())
    }

    pub fn prepare(&mut self) -> Result<AdjustableUnsignedTransaction> {
        let mut used = [false; 3];
        self.change.pools.to_pool().map(|p| {
            used[p as usize] = true;
        });

        for i in 0..3 {
            for inp in self.inputs[i].iter_mut() {
                inp.remaining = inp.amount;
            }
        }

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
                        output.remaining = output.amount;
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
                        assert!(out_pool != 0);
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

        if self.fee != 0 {
            self.change.value -= self.fee as i64;
        }
        let mut tx_notes = vec![];
        let mut tx_outputs = vec![];
        for i in 0..3 {
            for n in self.inputs[i].iter() {
                if n.remaining == n.amount {
                    continue;
                }
                tx_notes.push(n.clone());
                if n.remaining != 0 {
                    self.change.value += n.remaining as i64;
                }
            }
        }

        for n in self.outputs.iter() {
            if n.remaining != 0 {
                self.change.value -= n.remaining as i64;
            }
            let pi = n.clone().to_inner();
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

        let mut change = ChangeOutput::default();
        std::mem::swap(&mut self.change, &mut change);

        let transaction = AdjustableUnsignedTransaction {
            tx_notes,
            tx_outputs,
            change,
        };

        Ok(transaction)
    }

    pub fn finalize(self, mut utx: AdjustableUnsignedTransaction) -> Result<UnsignedTransaction> {
        let change = utx.change;
        if change.value.is_negative() {
            return Err(Error::NotEnoughFunds(-change.value as u64));
        }
        match change.note {
            Some(note) => {
                let address_string = note.to_address(&self.network);
                utx.tx_outputs.push(TxOutput {
                    address_string,
                    value: change.value as u64,
                    note,
                });
            }
            None => {
                if change.value != 0 {
                    return Err(Error::NoChangeOutput);
                }
            }
        }
        let utx = UnsignedTransaction {
            account: self.account,
            account_name: self.ai.name.clone(),
            account_id: self.ai.to_account_unique_id(),
            height: self.height,
            edges: [
                self.s_edge.to_auth_path(&SaplingHasher::default()),
                self.o_edge.to_auth_path(&OrchardHasher::default()),
            ],
            roots: [
                self.s_edge.root(&SaplingHasher::default()),
                self.o_edge.root(&OrchardHasher::default()),
            ],
            tx_notes: utx.tx_notes,
            tx_outputs: utx.tx_outputs,
        };

        Ok(utx)
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
}

impl AdjustableUnsignedTransaction {
    pub fn add_to_change(&mut self, offset: i64) -> Result<()> {
        if let Some(payee) = self.tx_outputs.first_mut() {
            if offset > 0 {
                let o = offset as u64;
                if o > payee.value {
                    return Err(Error::FeesTooHighForRecipient(o));
                }
                payee.value -= o;
                self.change.value += o as i64;
            } else {
                let o = -offset;
                payee.value += o as u64;
                self.change.value -= o as i64;
            }
        } else {
            return Err(Error::NoRecipient);
        }
        Ok(())
    }
}
