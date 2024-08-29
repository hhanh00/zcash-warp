use crate::Hash;

#[macro_export]
macro_rules! fb_to_bytes {
    ($v: ident) => {{
        let mut builder = FlatBufferBuilder::new();
        let backup_bytes = $v.pack(&mut builder);
        builder.finish(backup_bytes, None);
        Ok::<_, anyhow::Error>(builder.finished_data().to_vec())
    }};
}

pub fn to_txid_str(txid: &Hash) -> String {
    let mut txid = txid.clone();
    txid.reverse();
    hex::encode(&txid)
}
