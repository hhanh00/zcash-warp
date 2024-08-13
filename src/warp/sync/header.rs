use std::collections::HashMap;

use anyhow::Result;

use crate::warp::BlockHeader;

pub struct BlockHeaderStore {
    pub heights: HashMap<u32, Option<BlockHeader>>,
}

impl BlockHeaderStore {
    pub fn new() -> Self {
        Self {
            heights: HashMap::new(),
        }
    }

    pub fn add_heights(&mut self, heights: &[u32]) -> Result<()> {
        for h in heights {
            self.heights.insert(*h, None);
        }
        Ok(())
    }

    pub fn process(&mut self, header: &BlockHeader) -> Result<()> {
        if self.heights.contains_key(&header.height) {
            self.heights.insert(header.height, Some(header.clone()));
        }
        Ok(())
    }
}
