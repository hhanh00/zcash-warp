use lazy_static::lazy_static;
use zcash_protocol::{
    consensus::{BlockHeight, MainNetwork, NetworkUpgrade, Parameters},
    local_consensus::LocalNetwork,
};

lazy_static! {
    pub static ref REGTEST: Network = Network::Regtest(regtest());
}

#[derive(Copy, Clone, Debug)]
pub enum Network {
    Main,
    Regtest(LocalNetwork),
}

impl Parameters for Network {
    fn network_type(&self) -> zcash_address::Network {
        match self {
            Network::Main => MainNetwork.network_type(),
            Network::Regtest(n) => n.network_type(),
        }
    }

    fn activation_height(
        &self,
        nu: NetworkUpgrade,
    ) -> Option<zcash_protocol::consensus::BlockHeight> {
        match self {
            Network::Main => MainNetwork.activation_height(nu),
            Network::Regtest(n) => n.activation_height(nu),
        }
    }
}

pub fn regtest() -> LocalNetwork {
    LocalNetwork {
        overwinter: Some(BlockHeight::from_u32(1)),
        sapling: Some(BlockHeight::from_u32(1)),
        blossom: Some(BlockHeight::from_u32(1)),
        heartwood: Some(BlockHeight::from_u32(1)),
        canopy: Some(BlockHeight::from_u32(1)),
        nu5: Some(BlockHeight::from_u32(1)),
        nu6: None,
    }
}
