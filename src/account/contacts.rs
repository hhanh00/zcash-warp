use std::marker::PhantomData;

use anyhow::Result;
use orchard::Address;
use prost::bytes::{Buf as _, BufMut as _};
use rusqlite::Connection;
use sapling_crypto::PaymentAddress;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zcash_keys::address::Address as RecipientAddress;
use zcash_primitives::{
    legacy::TransparentAddress,
    memo::{Memo, MemoBytes},
};
use zcash_protocol::consensus::Network;

use crate::{
    data::fb::{ContactCardT, PaymentRequestT, RecipientT},
    db::{
        account::get_account_info,
        contacts::{get_unsaved_contacts, store_contact},
    },
    pay::{make_payment, UnsignedTransaction},
    types::{CheckpointHeight, PoolMask},
    warp::legacy::CommitmentTreeFrontier,
};

use crate::{
    coin::COINS,
    ffi::{map_result, CResult},
};
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

#[c_export]
pub fn add_contact(
    connection: &Connection,
    account: u32,
    name: &str,
    address: &str,
    saved: bool,
) -> Result<()> {
    let contact = ContactCardT {
        id: 0,
        account,
        name: Some(name.to_string()),
        address: Some(address.to_string()),
        saved,
    };
    store_contact(connection, &contact)?;
    Ok(())
}

pub fn serialize_contacts(contacts: &[ContactV1]) -> Result<Vec<Memo>> {
    let cs_bin = bincode::serialize(&contacts)?;
    let chunks = cs_bin.chunks(500);
    let memos: Vec<_> = chunks
        .enumerate()
        .map(|(i, c)| {
            let n = i as u8;
            let mut bytes = [0u8; 511];
            let mut bb: Vec<u8> = vec![];
            bb.put_u32(ChunkedContactV1::COOKIE);
            bb.put_u8(n);
            bb.put_u16(c.len() as u16);
            bb.put_slice(c);
            bytes[0..bb.len()].copy_from_slice(&bb);
            Memo::Arbitrary(Box::new(bytes))
        })
        .collect();

    Ok(memos)
}

const MIN_AMOUNT: u64 = 10_000;

pub fn commit_unsaved_contacts(
    network: &Network,
    connection: &Connection,
    account: u32,
    src_pools: u8,
    cp_height: CheckpointHeight,
    s: &CommitmentTreeFrontier,
    o: &CommitmentTreeFrontier,
) -> anyhow::Result<UnsignedTransaction> {
    let ai = get_account_info(network, connection, account)?;
    let address = ai.to_address(network, PoolMask(src_pools)).unwrap();
    tracing::info!("Contact -> {}", address);
    let contacts = get_unsaved_contacts(connection, account)?;
    let contacts = contacts
        .into_iter()
        .map(|c| ContactV1 {
            id: 0,
            name: c.name.unwrap(),
            address: c.address.unwrap(),
        })
        .collect::<Vec<_>>();
    let memos = serialize_contacts(&contacts)?;
    let recipients = memos
        .iter()
        .map(|m| {
            let memo = MemoBytes::from(m);
            RecipientT {
                address: Some(address.clone()),
                amount: MIN_AMOUNT,
                memo: None,
                pools: 7,
                memo_bytes: Some(memo.as_slice().to_vec()),
            }
        })
        .collect::<Vec<_>>();
    let payment = PaymentRequestT {
        recipients: Some(recipients),
        src_pools,
        sender_pay_fees: true,
        use_change: true,
        height: cp_height.0,
        expiration: cp_height.0 + 50,
    };
    let utx = make_payment(
        network,
        connection,
        account,
        &payment,
        s,
        o,
    )?;
    Ok(utx)
}

pub trait ChunkedMemoData {
    const COOKIE: u32;
    type Data: DeserializeOwned + std::fmt::Debug;
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ContactV1 {
    pub id: u32,
    pub name: String,
    pub address: String,
}

pub struct ChunkedContactV1;

impl ChunkedMemoData for ChunkedContactV1 {
    const COOKIE: u32 = 0x434E5440;
    type Data = ContactV1;
}

pub struct ChunkedMemoDecoder<T: ChunkedMemoData> {
    has_data: bool,
    chunks: Vec<Vec<u8>>,
    _phantom: PhantomData<T>,
}

impl<T: ChunkedMemoData> ChunkedMemoDecoder<T> {
    pub fn new(n: usize) -> Self {
        let mut chunks = vec![];
        chunks.resize(n, vec![]);
        Self {
            has_data: false,
            chunks,
            _phantom: PhantomData::default(),
        }
    }

    pub fn add_memo(&mut self, memo: &MemoBytes) -> anyhow::Result<()> {
        let memo = Memo::try_from(memo.clone())?;
        if let Memo::Arbitrary(bytes) = memo {
            if let Some((n, data)) = Self::decode_box(&bytes)? {
                self.has_data = true;
                self.chunks[n as usize] = data;
            }
        }

        Ok(())
    }

    pub fn finalize(&self) -> anyhow::Result<Vec<T::Data>> {
        if !self.has_data {
            return Ok(Vec::new());
        }
        let data: Vec<_> = self.chunks.iter().flatten().cloned().collect();
        let contacts = bincode::deserialize::<Vec<T::Data>>(&data)?;
        tracing::info!("{contacts:?}");
        Ok(contacts)
    }

    fn decode_box(bb: &[u8; 511]) -> anyhow::Result<Option<(u8, Vec<u8>)>> {
        let mut bb: &[u8] = bb;
        let magic = bb.get_u32();
        if magic != T::COOKIE {
            return Ok(None);
        }
        let n = bb.get_u8();
        let len = bb.get_u16() as usize;
        if len > bb.len() {
            anyhow::bail!("Buffer overflow");
        }

        let data = &bb[0..len];
        Ok(Some((n, data.to_vec())))
    }
}

// true if lhs and rhs has at least one receiver in common
pub fn recipient_contains(lhs: &RecipientAddress, rhs: &RecipientAddress) -> Result<bool> {
    let (t1, s1, o1) = decompose_recipient(&lhs)?;
    let (t2, s2, o2) = decompose_recipient(&rhs)?;
    let t = t1.zip(t2).map(|(a, b)| a == b).unwrap_or_default();
    let s = s1.zip(s2).map(|(a, b)| a == b).unwrap_or_default();
    let o = o1.zip(o2).map(|(a, b)| a == b).unwrap_or_default();
    Ok(t || s || o)
}

pub fn decompose_recipient(
    a: &RecipientAddress,
) -> Result<(
    Option<TransparentAddress>,
    Option<PaymentAddress>,
    Option<Address>,
)> {
    let mut t = None;
    let mut s = None;
    let mut o = None;
    match a {
        RecipientAddress::Transparent(ta) => {
            t = Some(ta.clone());
        }
        RecipientAddress::Sapling(sa) => {
            s = Some(sa.clone());
        }
        RecipientAddress::Unified(ua) => {
            t = ua.transparent().cloned();
            s = ua.sapling().cloned();
            o = ua.orchard().cloned();
        }
        RecipientAddress::Tex(ta) => {
            t = Some(TransparentAddress::PublicKeyHash(ta.clone()));
        }
    }
    Ok((t, s, o))
}
