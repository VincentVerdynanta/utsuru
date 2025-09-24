use std::{fmt, io::Write};
use tracing::trace;

use super::{
    bitstream::{BitWriter, BitWriterError},
    h264_parser::{
        DEFAULT_4X4_INTER, DEFAULT_4X4_INTRA, DEFAULT_8X8_INTER, DEFAULT_8X8_INTRA, HrdParams, Sps,
    },
};

/// Extended Sample Aspect Ratio - H.264 Table E-1
const EXTENDED_SAR: u8 = 255;

/// Internal wrapper over [`std::io::Write`] for possible emulation prevention
struct EmulationPrevention<W: Write> {
    out: W,
    prev_bytes: [Option<u8>; 2],

    /// Emulation prevention enabled.
    ep_enabled: bool,
}

impl<W: Write> EmulationPrevention<W> {
    fn new(writer: W, ep_enabled: bool) -> Self {
        Self {
            out: writer,
            prev_bytes: [None; 2],
            ep_enabled,
        }
    }

    fn write_byte(&mut self, curr_byte: u8) -> std::io::Result<()> {
        if self.prev_bytes[1] == Some(0x00) && self.prev_bytes[0] == Some(0x00) && curr_byte <= 0x03
        {
            self.out.write_all(&[0x00, 0x00, 0x03, curr_byte])?;
            self.prev_bytes = [None; 2];
        } else {
            if let Some(byte) = self.prev_bytes[1] {
                self.out.write_all(&[byte])?;
            }

            self.prev_bytes[1] = self.prev_bytes[0];
            self.prev_bytes[0] = Some(curr_byte);
        }

        Ok(())
    }

    /// Writes a H.264 NALU header.
    fn write_header(&mut self, idc: u8, type_: u8) -> SynthesizerResult<()> {
        self.out.write_all(&[
            0x00,
            0x00,
            0x00,
            0x01,
            (idc & 0b11) << 5 | (type_ & 0b11111),
        ])?;

        Ok(())
    }

    fn has_data_pending(&self) -> bool {
        self.prev_bytes[0].is_some() || self.prev_bytes[1].is_some()
    }
}

impl<W: Write> Write for EmulationPrevention<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if !self.ep_enabled {
            self.out.write_all(buf)?;
            return Ok(buf.len());
        }

        for byte in buf {
            self.write_byte(*byte)?;
        }

        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if let Some(byte) = self.prev_bytes[1].take() {
            self.out.write_all(&[byte])?;
        }

        if let Some(byte) = self.prev_bytes[0].take() {
            self.out.write_all(&[byte])?;
        }

        self.out.flush()
    }
}

impl<W: Write> Drop for EmulationPrevention<W> {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            trace!("Unable to flush pending bytes {e:?}");
        }
    }
}

#[derive(Debug)]
pub enum SynthesizerError {
    Overflow,
    Io(std::io::Error),
    BitWriterError(BitWriterError),
}

impl fmt::Display for SynthesizerError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            SynthesizerError::Overflow => write!(f, "value increment caused value overflow"),
            SynthesizerError::Io(x) => write!(f, "{}", x),
            SynthesizerError::BitWriterError(x) => write!(f, "{}", x),
        }
    }
}

impl From<std::io::Error> for SynthesizerError {
    fn from(err: std::io::Error) -> Self {
        SynthesizerError::Io(err)
    }
}

impl From<BitWriterError> for SynthesizerError {
    fn from(err: BitWriterError) -> Self {
        SynthesizerError::BitWriterError(err)
    }
}

pub type SynthesizerResult<T> = std::result::Result<T, SynthesizerError>;

/// A writer for H.264 bitstream. It is capable of outputing bitstream with
/// emulation-prevention.
pub struct Synthesizer<W: Write>(BitWriter<EmulationPrevention<W>>);

impl<W: Write> Synthesizer<W> {
    pub fn new(writer: W, ep_enabled: bool) -> Self {
        Self(BitWriter::new(EmulationPrevention::new(writer, ep_enabled)))
    }

    /// Writes fixed bit size integer (up to 32 bit) output with emulation
    /// prevention if enabled. Corresponds to `f(n)` in H.264 spec.
    pub fn write_f<T: Into<u32>>(&mut self, bits: usize, value: T) -> SynthesizerResult<usize> {
        self.0
            .write_f(bits, value)
            .map_err(SynthesizerError::BitWriterError)
    }

    /// An alias to [`Self::write_f`] Corresponds to `n(n)` in H.264 spec.
    pub fn write_u<T: Into<u32>>(&mut self, bits: usize, value: T) -> SynthesizerResult<usize> {
        self.write_f(bits, value)
    }

