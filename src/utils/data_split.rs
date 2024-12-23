use anyhow::Result;
use raptorq::{Decoder, Encoder, EncodingPacket, ObjectTransmissionInformation};

use crate::data::{PacketT, PacketsT};

const QR_DATA_SIZE: u16 = 256;

pub fn split(data: &[u8], threshold: u32) -> Result<Vec<PacketT>> {
    let length = data.len();
    let config = ObjectTransmissionInformation::with_defaults(length as u64, QR_DATA_SIZE);
    let encoder = Encoder::new(data, config);
    let packets = encoder.get_encoded_packets(threshold);
    let packets = packets
        .iter()
        .map(|p| PacketT {
            data: p.serialize(),
            len: length as u32,
        })
        .collect::<Vec<_>>();
    Ok(packets)
}

pub fn merge(parts: &PacketsT) -> Result<Vec<u8>> {
    let packets = &parts.packets;
    if packets.is_empty() {
        return Ok(vec![]);
    }
    let length = packets.first().unwrap().len;
    tracing::info!("{length}");
    let config = ObjectTransmissionInformation::with_defaults(length as u64, QR_DATA_SIZE);
    let packets = packets
        .iter()
        .map(|p| EncodingPacket::deserialize(&p.data))
        .collect::<Vec<_>>();
    let mut decoder = Decoder::new(config);
    for packet in packets {
        decoder.decode(packet);
    }
    let data = decoder.get_result();
    Ok(data.unwrap_or_default())
}
