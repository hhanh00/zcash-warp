use std::sync::mpsc::Sender;

use anyhow::Result;
use orchard::{
    keys::{IncomingViewingKey, Scope},
    note::{RandomSeed, Rho},
    value::NoteValue,
    Address, Note,
};

use crate::{
    lwd::rpc::{Bridge, CompactOrchardAction, CompactTx},
    network::Network,
    types::AccountInfo,
    warp::{hasher::OrchardHasher, sync::ReceivedNote, try_orchard_decrypt},
    Hash,
};

use super::ShieldedProtocol;

pub struct OrchardProtocol;

impl ShieldedProtocol for OrchardProtocol {
    type Hasher = OrchardHasher;
    type IVK = IncomingViewingKey;
    type Spend = CompactOrchardAction;
    type Output = CompactOrchardAction;

    fn is_orchard() -> bool {
        true
    }

    fn extract_ivk(ai: &AccountInfo) -> Option<(u32, Self::IVK)> {
        ai.orchard
            .as_ref()
            .map(|oi| (ai.account, oi.vk.to_ivk(Scope::External)))
    }

    fn extract_inputs(tx: &CompactTx) -> &Vec<Self::Spend> {
        &tx.actions
    }

    fn extract_outputs(tx: &CompactTx) -> &Vec<Self::Output> {
        &tx.actions
    }

    fn extract_bridge(tx: &CompactTx) -> Option<&Bridge> {
        tx.orchard_bridge.as_ref()
    }

    fn extract_nf(i: &Self::Spend) -> Hash {
        i.nullifier.clone().try_into().unwrap()
    }

    fn extract_cmx(o: &Self::Output) -> Hash {
        o.cmx.clone().try_into().unwrap()
    }

    fn try_decrypt(
        network: &Network,
        ivks: &[(u32, Self::IVK)],
        height: u32,
        time: u32,
        ivtx: u32,
        vout: u32,
        output: &Self::Output,
        sender: &mut Sender<ReceivedNote>,
    ) -> Result<()> {
        try_orchard_decrypt(network, ivks, height, time, ivtx, vout, output, sender)
    }

    fn finalize_received_note(txid: Hash, note: &mut ReceivedNote, ai: &AccountInfo) -> Result<()> {
        let recipient = Address::from_raw_address_bytes(&note.address).unwrap();
        let rho = Rho::from_bytes(&note.rho.unwrap()).unwrap();
        let n = Note::from_parts(
            recipient,
            NoteValue::from_raw(note.value),
            rho,
            RandomSeed::from_bytes(note.rcm, &rho).unwrap(),
        )
        .unwrap();
        let vk = &ai.orchard.as_ref().unwrap().vk;
        let nf = n.nullifier(&vk);
        note.nf = nf.to_bytes();
        note.tx.txid = txid;
        Ok(())
    }
}