    /// Writes a number in exponential golumb format.
    pub fn write_exp_golumb(&mut self, value: u32) -> SynthesizerResult<()> {
        let value = value.checked_add(1).ok_or(SynthesizerError::Overflow)?;
        let bits = 32 - value.leading_zeros() as usize;
        let zeros = bits - 1;

        self.write_f(zeros, 0u32)?;
        self.write_f(bits, value)?;

        Ok(())
    }

    /// Writes a unsigned integer in exponential golumb format.
    /// Coresponds to `ue(v)` in H.264 spec.
    pub fn write_ue<T: Into<u32>>(&mut self, value: T) -> SynthesizerResult<()> {
        let value = value.into();

        self.write_exp_golumb(value)
    }

    /// Writes a signed integer in exponential golumb format.
    /// Coresponds to `se(v)` in H.264 spec.
    pub fn write_se<T: Into<i32>>(&mut self, value: T) -> SynthesizerResult<()> {
        let value: i32 = value.into();
        let abs_value: u32 = value.unsigned_abs();

        if value <= 0 {
            self.write_ue(2 * abs_value)
        } else {
            self.write_ue(2 * abs_value - 1)
        }
    }

    /// Returns `true` if ['Self`] hold data that wasn't written to [`std::io::Write`]
    pub fn has_data_pending(&self) -> bool {
        self.0.has_data_pending() || self.0.inner().has_data_pending()
    }

    /// Writes a H.264 NALU header.
    pub fn write_header(&mut self, idc: u8, _type: u8) -> SynthesizerResult<()> {
        self.0.flush()?;
        self.0.inner_mut().write_header(idc, _type)?;
        Ok(())
    }

    /// Returns `true` if next bits will be aligned to 8
    pub fn aligned(&self) -> bool {
        !self.0.has_data_pending()
    }
}

fn scaling_list<W>(s: &mut Synthesizer<W>, list: &[u8], default: &[u8]) -> SynthesizerResult<()>
where
    W: Write,
{
    // H.264 7.3.2.1.1.1
    if list == default {
        s.write_se(-8)?;
        return Ok(());
    }

    // The number of list values we want to encode.
    let mut run = list.len();

    // Check how many values at the end of the matrix are the same,
    // so we can save on encoding those.
    for j in (1..list.len()).rev() {
        if list[j - 1] != list[j] {
            break;
        }
        run -= 1;
    }

    // Encode deltas.
    let mut last_scale = 8;
    for scale in &list[0..run] {
        let delta_scale = *scale as i32 - last_scale;
        s.write_se(delta_scale)?;
        last_scale = *scale as i32;
    }

    // Didn't encode all values, encode -|last_scale| to set decoder's
    // |next_scale| (H.264 7.3.2.1.1.1) to zero, i.e. decoder should repeat
    // last values in matrix.
    if run < list.len() {
        s.write_se(-last_scale)?;
    }

    Ok(())
}

fn default_scaling_list(i: usize) -> &'static [u8] {
    // H.264 Table 7-2
    match i {
        0 => &DEFAULT_4X4_INTRA[..],
        1 => &DEFAULT_4X4_INTRA[..],
        2 => &DEFAULT_4X4_INTRA[..],
        3 => &DEFAULT_4X4_INTER[..],
        4 => &DEFAULT_4X4_INTER[..],
        5 => &DEFAULT_4X4_INTER[..],
        6 => &DEFAULT_8X8_INTRA[..],
        7 => &DEFAULT_8X8_INTER[..],
        8 => &DEFAULT_8X8_INTRA[..],
        9 => &DEFAULT_8X8_INTER[..],
        10 => &DEFAULT_8X8_INTRA[..],
        11 => &DEFAULT_8X8_INTER[..],
        _ => unreachable!(),
    }
}

fn rbsp_trailing_bits<W>(s: &mut Synthesizer<W>) -> SynthesizerResult<()>
where
    W: Write,
{
    s.write_f(1, 1u32)?;

    while !s.aligned() {
        s.write_f(1, 0u32)?;
    }

    Ok(())
}

pub fn synthesize_sps<W>(sps: &Sps, writer: W, ep_enabled: bool) -> SynthesizerResult<()>
where
    W: Write,
{
    let mut s = Synthesizer::<W>::new(writer, ep_enabled);

    seq_parameter_set_data(&mut s, sps)?;
    rbsp_trailing_bits(&mut s)
}

