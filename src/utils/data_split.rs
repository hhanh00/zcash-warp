use anyhow::Result;
use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};
use rusqlite::Connection;
use warp_macros::c_export;
use zcash_protocol::consensus::Network;
use crate::coin::COINS;
use crate::ffi::{CResult, map_result};
use std::ffi::{c_char, CStr};

use crate::Client;

const QR_DATA_SIZE: u16 = 256;

#[c_export]
pub async fn test(network: &Network, client: &mut Client, account: u32, s: &str) -> Result<u8> { Ok(0) }

pub fn split(data: &[u8], threshold: u32) -> Result<Vec<Vec<u8>>> {
    let config = ObjectTransmissionInformation::with_defaults(data.len() as u64, QR_DATA_SIZE);
    let encoder = Encoder::new(data, config);
    let packets = encoder.get_encoded_packets(threshold);
    let packets = packets.iter().map(|p| p.serialize()).collect::<Vec<_>>();
    Ok(packets)
}

pub fn merge(parts: &[Vec<u8>], data_len: usize) -> Result<Option<Vec<u8>>> {
    let config = ObjectTransmissionInformation::with_defaults(data_len as u64, QR_DATA_SIZE);
    let packets = parts.iter().map(|p| EncodingPacket::deserialize(p)).collect::<Vec<_>>();
    let mut decoder = Decoder::new(config);
    for packet in packets {
        decoder.decode(packet);        
    }
    let data = decoder.get_result();
    Ok(data)
}
