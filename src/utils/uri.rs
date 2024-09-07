use anyhow::Result;
use zcash_address::ZcashAddress;
use zcash_client_backend::zip321::{Payment, TransactionRequest};
use zcash_protocol::value::Zatoshis;

use crate::pay::PaymentItem;

pub fn make_payment_uri(recipients: &[PaymentItem]) -> Result<String> {
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

pub fn parse_payment_uri(uri: &str) -> Result<crate::pay::Payment> {
    let treq = TransactionRequest::from_uri(uri)?;
    let recipients = treq.payments().iter().map(|(_, p)| PaymentItem {
        address: p.recipient_address().encode(),
        amount: p.amount().into(),
        memo: p.memo().cloned(),
    }).collect::<Vec<_>>();
    let p = crate::pay::Payment { recipients };
    Ok(p)
}
