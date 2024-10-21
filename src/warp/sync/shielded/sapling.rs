use anyhow::Result;
use jubjub::Fr;
use sapling_crypto::{value::NoteValue, Note, PaymentAddress, Rseed, SaplingIvk};

use crate::{
    lwd::rpc::{Bridge, CompactSaplingOutput, CompactSaplingSpend, CompactTx},
    types::AccountInfo,
    warp::{hasher::SaplingHasher, sync::ReceivedNote, try_sapling_decrypt},
    Hash,
};

use super::ShieldedProtocol;

pub struct SaplingProtocol;

impl ShieldedProtocol for SaplingProtocol {
    type Hasher = SaplingHasher;
    type IVK = SaplingIvk;
    type Spend = CompactSaplingSpend;
    type Output = CompactSaplingOutput;

    fn is_orchard() -> bool {
        false
    }

    fn extract_ivk(ai: &AccountInfo) -> Option<(u32, Self::IVK)> {
        ai.sapling
            .as_ref()
            .map(|si| (ai.account, si.vk.fvk().vk.ivk()))
    }

    fn extract_inputs(tx: &CompactTx) -> &Vec<Self::Spend> {
        &tx.spends
    }

    fn extract_outputs(tx: &CompactTx) -> &Vec<Self::Output> {
        &tx.outputs
    }

    fn extract_bridge(tx: &CompactTx) -> Option<&Bridge> {
        tx.sapling_bridge.as_ref()
    }

    fn extract_nf(i: &Self::Spend) -> crate::Hash {
        i.clone().nf.try_into().unwrap()
    }

    fn extract_cmx(o: &Self::Output) -> crate::Hash {
        o.cmu.clone().try_into().unwrap()
    }

    fn try_decrypt(
        network: &crate::network::Network,
        ivks: &[(u32, Self::IVK)],
        height: u32,
        time: u32,
        ivtx: u32,
        vout: u32,
        output: &Self::Output,
        sender: &mut std::sync::mpsc::Sender<crate::warp::sync::ReceivedNote>,
    ) -> Result<()> {
        try_sapling_decrypt(
            network,
            ivks,
            height as u32,
            time,
            ivtx as u32,
            vout as u32,
            output,
            sender,
        )
    }

    fn finalize_received_note(txid: Hash, note: &mut ReceivedNote, ai: &AccountInfo) -> Result<()> {
        let recipient = PaymentAddress::from_bytes(&note.address).unwrap();
        if let Some(vk) = ai.sapling.as_ref().map(|si| &si.vk.fvk().vk) {
            let n = Note::from_parts(
                recipient,
                NoteValue::from_raw(note.value),
                Rseed::BeforeZip212(Fr::from_bytes(&note.rcm).unwrap()),
            );
            let nf = n.nf(&vk.nk, note.position as u64);
            note.nf = nf.0;
            note.tx.txid = txid;
        }
        Ok(())
    }
}
