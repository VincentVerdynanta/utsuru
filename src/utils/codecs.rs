use bytes::{BufMut, Bytes, BytesMut};
use webrtc::rtp::packetizer::Depacketizer;

pub const STAPA_NALU_TYPE: u8 = 24;
pub const FUA_NALU_TYPE: u8 = 28;
pub const FUB_NALU_TYPE: u8 = 29;
pub const SPS_NALU_TYPE: u8 = 7;
pub const PPS_NALU_TYPE: u8 = 8;
pub const AUD_NALU_TYPE: u8 = 9;
pub const FILLER_NALU_TYPE: u8 = 12;

pub const FUA_HEADER_SIZE: usize = 2;
pub const STAPA_HEADER_SIZE: usize = 1;
pub const STAPA_NALU_LENGTH_SIZE: usize = 2;

pub const NALU_TYPE_BITMASK: u8 = 0x1F;
pub const NALU_REF_IDC_BITMASK: u8 = 0x60;
pub const FU_START_BITMASK: u8 = 0x80;
pub const FU_END_BITMASK: u8 = 0x40;

pub const OUTPUT_STAP_AHEADER: u8 = 0x78;

pub static ANNEXB_NALUSTART_CODE: Bytes = Bytes::from_static(&[0x00, 0x00, 0x00, 0x01]);

/// H264Packet represents the H264 header that is stored in the payload of an RTP Packet
#[derive(PartialEq, Eq, Debug, Default, Clone)]
pub struct H264Packet {
    pub is_avc: bool,
    fua_buffer: Option<BytesMut>,
}

impl Depacketizer for H264Packet {
    /// depacketize parses the passed byte slice and stores the result in the H264Packet this method is called upon
    fn depacketize(&mut self, packet: &Bytes) -> Result<Bytes, webrtc::rtp::Error> {
        if packet.is_empty() {
            return Err(webrtc::rtp::Error::ErrShortPacket);
        }

        let mut payload = BytesMut::new();

        // NALU Types
        // https://tools.ietf.org/html/rfc6184#section-5.4
        let b0 = packet[0];
        let nalu_type = b0 & NALU_TYPE_BITMASK;

        match nalu_type {
            1..=23 => {
                if self.is_avc {
                    payload.put_u32(packet.len() as u32);
                } else {
                    payload.put(&*ANNEXB_NALUSTART_CODE);
                }
                payload.put(&*packet.clone());
                Ok(payload.freeze())
            }
            STAPA_NALU_TYPE => {
                let mut curr_offset = STAPA_HEADER_SIZE;
                while curr_offset + 1 < packet.len() {
                    let nalu_size =
                        ((packet[curr_offset] as usize) << 8) | packet[curr_offset + 1] as usize;
                    curr_offset += STAPA_NALU_LENGTH_SIZE;

                    if curr_offset + nalu_size > packet.len() {
                        return Err(webrtc::rtp::Error::StapASizeLargerThanBuffer(
                            nalu_size,
                            packet.len() - curr_offset,
                        ));
                    }

                    if self.is_avc {
                        payload.put_u32(nalu_size as u32);
                    } else {
                        payload.put(&*ANNEXB_NALUSTART_CODE);
                    }
                    payload.put(&*packet.slice(curr_offset..curr_offset + nalu_size));
                    curr_offset += nalu_size;
                }

                Ok(payload.freeze())
            }
            FUA_NALU_TYPE => {
                if packet.len() < FUA_HEADER_SIZE {
                    return Err(webrtc::rtp::Error::ErrShortPacket);
                }

                if self.fua_buffer.is_none() {
                    self.fua_buffer = Some(BytesMut::new());
                }

                if let Some(fua_buffer) = &mut self.fua_buffer {
                    fua_buffer.put(&*packet.slice(FUA_HEADER_SIZE..));
                }

                let b1 = packet[1];
                if b1 & FU_END_BITMASK != 0 {
                    let nalu_ref_idc = b0 & NALU_REF_IDC_BITMASK;
                    let fragmented_nalu_type = b1 & NALU_TYPE_BITMASK;

                    if let Some(fua_buffer) = self.fua_buffer.take() {
                        if self.is_avc {
                            payload.put_u32((fua_buffer.len() + 1) as u32);
                        } else {
                            payload.put(&*ANNEXB_NALUSTART_CODE);
                        }
                        payload.put_u8(nalu_ref_idc | fragmented_nalu_type);
                        payload.put(fua_buffer);
                    }

                    Ok(payload.freeze())
                } else {
                    Ok(Bytes::new())
                }
            }
            _ => Err(webrtc::rtp::Error::NaluTypeIsNotHandled(nalu_type)),
        }
    }

    /// is_partition_head checks if this is the head of a packetized nalu stream.
    fn is_partition_head(&self, payload: &Bytes) -> bool {
        if payload.len() < 2 {
            return false;
        }

        if payload[0] & NALU_TYPE_BITMASK == FUA_NALU_TYPE
            || payload[0] & NALU_TYPE_BITMASK == FUB_NALU_TYPE
        {
            (payload[1] & FU_START_BITMASK) != 0
        } else {
            true
        }
    }

    fn is_partition_tail(&self, marker: bool, _payload: &Bytes) -> bool {
        marker
    }
}