fn hrd_parameters<W>(s: &mut Synthesizer<W>, hrd_params: &HrdParams) -> SynthesizerResult<()>
where
    W: Write,
{
    s.write_ue(hrd_params.cpb_cnt_minus1)?;
    s.write_u(4, hrd_params.bit_rate_scale)?;
    s.write_u(4, hrd_params.cpb_size_scale)?;

    for i in 0..=(hrd_params.cpb_cnt_minus1 as usize) {
        s.write_ue(hrd_params.bit_rate_value_minus1[i])?;
        s.write_ue(hrd_params.cpb_size_value_minus1[i])?;
        s.write_u(1, hrd_params.cbr_flag[i])?;
    }

    s.write_u(5, hrd_params.initial_cpb_removal_delay_length_minus1)?;
    s.write_u(5, hrd_params.cpb_removal_delay_length_minus1)?;
    s.write_u(5, hrd_params.dpb_output_delay_length_minus1)?;
    s.write_u(5, hrd_params.time_offset_length)?;

    Ok(())
}

fn vui_parameters<W>(s: &mut Synthesizer<W>, nalu: &Sps) -> SynthesizerResult<()>
where
    W: Write,
{
    // H.264 E.1.1
    let vui_params = &nalu.vui_parameters;

    s.write_u(1, vui_params.aspect_ratio_info_present_flag)?;
    if vui_params.aspect_ratio_info_present_flag {
        s.write_u(8, vui_params.aspect_ratio_idc)?;
        if vui_params.aspect_ratio_idc == EXTENDED_SAR {
            s.write_u(16, vui_params.sar_width)?;
            s.write_u(16, vui_params.sar_height)?;
        }
    }

    s.write_u(1, vui_params.overscan_info_present_flag)?;
    if vui_params.overscan_info_present_flag {
        s.write_u(1, vui_params.overscan_appropriate_flag)?;
    }

    s.write_u(1, vui_params.video_signal_type_present_flag)?;
    if vui_params.video_signal_type_present_flag {
        s.write_u(3, vui_params.video_format)?;
        s.write_u(1, vui_params.video_full_range_flag)?;

        s.write_u(1, vui_params.colour_description_present_flag)?;
        if vui_params.colour_description_present_flag {
            s.write_u(8, vui_params.colour_primaries)?;
            s.write_u(8, vui_params.transfer_characteristics)?;
            s.write_u(8, vui_params.matrix_coefficients)?;
        }
    }

    s.write_u(1, vui_params.chroma_loc_info_present_flag)?;
    if vui_params.chroma_loc_info_present_flag {
        s.write_ue(vui_params.chroma_sample_loc_type_top_field)?;
        s.write_ue(nalu.vui_parameters.chroma_sample_loc_type_bottom_field)?;
    }

    s.write_u(1, vui_params.timing_info_present_flag)?;
    if vui_params.timing_info_present_flag {
        s.write_u(32, vui_params.num_units_in_tick)?;
        s.write_u(32, vui_params.time_scale)?;
        s.write_u(1, vui_params.fixed_frame_rate_flag)?;
    }

    s.write_u(1, vui_params.nal_hrd_parameters_present_flag)?;
    if vui_params.nal_hrd_parameters_present_flag {
        hrd_parameters(s, &vui_params.nal_hrd_parameters)?;
    }
    s.write_u(1, vui_params.vcl_hrd_parameters_present_flag)?;
    if vui_params.vcl_hrd_parameters_present_flag {
        hrd_parameters(s, &vui_params.vcl_hrd_parameters)?;
    }

    if vui_params.nal_hrd_parameters_present_flag || vui_params.vcl_hrd_parameters_present_flag {
        s.write_u(1, vui_params.low_delay_hrd_flag)?;
    }

    s.write_u(1, vui_params.pic_struct_present_flag)?;

    s.write_u(1, vui_params.bitstream_restriction_flag)?;
    if vui_params.bitstream_restriction_flag {
        s.write_u(1, vui_params.motion_vectors_over_pic_boundaries_flag)?;
        s.write_ue(vui_params.max_bytes_per_pic_denom)?;
        s.write_ue(vui_params.max_bits_per_mb_denom)?;
        s.write_ue(vui_params.log2_max_mv_length_horizontal)?;
        s.write_ue(vui_params.log2_max_mv_length_vertical)?;
        s.write_ue(vui_params.max_num_reorder_frames)?;
        s.write_ue(vui_params.max_dec_frame_buffering)?;
    }

    Ok(())
}

