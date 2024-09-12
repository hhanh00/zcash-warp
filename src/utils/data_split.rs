use anyhow::Result;
use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};

use crate::data::fb::{PacketT, PacketsT, Packets};
use crate::ffi::CParam;
use crate::{
    coin::COINS,
    ffi::{map_result_bytes, CResult},
};
use flatbuffers::{FlatBufferBuilder, WIPOffset};
use warp_macros::c_export;

const QR_DATA_SIZE: u16 = 256;

#[c_export]
pub fn split(data: &[u8], threshold: u32) -> Result<Vec<PacketT>> {
    let config = ObjectTransmissionInformation::with_defaults(data.len() as u64, QR_DATA_SIZE);
    let encoder = Encoder::new(data, config);
    let packets = encoder.get_encoded_packets(threshold);
    let packets = packets
        .iter()
        .map(|p| PacketT {
            data: Some(p.serialize()),
        })
        .collect::<Vec<_>>();
    Ok(packets)
}

#[c_export]
pub fn merge(parts: &PacketsT) -> Result<PacketT> {
    let config = ObjectTransmissionInformation::with_defaults(parts.len as u64, QR_DATA_SIZE);
    let packets = parts.packets.as_ref().unwrap()
        .iter()
        .map(|p| EncodingPacket::deserialize(p.data.as_ref().unwrap()))
        .collect::<Vec<_>>();
    let mut decoder = Decoder::new(config);
    for packet in packets {
        decoder.decode(packet);
    }
    let data = decoder.get_result();
    Ok(PacketT { data })
}
