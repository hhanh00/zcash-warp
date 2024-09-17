use anyhow::Result;
use zcash_address::ZcashAddress;
use zcash_client_backend::zip321::{Payment, TransactionRequest};
use zcash_protocol::{consensus::Network, value::Zatoshis};

use crate::{
    coin::COINS,
    ffi::{map_result, map_result_bytes, map_result_string, CParam, CResult},
};
use crate::{
    data::fb::{PaymentRequestT, PaymentRequests, PaymentRequestsT},
    pay::PaymentItem,
};
use flatbuffers::FlatBufferBuilder;
use std::ffi::{c_char, CStr};
use warp_macros::c_export;

use super::ua::decode_address;

#[c_export]
pub fn make_payment_uri(recipients: &PaymentRequestsT) -> Result<String> {
    let recipients = recipients
        .payments
        .as_ref()
        .unwrap()
        .iter()
        .map(|r| PaymentItem::try_from(r))
        .collect::<Result<Vec<_>, _>>()?;

    let payments = recipients
        .iter()
        .map(|r| {
            let recipient_address = ZcashAddress::try_from_encoded(&r.address)?;
            let amount = Zatoshis::from_u64(r.amount)?;
            let memo = r.memo.clone();
            let p = Payment::new(recipient_address, amount, memo, None, None, vec![])
                .ok_or(anyhow::anyhow!("Incompatible with Payment URI"));
            p
        })
        .collect::<Result<Vec<_>, _>>()?;
    let treq = TransactionRequest::new(payments)?;
    let uri = treq.to_uri();
    Ok(uri)
}

#[c_export]
pub fn parse_payment_uri(uri: &str) -> Result<PaymentRequestsT> {
    let treq = TransactionRequest::from_uri(uri)?;
    let recipients = treq
        .payments()
        .iter()
        .map(|(_, p)| PaymentRequestT {
            address: Some(p.recipient_address().encode()),
            amount: p.amount().into(),
            memo: None,
            memo_bytes: p.memo().cloned().map(|m| m.as_slice().to_vec()),
        })
        .collect::<Vec<_>>();
    let p = PaymentRequestsT {
        payments: Some(recipients),
    };
    Ok(p)
}

#[c_export]
pub fn is_valid_address_or_uri(network: &Network, s: &str) -> Result<u8> {
    let res = if decode_address(network, s).is_ok() {
        1
    } else if parse_payment_uri(s).is_ok() {
        2
    } else {
        0
    };
    Ok(res)
}
