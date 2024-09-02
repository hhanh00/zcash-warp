use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{
    lwd::{
        get_compact_block, get_tree_state,
        rpc::{CompactBlock, CompactTx},
    },
    warp::{
        hasher::{OrchardHasher, SaplingHasher},
        legacy::CommitmentTreeFrontier,
        Hasher, MERKLE_DEPTH,
    },
    Client, Hash,
};

#[derive(Serialize, Deserialize, Debug)]
pub struct BridgeLevel {
    pub head: Option<Either>,
    pub tail: Option<Either>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Bridge {
    pub start: u32,
    pub len: u32,
    pub levels: Vec<BridgeLevel>,
    pub s: i32,
    pub e: i32,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Either {
    #[serde(with = "serde_bytes")]
    Left(Hash),
    #[serde(with = "serde_bytes")]
    Right(Hash),
}

enum Node {
    Keep(Hash),
    Discard(Hash),
}

pub trait CompactTxCMXExtractor {
    fn items(tx: &CompactTx) -> impl Iterator<Item = Hash>;
    fn len(tx: &CompactTx) -> usize;
}

impl CompactTxCMXExtractor for SaplingHasher {
    fn len(tx: &CompactTx) -> usize {
        tx.outputs.len()
    }

    fn items(tx: &CompactTx) -> impl Iterator<Item = Hash> {
        tx.outputs.iter().map(|o| o.cmu.clone().try_into().unwrap())
    }
}

impl CompactTxCMXExtractor for OrchardHasher {
    fn len(tx: &CompactTx) -> usize {
        tx.actions.len()
    }

    fn items(tx: &CompactTx) -> impl Iterator<Item = Hash> {
        tx.actions.iter().map(|a| a.cmx.clone().try_into().unwrap())
    }
}

pub fn compact_tree<H: Hasher + CompactTxCMXExtractor>(
    start: &CommitmentTreeFrontier,
    block: &CompactBlock,
    hasher: &H,
) -> Result<Vec<Bridge>> {
    let empty_cmx = Node::Keep([0u8; 32]);

    let start_position = start.size() as u32;
    let start_edge = start.to_edge(hasher);

    let mut position = start_position;
    let mut bridges = vec![];
    let mut nodes: Vec<Node> = vec![];
    for tx in block.vtx.iter() {
        let n_outputs = H::len(tx) as u32;
        if n_outputs >= 32 {
            bridges.push(Bridge {
                start: position,
                len: n_outputs,
                levels: vec![],
                s: position as i32,
                e: (position + n_outputs - 1) as i32,
            });
            nodes.extend(H::items(tx).map(|o| Node::Discard(o)));
        } else {
            nodes.extend(H::items(tx).map(|o| Node::Keep(o)));
        }
        position += n_outputs;
    }

    let mut p = start_position as i32;
    for depth in 0..MERKLE_DEPTH as u32 {
        if p % 2 != 0 {
            nodes.insert(0, Node::Keep(start_edge.0[depth as usize].unwrap()));
            p -= 1;
        }

        // println!("before ===");
        // for n in nodes.iter() {
        //     match n {
        //         Node::Keep(_) => print!("+"),
        //         Node::Discard(_) => print!("."),
        //     }
        // }
        // println!("===");

        for bridge in bridges.iter_mut() {
            // save the bridge level

            let s = bridge.s as i32;
            let e = bridge.e as i32;

            // println!(">> {p} {s} {e}");
            if s > e || nodes.is_empty() {
                continue;
            }

            // S is the first node of the bridge
            // Since we are going to replace every node with a dummy
            // we need to save the node that can be an ommer in
            // auth path
            let mut head = None;
            let idx = (s - p) as usize;
            // println!("{idx}");
            let (i, left, right) = if s % 2 == 0 {
                let r = if idx + 1 < nodes.len() {
                    &nodes[idx + 1]
                } else {
                    &empty_cmx
                };
                (idx, &nodes[idx], r)
            } else {
                (idx - 1, &nodes[idx - 1], &nodes[idx])
            };
            match (left, right) {
                (Node::Keep(_), Node::Keep(_)) => {}
                (Node::Keep(_), Node::Discard(r)) => {
                    head = Some(Either::Right(r.clone()));
                    nodes[i + 1] = Node::Keep(r.clone());
                }
                (Node::Discard(l), Node::Keep(_)) => {
                    head = Some(Either::Left(l.clone()));
                    nodes[i] = Node::Keep(l.clone());
                }
                (Node::Discard(_), Node::Discard(_)) => {}
            }

            let mut tail = None;
            let idx = (e - p) as usize;
            // println!("{idx}");
            let (i, left, right) = if e % 2 == 0 {
                let r = if idx + 1 < nodes.len() {
                    &nodes[idx + 1]
                } else {
                    &empty_cmx
                };
                (idx, &nodes[idx], r)
            } else {
                (idx - 1, &nodes[idx - 1], &nodes[idx])
            };
            match (left, right) {
                (Node::Keep(_), Node::Keep(_)) => {}
                (Node::Keep(_), Node::Discard(r)) => {
                    tail = Some(Either::Right(r.clone()));
                    nodes[i + 1] = Node::Keep(r.clone());
                }
                (Node::Discard(l), Node::Keep(_)) => {
                    tail = Some(Either::Left(l.clone()));
                    nodes[i] = Node::Keep(l.clone());
                }
                (Node::Discard(_), Node::Discard(_)) => {}
            }

            bridge.s = s / 2;
            bridge.e = (e - 1) / 2;
            let level = BridgeLevel { head, tail };
            bridge.levels.push(level);
        }

        // println!("=== after");
        // for n in nodes.iter() {
        //     match n {
        //         Node::Keep(_) => print!("+"),
        //         Node::Discard(_) => print!("."),
        //     }
        // }
        // println!("===");

        let pairs = nodes.len() / 2;

        let mut next_nodes = vec![];
        for i in 0..pairs {
            let lhs = &nodes[i * 2];
            let rhs = &nodes[i * 2 + 1];
            let h = match (lhs, rhs) {
                (Node::Keep(l), Node::Keep(r)) => Node::Keep(hasher.combine(depth as u8, l, r)),
                (Node::Discard(l), Node::Discard(r)) => {
                    Node::Discard(hasher.combine(depth as u8, l, r))
                }
                (Node::Keep(_), Node::Discard(_)) => unreachable!("{depth} {i} {}", nodes.len()),
                (Node::Discard(_), Node::Keep(_)) => unreachable!("{depth} {i}"),
            };
            next_nodes.push(h);
        }

        std::mem::swap(&mut nodes, &mut next_nodes);
        p /= 2;
    }

    Ok(bridges)
}

pub fn verify_compact_block<H: Hasher + CompactTxCMXExtractor>(
    start: &CommitmentTreeFrontier,
    block: &CompactBlock,
    bridges: &mut [Bridge],
    hasher: &H,
) -> Result<()> {
    let start_position = start.size() as u32;
    let start_edge = start.to_edge(hasher);

    let mut p = start_position as i32;
    for bridge in bridges.iter_mut() {
        bridge.s = bridge.start as i32;
        bridge.e = (bridge.start + bridge.len - 1) as i32;
    }

    let mut nodes: Vec<Option<Hash>> = vec![];
    for tx in block.vtx.iter() {
        nodes.extend(H::items(tx).map(|o| Some(o)));
    }

    // clear out the hashes from the bridge
    for bridge in bridges.iter() {
        let s = bridge.start - p as u32;
        for i in 0..bridge.len {
            nodes[(s + i) as usize] = None;
        }
    }

    for depth in 0..MERKLE_DEPTH as u32 {
        if p % 2 != 0 {
            nodes.insert(0, Some(start_edge.0[depth as usize].unwrap()));
            p -= 1;
        }

        for bridge in bridges.iter_mut() {
            let s = bridge.s;
            let e = bridge.e;

            if depth < bridge.levels.len() as u32 {
                let level = &bridge.levels[depth as usize];
                if let Some(h) = &level.head {
                    let i = if s % 2 != 0 { s - p - 1 } else { s - p } as usize;
                    match h {
                        Either::Left(h) => {
                            nodes[i] = Some(h.clone());
                        }
                        Either::Right(h) => {
                            nodes[i + 1] = Some(h.clone());
                        }
                    }
                }
                if let Some(h) = &level.tail {
                    let i = if e % 2 != 0 { e - p - 1 } else { e - p } as usize;
                    match h {
                        Either::Left(h) => {
                            nodes[i] = Some(h.clone());
                        }
                        Either::Right(h) => {
                            nodes[i + 1] = Some(h.clone());
                        }
                    }
                }
                bridge.s = bridge.s / 2;
                bridge.e = (bridge.e - 1) / 2;
            }
        }

        let pairs = nodes.len() / 2;
        let mut next_nodes = vec![];
        for i in 0..pairs {
            let lhs = &nodes[i * 2];
            let rhs = &nodes[i * 2 + 1];
            let h = match (lhs, rhs) {
                (None, None) => None,
                (Some(x), Some(y)) => Some(hasher.combine(depth as u8, x, y)),
                _ => {
                    unreachable!("{} {:?} {:?}", i * 2, lhs, rhs);
                }
            };
            next_nodes.push(h);
        }
        std::mem::swap(&mut nodes, &mut next_nodes);
        p /= 2;
    }
    Ok(())
}

pub fn purge_block(
    mut block: CompactBlock,
    s: &CommitmentTreeFrontier,
    o: &CommitmentTreeFrontier,
) -> Result<CompactBlock> {
    let s_bridges = compact_tree(s, &block, &SaplingHasher::default())?;
    let o_bridges = compact_tree(o, &block, &OrchardHasher::default())?;

    let mut sit = s_bridges.into_iter();
    let mut oit = o_bridges.into_iter();

    for tx in block.vtx.iter_mut() {
        if tx.outputs.len() >= 32 {
            tx.outputs.clear();
            tx.spends.clear();
            tx.sapling_bridge = Some(sit.next().unwrap().to_rpc_bridge());
        }
        if tx.actions.len() >= 32 {
            tx.actions.clear();
            tx.orchard_bridge = Some(oit.next().unwrap().to_rpc_bridge());
        }
    }
    Ok(block)
}

pub async fn test_compact_block(client: &mut Client) -> anyhow::Result<()> {
    for h in 1800000..1801000 {
        tracing::info!("{h}");
        let hasher = SaplingHasher::default();
        let (s, o) = get_tree_state(client, h).await?;
        let block = get_compact_block(client, h + 1).await?;
        let mut bridges = compact_tree(&o, &block, &hasher)?;
        let r = serde_cbor::ser::to_vec(&bridges)?;
        println!("{}", hex::encode(&r));
        verify_compact_block(&o, &block, &mut bridges, &hasher)?;
    }

    Ok(())
}

impl Bridge {
    pub fn to_rpc_bridge(self) -> crate::lwd::rpc::Bridge {
        let levels = self
            .levels
            .into_iter()
            .map(|l| crate::lwd::rpc::BridgeLevel {
                head: l.head.map(|e| e.to_rpc_either()),
                tail: l.tail.map(|e| e.to_rpc_either()),
            })
            .collect::<Vec<_>>();

        crate::lwd::rpc::Bridge {
            len: self.len,
            levels,
        }
    }
}

impl Either {
    pub fn to_rpc_either(self) -> crate::lwd::rpc::Either {
        match self {
            Either::Left(h) => crate::lwd::rpc::Either {
                side: Some(crate::lwd::rpc::either::Side::Left(h.to_vec())),
            },
            Either::Right(h) => crate::lwd::rpc::Either {
                side: Some(crate::lwd::rpc::either::Side::Right(h.to_vec())),
            },
        }
    }
}
