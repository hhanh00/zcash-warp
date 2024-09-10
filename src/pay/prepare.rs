use super::{
    fee::FeeManager, AdjustableUnsignedTransaction, Error, ExtendedPayment, OutputNote, Payment,
    PaymentBuilder, PaymentItem, Result, TxInput, TxOutput, UnsignedTransaction,
};
use rusqlite::Connection;
use zcash_primitives::{consensus::Network, memo::MemoBytes};
use zcash_keys::address::Address as RecipientAddress;

use crate::{
    db::{
        account::get_account_info,
        notes::{list_received_notes, list_utxos},
    },
    types::{CheckpointHeight, PoolMask},
    utils::ua::single_receiver_address,
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
        height: CheckpointHeight,
        payment: Payment,
        src_pools: PoolMask,
        s_tree: &CommitmentTreeFrontier,
        o_tree: &CommitmentTreeFrontier,
    ) -> Result<Self> {
        let height: u32 = height.into();
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
            use_change: true,
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

        let has_tex = self.outputs.iter().any(|o| { 
            let address = &o.payment.address;
            let address = RecipientAddress::decode(&self.network, address).unwrap();
            if let RecipientAddress::Tex(_) = address { true } else { false }
        });

        let transparent_inputs = if account_pools & 1 != 0 {
            list_utxos(connection, CheckpointHeight(self.height))?
        } else {
            vec![]
        };
        let sapling_inputs = if account_pools & 2 != 0 && !has_tex {
            list_received_notes(connection, CheckpointHeight(self.height), false)?
        } else {
            vec![]
        };
        let orchard_inputs = if account_pools & 4 != 0 && !has_tex {
            list_received_notes(connection, CheckpointHeight(self.height), true)?
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
        tracing::debug!("{:?}", self.inputs);

        Ok(())
    }

    pub fn set_use_change(&mut self, use_change: bool) -> Result<()> {
        self.use_change = use_change;
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
        if self.outputs.is_empty() {
            return Err(Error::NoRecipient);
        }

        let mut used = [false; 3];

        if self.use_change {
            // add a change output in first position
            // Determine which pool to use for the change output
            // 1. pick one of the output pools if they are supported by our account
            let o_pools = self.outputs.iter().map(|o| o.pool.0).fold(0, |a, b| a | b);
            // 2. Use a pool in common between our account's and the recipients
            let change_pools = self.account_pools.0 & o_pools & 6; // but not the transparent pool
            let change_pools = if change_pools != 0 {
                change_pools
            } else {
                // fallback to the account's best pool if there is nothing
                self.account_pools.0
            };
            let change_pools = PoolMask(change_pools);

            let change_address = self.ai.to_address(&self.network, change_pools).unwrap();
            tracing::info!("Use pool {change_pools:?} for change");
            let change = ExtendedPayment {
                payment: PaymentItem {
                    address: change_address,
                    amount: 0,
                    memo: None,
                },
                amount: 0,
                remaining: 0,
                pool: change_pools,
                is_change: true,
            };
            self.outputs.insert(0, change);
        }

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
                tracing::debug!("phase {} output {:?}", phase, output);
                if phase > 0 && output.remaining == 0 && !output.is_change {
                    continue;
                }
                let src_pool: u8;
                let out_pool_mask = output.pool.0;
                assert!(
                    out_pool_mask == 1
                        || out_pool_mask == 2
                        || out_pool_mask == 4
                        || out_pool_mask == 6
                );
                match phase {
                    0 => {
                        tracing::debug!("Initialization");
                        output.remaining = output.amount;
                        if out_pool_mask != 6 {
                            // add outputs except S+O
                            self.fee += self
                                .fee_manager
                                .add_output(PoolMask(out_pool_mask).to_pool().unwrap());
                        }
                        continue;
                    }
                    // pay shielded outputs from the same source pool
                    // s -> s, o -> o
                    1 => {
                        tracing::debug!("S -> S, O -> O");
                        if out_pool_mask == 1 || out_pool_mask == 6 {
                            continue;
                        }
                        assert!(out_pool_mask == 2 || out_pool_mask == 4);
                        src_pool = PoolMask(out_pool_mask).to_pool().unwrap();
                    }
                    // handle S+O
                    2 => {
                        tracing::debug!("S+O -> S|O");
                        if out_pool_mask != 6 {
                            continue;
                        }
                        assert!(out_pool_mask == 6);

                        src_pool = Self::select_pool(&used, &self.available);
                        output.pool = PoolMask::from_pool(src_pool);
                        // Only T/S/O possible from now
                        self.fee += self.fee_manager.add_output(src_pool);
                    }
                    // use the other shielded pool
                    // s -> o, o -> s
                    3 => {
                        tracing::debug!("S -> O, O -> S");
                        assert!(out_pool_mask != 6);
                        if out_pool_mask == 1 {
                            // Skip not T
                            continue;
                        }
                        src_pool = PoolMask(6 - out_pool_mask).to_pool().unwrap();
                        // S -> O, O -> S
                    }
                    // use t -> s/o
                    4 => {
                        tracing::debug!("T -> Z");
                        if out_pool_mask == 1 {
                            // Skip not T
                            continue;
                        }
                        assert!(out_pool_mask != 1);
                        src_pool = 0;
                    }
                    // handle transparent payments
                    // s/o -> t, using the select_pool algorithm
                    5 | 6 => {
                        tracing::debug!("Z -> T");
                        if out_pool_mask != 1 {
                            // T
                            continue;
                        }
                        src_pool = Self::select_pool(&used, &self.available);
                    }
                    // finally
                    // t -> t
                    7 => {
                        tracing::debug!("T -> T");
                        if out_pool_mask != 1 {
                            continue;
                        }
                        src_pool = 0;
                    }

                    _ => unreachable!(),
                }

                tracing::debug!(
                    "src {} out {:?} amount {}",
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

        let mut tx_notes = vec![];
        let mut tx_outputs = vec![];
        for i in 0..3 {
            for n in self.inputs[i].iter() {
                if n.remaining != n.amount {
                    tx_notes.push(n.clone());
                }
            }
        }

        for n in self.outputs.iter() {
            let pi = n.clone().to_inner();
            let PaymentItem {
                address,
                memo,
                amount,
                ..
            } = pi;
            let address = single_receiver_address(&self.network, &address, n.pool)?.unwrap();
            let note = OutputNote::from_address(
                &self.network,
                &address,
                memo.unwrap_or(MemoBytes::empty()),
            )?;
            tx_outputs.push(TxOutput {
                address_string: address,
                amount,
                note,
                change: false,
            });
        }
        if self.use_change {
            tx_outputs[0].change = true;
        }

        let sum_ins = tx_notes.iter().map(|n| n.amount).sum::<u64>();
        let sum_outs = tx_outputs.iter().map(|n| n.amount).sum::<u64>() + self.fee_manager.fee();
        let change = (sum_ins as i64) - (sum_outs as i64); // can be negative at this point

        let transaction = AdjustableUnsignedTransaction {
            tx_notes,
            tx_outputs,
            change,
        };

        Ok(transaction)
    }

    pub fn finalize(self, mut utx: AdjustableUnsignedTransaction) -> Result<UnsignedTransaction> {
        tracing::debug!("{:?}", utx.tx_notes);
        let change = utx.change;
        if change < 0 {
            return Err(Error::NotEnoughFunds(-change as u64));
        }
        if self.use_change {
            let note = OutputNote::from_address(
                &self.network,
                &utx.tx_outputs[0].address_string,
                MemoBytes::empty(),
            )?;
            utx.tx_outputs[0].amount = change as u64;
            utx.tx_outputs[0].note = note;
        } else if change != 0 {
            return Err(Error::NoChangeOutput);
        }
        tracing::debug!("{:?}", utx.tx_outputs);

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
        if let Some(payee) = self.tx_outputs.last_mut() {
            if offset > 0 {
                let o = offset as u64;
                if o > payee.amount {
                    return Err(Error::FeesTooHighForRecipient(o));
                }
                payee.amount -= o;
                self.change += o as i64;
            } else {
                let o = -offset;
                payee.amount += o as u64;
                self.change -= o as i64;
            }
        } else {
            return Err(Error::NoRecipient);
        }
        Ok(())
    }
}
