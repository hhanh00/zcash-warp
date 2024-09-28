use super::{
    fee::FeeManager, AdjustableUnsignedTransaction, Error, ExtendedRecipient, OutputNote,
    PaymentBuilder, Result, TxInput, TxOutput, UnsignedTransaction,
};
use rusqlite::Connection;
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::memo::MemoBytes;

use crate::{
    data::fb::RecipientT,
    db::{
        account::get_account_info,
        notes::{list_received_notes, list_utxos},
    },
    fb_unwrap,
    keys::AccountKeys,
    network::Network,
    types::{CheckpointHeight, PoolMask},
    utils::{pay::COST_PER_ACTION, ua::single_receiver_address},
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
        recipients: &[RecipientT],
        src_pools: PoolMask,
        s_tree: &CommitmentTreeFrontier,
        o_tree: &CommitmentTreeFrontier,
    ) -> Result<Self> {
        let height: u32 = height.into();
        let ai = get_account_info(network, connection, account)?;
        let outputs = recipients
            .into_iter()
            .map(|p| ExtendedRecipient::to_extended(network, p.clone()))
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
            used: [false; 3],
            use_change: true,
            s_edge: s_tree.to_edge(&SaplingHasher::default()),
            o_edge: o_tree.to_edge(&OrchardHasher::default()),
        })
    }

    pub fn add_account_funds(&mut self, connection: &Connection) -> Result<()> {
        let account_pools = match self.ai.account_type()? {
            crate::types::AccountType::Seed { .. } => 7, // T + S + O
            crate::types::AccountType::AccountKeys(AccountKeys {
                tvk,
                svk,
                ovk,
                ..
            }) => {
                let mut pools = 0;
                if tvk.is_some() {
                    pools |= 1;
                }
                if svk.is_some() {
                    pools |= 2;
                }
                if ovk.is_some() {
                    pools |= 4;
                }
                pools
            }
        } as u8;
        let account_pools = account_pools & self.src_pools.0; // exclude pools
        self.account_pools = PoolMask(account_pools);

        let has_tex = self.outputs.iter().any(|o| {
            let address = &o.recipient.address;
            let address = RecipientAddress::decode(&self.network, fb_unwrap!(address)).unwrap();
            if let RecipientAddress::Tex(_) = address {
                true
            } else {
                false
            }
        });

        let transparent_inputs = if account_pools & 1 != 0 {
            list_utxos(
                connection,
                Some(self.account),
                CheckpointHeight(self.height),
            )?
        } else {
            vec![]
        };
        let sapling_inputs = if account_pools & 2 != 0 && !has_tex {
            list_received_notes(
                connection,
                Some(self.account),
                CheckpointHeight(self.height),
                false,
                false,
            )?
        } else {
            vec![]
        };
        let orchard_inputs = if account_pools & 4 != 0 && !has_tex {
            list_received_notes(
                connection,
                Some(self.account),
                CheckpointHeight(self.height),
                true,
                false,
            )?
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

    fn calculate_available(&mut self) -> Result<()> {
        for i in 0..3 {
            self.available[i] = self.inputs[i].iter().map(|n| n.remaining).sum::<u64>();
        }
        Ok(())
    }

    fn fill_outputs_from(
        &mut self,
        src: u8,
        dst: u8,
        outputs: &mut [&mut ExtendedRecipient],
    ) -> Result<()> {
        self.calculate_available()?;
        for output in outputs.iter_mut() {
            if output.pool_mask.to_pool().unwrap() != dst {
                continue;
            }
            for n in self.inputs[src as usize].iter_mut() {
                if output.remaining > 0 && n.amount > COST_PER_ACTION && n.remaining > 0 {
                    self.used[src as usize] = true;
                    if n.remaining == n.amount && (output.remaining > 0 || self.fee > 0) {
                        // first time this note is used
                        // adjust the fee
                        self.fee += self.fee_manager.add_input(src);
                    }
                    let r = n.remaining.min(output.remaining + self.fee);
                    tracing::info!("Using Amount {r}");

                    n.remaining -= r;
                    let r2 = r.min(self.fee);
                    self.fee -= r2;
                    output.remaining -= r - r2;
                }

                if output.remaining == 0 {
                    break;
                }
            }
        }

        Ok(())
    }

    /*
       T  S  O
    T  8  4  5
    S  6  1  2
    O  7  3  0
    */
    fn fill_outputs(&mut self, outputs: &mut [&mut ExtendedRecipient]) -> Result<()> {
        for o in outputs.iter() {
            self.fee += self.fee_manager.add_output(o.pool_mask.to_pool().unwrap());
        }
        // S->T has 6, i.e the entry at index 6 is S(1)*3 + T(0) = 3
        let connection_order = [8, 4, 5, 7, 1, 2, 3, 6, 0];
        for i in connection_order {
            let src = i / 3;
            let dst = i % 3;
            self.fill_outputs_from(src, dst, outputs)?;
        }
        Ok(())
    }

    pub fn prepare(&mut self) -> Result<AdjustableUnsignedTransaction> {
        if self.outputs.is_empty() {
            return Err(Error::NoRecipient);
        }

        for i in 0..3 {
            for inp in self.inputs[i].iter_mut() {
                inp.remaining = inp.amount;
            }
        }
        tracing::info!("Inputs {:?}", self.inputs);

        let mut outputs = std::mem::take(&mut self.outputs);
        tracing::info!("outputs {:?}", outputs);
        let (mut single, mut multiple): (Vec<_>, Vec<_>) =
            outputs.iter_mut().partition(|p| p.pool_mask.single_pool());

        tracing::info!("single {:?}", single);
        // fill the recipient orders that use a single pool
        self.fill_outputs(single.as_mut_slice())?;

        self.calculate_available()?;
        // use the largest shielded pool available for orders
        // that have more than one receiver
        let p = if self.available[1] > self.available[2] {
            2
        } else {
            4
        };
        for r in multiple.iter_mut() {
            r.pool_mask = PoolMask(p);
        }
        self.fill_outputs(multiple.as_mut_slice())?;

        if self.use_change {
            tracing::info!("Used pools {:?}", self.used);
            let change_pool = (0..3usize)
                .rev()
                .find(|&i| self.used[i])
                .ok_or(anyhow::anyhow!("No Funds"))? as u8;
            tracing::info!("Change pool {change_pool}");
            let change_pool = 1 << change_pool;
            let change_address = self
                .ai
                .to_address(&self.network, PoolMask(change_pool))
                .unwrap();
            let mut change = ExtendedRecipient {
                recipient: RecipientT {
                    address: Some(change_address),
                    amount: 0,
                    pools: change_pool,
                    memo: None,
                    memo_bytes: None,
                },
                amount: 0,
                remaining: 0,
                pool_mask: PoolMask(change_pool),
                is_change: true,
            };
            self.fill_outputs(std::slice::from_mut(&mut &mut change))?;
            outputs.push(change);
        }

        // Collect the input/output assignments
        let mut tx_notes = vec![];
        for i in 0..3 {
            for inp in self.inputs[i].iter() {
                if inp.remaining != inp.amount {
                    tx_notes.push(inp.clone());
                }
            }
        }

        let mut tx_outputs = vec![];
        for n in outputs.iter() {
            let pi = n.clone().to_inner();
            let pi = pi.normalize_memo()?;
            let RecipientT {
                address,
                memo_bytes,
                amount,
                ..
            } = pi;
            let address =
                single_receiver_address(&self.network, fb_unwrap!(address), n.pool_mask)?.unwrap();
            let memo = memo_bytes.map(|memo| MemoBytes::from_bytes(&memo).unwrap());
            let memo = memo.unwrap_or(MemoBytes::empty());
            let note = OutputNote::from_address(&self.network, &address, memo)?;
            tx_outputs.push(TxOutput {
                address_string: address,
                pool: n.pool_mask.to_pool().unwrap(),
                amount,
                note,
                is_change: n.is_change,
            });
        }

        tracing::debug!("# inputs {}", tx_notes.len());
        tracing::debug!("{:?}", tx_notes);
        tracing::debug!("# outputs {}", tx_outputs.len());
        tracing::debug!("{:?}", tx_outputs);

        let sum_ins = tx_notes.iter().map(|n| n.amount).sum::<u64>();
        let sum_outs = tx_outputs.iter().map(|n| n.amount).sum::<u64>() + self.fee_manager.fee();
        let change = (sum_ins as i64) - (sum_outs as i64); // can be negative at this point

        let transaction = AdjustableUnsignedTransaction {
            tx_notes,
            tx_outputs,
            change,
        };

        tracing::debug!("tx {:?}", transaction);

        Ok(transaction)
    }

    pub fn finalize(self, mut utx: AdjustableUnsignedTransaction) -> Result<UnsignedTransaction> {
        tracing::debug!("{:?}", utx.tx_notes);
        let change = utx.change;
        if change < 0 {
            return Err(Error::NotEnoughFunds(-change as u64));
        }
        if self.use_change {
            let change_output = utx.tx_outputs.last_mut().unwrap();
            change_output.amount = change as u64;
        } else if change != 0 {
            return Err(Error::NoChangeOutput);
        }
        tracing::debug!("{:?}", utx.tx_outputs);

        let utx = UnsignedTransaction {
            account: self.account,
            account_name: self.ai.name.clone(),
            account_id: self.ai.fingerprint.clone(),
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
            fees: self.fee_manager,
        };

        Ok(utx)
    }
}

impl AdjustableUnsignedTransaction {
    pub fn add_to_change(&mut self, offset: i64) -> Result<()> {
        if let Some(payee) = self.tx_outputs.first_mut() {
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