fn seq_parameter_set_data<W>(s: &mut Synthesizer<W>, nalu: &Sps) -> SynthesizerResult<()>
where
    W: Write,
{
    // H.264 7.3.2.1.1
    s.write_u(8, nalu.profile_idc)?;
    s.write_u(1, nalu.constraint_set0_flag)?;
    s.write_u(1, nalu.constraint_set1_flag)?;
    s.write_u(1, nalu.constraint_set2_flag)?;
    s.write_u(1, nalu.constraint_set3_flag)?;
    s.write_u(1, nalu.constraint_set4_flag)?;
    s.write_u(1, nalu.constraint_set5_flag)?;
    s.write_u(2, /* reserved_zero_2bits */ 0u32)?;
    s.write_u(8, nalu.level_idc as u32)?;
    s.write_ue(nalu.seq_parameter_set_id)?;

    if nalu.profile_idc == 100
        || nalu.profile_idc == 110
        || nalu.profile_idc == 122
        || nalu.profile_idc == 244
        || nalu.profile_idc == 44
        || nalu.profile_idc == 83
        || nalu.profile_idc == 86
        || nalu.profile_idc == 118
        || nalu.profile_idc == 128
        || nalu.profile_idc == 138
        || nalu.profile_idc == 139
        || nalu.profile_idc == 134
        || nalu.profile_idc == 135
    {
        s.write_ue(nalu.chroma_format_idc)?;

        if nalu.chroma_format_idc == 3 {
            s.write_u(1, nalu.separate_colour_plane_flag)?;
        }

        s.write_ue(nalu.bit_depth_luma_minus8)?;
        s.write_ue(nalu.bit_depth_chroma_minus8)?;
        s.write_u(1, nalu.qpprime_y_zero_transform_bypass_flag)?;
        s.write_u(1, nalu.seq_scaling_matrix_present_flag)?;

        if nalu.seq_scaling_matrix_present_flag {
            let scaling_list_count = if nalu.chroma_format_idc != 3 { 8 } else { 12 };

            for i in 0..scaling_list_count {
                // Assume if scaling lists are zeroed that they are not present.
                if i < 6 {
                    if nalu.scaling_lists_4x4[i] == [0; 16] {
                        s.write_u(1, /* seq_scaling_list_present_flag */ false)?;
                    } else {
                        s.write_u(1, /* seq_scaling_list_present_flag */ true)?;
                        scaling_list(s, &nalu.scaling_lists_4x4[i], default_scaling_list(i))?;
                    }
                } else if nalu.scaling_lists_8x8[i - 6] == [0; 64] {
                    s.write_u(1, /* seq_scaling_list_present_flag */ false)?;
                } else {
                    s.write_u(1, /* seq_scaling_list_present_flag */ true)?;
                    scaling_list(s, &nalu.scaling_lists_8x8[i - 6], default_scaling_list(i))?;
                }
            }
        }
    }

    s.write_ue(nalu.log2_max_frame_num_minus4)?;
    s.write_ue(nalu.pic_order_cnt_type)?;

    if nalu.pic_order_cnt_type == 0 {
        s.write_ue(nalu.log2_max_pic_order_cnt_lsb_minus4)?;
    } else if nalu.pic_order_cnt_type == 1 {
        s.write_u(1, nalu.delta_pic_order_always_zero_flag)?;
        s.write_se(nalu.offset_for_non_ref_pic)?;
        s.write_se(nalu.offset_for_top_to_bottom_field)?;
        s.write_ue(nalu.num_ref_frames_in_pic_order_cnt_cycle)?;

        for offset_for_ref_frame in &nalu.offset_for_ref_frame {
            s.write_se(*offset_for_ref_frame)?;
        }
    }

    s.write_ue(nalu.max_num_ref_frames)?;
    s.write_u(1, nalu.gaps_in_frame_num_value_allowed_flag)?;
    s.write_ue(nalu.pic_width_in_mbs_minus1)?;
    s.write_ue(nalu.pic_height_in_map_units_minus1)?;
    s.write_u(1, nalu.frame_mbs_only_flag)?;
    if !nalu.frame_mbs_only_flag {
        s.write_u(1, nalu.mb_adaptive_frame_field_flag)?;
    }
    s.write_u(1, nalu.direct_8x8_inference_flag)?;

    s.write_u(1, nalu.frame_cropping_flag)?;
    if nalu.frame_cropping_flag {
        s.write_ue(nalu.frame_crop_left_offset)?;
        s.write_ue(nalu.frame_crop_right_offset)?;
        s.write_ue(nalu.frame_crop_top_offset)?;
        s.write_ue(nalu.frame_crop_bottom_offset)?;
    }

    s.write_u(1, nalu.vui_parameters_present_flag)?;
    if nalu.vui_parameters_present_flag {
        vui_parameters(s, nalu)?;
    }

    Ok(())
}
