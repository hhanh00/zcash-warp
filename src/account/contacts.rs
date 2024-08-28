use std::marker::PhantomData;

use anyhow::Result;
use orchard::Address;
use prost::bytes::Buf as _;
use rusqlite::Connection;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zcash_client_backend::address::{RecipientAddress, UnifiedAddress};
use zcash_primitives::{
    consensus::Network,
    legacy::TransparentAddress,
    memo::{Memo, MemoBytes},
    sapling::PaymentAddress,
};

use crate::{db::contacts::store_contact, types::Contact};

pub fn add_contact(
    network: &Network,
    connection: &Connection,
    account: u32,
    name: &str,
    address: &str,
) -> Result<()> {
    let a = RecipientAddress::decode(network, address).ok_or(anyhow::anyhow!("Invalid Address"))?;
    store_contact(connection, account, name, address, true)?;
    Ok(())
}

pub fn ua_of_orchard(address: &[u8; 43]) -> UnifiedAddress {
    let orchard = Address::from_raw_address_bytes(address).unwrap();
    let ua =
        zcash_client_backend::address::UnifiedAddress::from_receivers(Some(orchard), None, None)
            .unwrap();
    ua
}

const CONTACT_COOKIE_v2: u32 = 0x434E5441;

pub trait ChunkedMemoData {
    const COOKIE: u32;
    type Data: DeserializeOwned;
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
            let (n, data) = Self::decode_box(&bytes)?;
            self.has_data = true;
            self.chunks[n as usize] = data;
        }

        Ok(())
    }

    pub fn finalize(&self) -> anyhow::Result<Vec<T::Data>> {
        if !self.has_data {
            return Ok(Vec::new());
        }
        let data: Vec<_> = self.chunks.iter().flatten().cloned().collect();
        let contacts = bincode::deserialize::<Vec<T::Data>>(&data)?;
        Ok(contacts)
    }

    fn decode_box(bb: &[u8; 511]) -> anyhow::Result<(u8, Vec<u8>)> {
        let mut bb: &[u8] = bb;
        let magic = bb.get_u32();
        if magic != T::COOKIE {
            anyhow::bail!("Not a contact record");
        }
        let n = bb.get_u8();
        let len = bb.get_u16() as usize;
        if len > bb.len() {
            anyhow::bail!("Buffer overflow");
        }

        let data = &bb[0..len];
        Ok((n, data.to_vec()))
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
        RecipientAddress::Shielded(sa) => {
            s = Some(sa.clone());
        }
        RecipientAddress::Unified(ua) => {
            t = ua.transparent().cloned();
            s = ua.sapling().cloned();
            o = ua.orchard().cloned();
        }
    }
    Ok((t, s, o))
}
