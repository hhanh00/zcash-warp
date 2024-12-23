use crate::data::UserMemoT;
use crate::network::Network;
use anyhow::Result;
use zcash_address::ZcashAddress;
use zcash_client_backend::zip321::{Payment, TransactionRequest};
use zcash_protocol::memo::Memo;
use zcash_protocol::{memo::MemoBytes, value::Zatoshis};

use crate::data::{PaymentRequestT, RecipientT};

use super::ua::{decode_address, filter_address};

pub fn make_payment_uri(network: &Network, payment: &PaymentRequestT) -> Result<String> {
    let payments = payment
        .recipients
        .iter()
        .map(|r| {
            let r = r.normalize_memo()?;
            let address = filter_address(network, &r.address, r.pools)?;
            let recipient_address = ZcashAddress::try_from_encoded(&address)?;
            let amount = Zatoshis::from_u64(r.amount)?;
            let memo = &r.memo_bytes;
            let memo = MemoBytes::from_bytes(memo)?;
            let memo = if recipient_address.can_receive_memo() {
                Some(memo)
            } else {
                None
            };
            let p = Payment::new(recipient_address, amount, memo, None, None, vec![])
                .ok_or(anyhow::anyhow!("Invalid Payment URI"));
            p
        })
        .collect::<Result<Vec<_>, _>>()?;
    let treq = TransactionRequest::new(payments)?;
    let uri = treq.to_uri();
    Ok(uri)
}

pub fn parse_payment_uri(
    #[allow(unused_variables)] network: &Network,
    uri: &str,
    height: u32,
    expiration: u32,
) -> Result<PaymentRequestT> {
    let treq = TransactionRequest::from_uri(uri)?; // this should include network
    let recipients = treq
        .payments()
        .iter()
        .map(|(_, p)| {
            let memo = p.memo().and_then(|m| Memo::try_from(m).ok());
            let memo_text = memo.and_then(|memo| match memo {
                Memo::Text(text_memo) => Some(text_memo),
                _ => None,
            });
            let memo_text = memo_text.map(|t| t.to_string());
            let user_memo = memo_text.map(|t| {
                UserMemoT {
                    reply_to: false,
                    sender: String::new(),
                    recipient: String::new(),
                    subject: String::new(),
                    body: t,
                }
            });

            RecipientT {
                address: p.recipient_address().encode(),
                amount: p.amount().into(),
                pools: 7,
                memo: user_memo,
                memo_bytes: p.memo().cloned().map(|m| m.as_slice().to_vec()).unwrap_or_default(),
            }
        })
        .collect::<Vec<_>>();
    let p = PaymentRequestT {
        recipients,
        src_pools: 7,
        sender_pay_fees: true,
        use_change: true,
        height,
        expiration,
    };
    Ok(p)
}

pub fn is_valid_address_or_uri(network: &Network, s: &str) -> Result<u8> {
    let res = if decode_address(network, s).is_ok() {
        1
    } else if parse_payment_uri(network, s, 0, 0).is_ok() {
        2
    } else {
        0
    };
    Ok(res)
}
