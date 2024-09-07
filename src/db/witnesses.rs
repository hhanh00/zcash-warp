// use anyhow::Result;
// use incrementalmerkletree::witness::IncrementalWitness;
// use rusqlite::Connection;
// use sapling_crypto::Node;
// use tracing::info;

use crate::{
    warp::{AuthPath, Hasher, Witness, MERKLE_DEPTH},
    Hash,
};

// TODO: Use witness migration
// Read is no longer in librustzcash
// #[allow(dead_code)]
// pub fn get_witnesses_v1(
//     connection: &Connection,
//     height: u32,
//     pool: &'static str,
// ) -> Result<Vec<Witness>> {
//     let mut s = connection.prepare(&format!(
//         "SELECT note, witness FROM {pool}_witnesses WHERE height = ?1"
//     ))?;
//     let rows = s.query_map([height], |r| {
//         Ok((r.get::<_, u32>(0)?, r.get::<_, Vec<u8>>(1)?))
//     })?;
//     let mut ws = vec![];
//     for r in rows {
//         let (_note, witness) = r?;
//         let w = IncrementalWitness::<Node, MERKLE_DEPTH>::from_tree(&*witness)?; // reading as sapling is ok
//         info!("wroot {}", hex::encode(&w.root().repr));
//         let position = w.position() as u32;
//         let a = &w.tree;
//         let b = &w.filled;
//         let mut bit = b.iter().fuse();
//         let mut ommers = Edge::default();
//         let mut p = position;
//         for i in 0..MERKLE_DEPTH {
//             if p & 1 == 1 {
//                 ommers.0[i] = if i == 0 {
//                     a.left.map(|n| n.repr)
//                 } else {
//                     a.parents[i - 1].map(|n| n.repr)
//                 };
//             } else {
//                 ommers.0[i] = bit.next().map(|n| n.repr);
//             }
//             p /= 2;
//         }
//         let value = if position & 1 == 0 {
//             a.left.unwrap()
//         } else {
//             a.right.unwrap()
//         };
//         let w = Witness {
//             value: value.repr,
//             position,
//             ommers,
//         };
//         ws.push(w);
//     }

//     Ok(ws)
// }

impl Witness {
    pub fn build_auth_path(&self, edge: &AuthPath, empty_roots: &AuthPath) -> AuthPath {
        let mut path = AuthPath::default();
        let mut p = self.position;
        let mut edge_used = false;
        for i in 0..MERKLE_DEPTH as usize {
            let ommer = self.ommers.0[i];
            path.0[i] = match ommer {
                Some(o) => o,
                None => {
                    assert!(p & 1 == 0);
                    if edge_used {
                        empty_roots.0[i]
                    } else {
                        edge_used = true;
                        edge.0[i]
                    }
                }
            };
            p /= 2;
        }
        path
    }

    pub fn root<H: Hasher>(&self, edge: &AuthPath, h: &H) -> Hash {
        let mut hash = self.value;
        let mut p = self.position;
        let mut empty = h.empty();
        let mut edge_used = false;
        for i in 0..MERKLE_DEPTH as usize {
            let ommer = self.ommers.0[i];
            hash = match ommer {
                Some(o) => {
                    if p & 1 == 0 {
                        h.combine(i as u8, &hash, &o)
                    } else {
                        h.combine(i as u8, &o, &hash)
                    }
                }
                None => {
                    assert!(p & 1 == 0);
                    let o = if edge_used {
                        empty
                    } else {
                        edge_used = true;
                        edge.0[i as usize]
                    };
                    h.combine(i as u8, &hash, &o)
                }
            };
            empty = h.combine(i as u8, &empty, &empty);
            p /= 2;
        }
        hash
    }
}
