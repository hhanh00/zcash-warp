use anyhow::Result;
use zcash_address::ZcashAddress;
use zcash_client_backend::zip321::{Payment, TransactionRequest};
use zcash_protocol::{memo::MemoBytes, value::Zatoshis};
use crate::network::Network;

use crate::{
    coin::COINS,
    data::fb::{RecipientT, PaymentRequest, PaymentRequestT},
    ffi::{map_result, map_result_bytes, map_result_string, CParam, CResult},
};
use flatbuffers::FlatBufferBuilder;
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

use super::ua::{decode_address, filter_address};

#[c_export]
pub fn make_payment_uri(network: &Network, payment: &PaymentRequestT) -> Result<String> {
    let payments = payment
        .recipients
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| {
            let r = r.normalize_memo()?;
            let address = filter_address(network, r.address.as_ref().unwrap(), r.pools)?;
            let recipient_address = ZcashAddress::try_from_encoded(&address)?;
            let amount = Zatoshis::from_u64(r.amount)?;
            let memo = r.memo_bytes.as_ref().unwrap();
            let memo = MemoBytes::from_bytes(memo)?;
            let memo = if recipient_address.can_receive_memo() { Some(memo) } else { None };
            let p = Payment::new(recipient_address, amount, memo, None, None, vec![])
                .ok_or(anyhow::anyhow!("Invalid Payment URI"));
            p
        })
        .collect::<Result<Vec<_>, _>>()?;
    let treq = TransactionRequest::new(payments)?;
    let uri = treq.to_uri();
    Ok(uri)
}

#[c_export]
pub fn parse_payment_uri(uri: &str, height: u32, expiration: u32) -> Result<PaymentRequestT> {
    let treq = TransactionRequest::from_uri(uri)?;
    let recipients = treq
        .payments()
        .iter()
        .map(|(_, p)| RecipientT {
            address: Some(p.recipient_address().encode()),
            amount: p.amount().into(),
            pools: 7,
            memo: None,
            memo_bytes: p.memo().cloned().map(|m| m.as_slice().to_vec()),
        })
        .collect::<Vec<_>>();
    let p = PaymentRequestT {
        recipients: Some(recipients),
        src_pools: 7,
        sender_pay_fees: true,
        use_change: true,
        height,
        expiration,
    };
    Ok(p)
}

#[c_export]
pub fn is_valid_address_or_uri(network: &Network, s: &str) -> Result<u8> {
    let res = if decode_address(network, s).is_ok() {
        1
    } else if parse_payment_uri(s, 0, 0).is_ok() {
        2
    } else {
        0
    };
    Ok(res)
}
