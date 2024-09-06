use crate::Hash;

pub mod ua;
pub mod uri;

#[macro_export]
macro_rules! fb_to_bytes {
    ($v: ident) => {{
        let mut builder = FlatBufferBuilder::new();
        let backup_bytes = $v.pack(&mut builder);
        builder.finish(backup_bytes, None);
        Ok::<_, anyhow::Error>(builder.finished_data().to_vec())
    }};
}

#[macro_export]
macro_rules! fb_vec_to_bytes {
    ($vs: ident, $T: ident) => {{
        let mut builder = FlatBufferBuilder::new();
        let mut os = vec![];
        for v in $vs.iter() {
            let o = v.pack(&mut builder);
            builder.push(o);
            os.push(o);
        }
        builder.start_vector::<WIPOffset<$T>>($vs.len());
        for o in os {
            builder.push(o);
        }
        let o = builder.end_vector::<WIPOffset<$T>>($vs.len());
        builder.finish(o, None);
        let data = builder.finished_data();
        Ok::<_, anyhow::Error>(data.to_vec())
    }};
}

pub fn to_txid_str(txid: &Hash) -> String {
    let mut txid = txid.clone();
    txid.reverse();
    hex::encode(&txid)
}
