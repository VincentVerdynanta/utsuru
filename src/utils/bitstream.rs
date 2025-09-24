use std::{
    fmt,
    io::{Cursor, Read, Seek, SeekFrom, Write},
};
use tracing::trace;

/// A bit reader for codec bitstreams. It properly handles emulation-prevention
/// bytes and stop bits for H264.
#[derive(Clone)]
pub struct BitReader<'a> {
    /// A reference into the next unread byte in the stream.
    data: Cursor<&'a [u8]>,
    /// Contents of the current byte. First unread bit starting at position 8 -
    /// num_remaining_bits_in_curr_bytes.
    curr_byte: u8,
    /// Number of bits remaining in `curr_byte`
    num_remaining_bits_in_curr_byte: usize,
    /// Used in emulation prevention byte detection.
    prev_two_bytes: u16,
    /// Number of emulation prevention bytes (i.e. 0x000003) we found.
    num_epb: usize,
    /// Whether or not we need emulation prevention logic.
    needs_epb: bool,
    /// How many bits have been read so far.
    position: u64,
}

#[derive(Debug)]
pub enum GetByteError {
    OutOfBits,
}

impl fmt::Display for GetByteError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "reader ran out of bits")
    }
}

#[derive(Debug)]
pub enum ReadBitsError {
    TooManyBitsRequested(usize),
    GetByte(GetByteError),
    ConversionFailed,
}

impl fmt::Display for ReadBitsError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ReadBitsError::TooManyBitsRequested(bits) => {
                write!(f, "more than 31 ({}) bits were requested", bits)
            }
            ReadBitsError::GetByte(_) => write!(f, "failed to advance the current byte"),
            ReadBitsError::ConversionFailed => {
                write!(f, "failed to convert read input to target type")
            }
        }
    }
}

impl From<GetByteError> for ReadBitsError {
    fn from(err: GetByteError) -> Self {
        ReadBitsError::GetByte(err)
    }
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8], needs_epb: bool) -> Self {
        Self {
            data: Cursor::new(data),
            curr_byte: Default::default(),
            num_remaining_bits_in_curr_byte: Default::default(),
            prev_two_bytes: 0xffff,
            num_epb: Default::default(),
            needs_epb,
            position: 0,
        }
    }

    /// Read a single bit from the stream.
    pub fn read_bit(&mut self) -> Result<bool, String> {
        let bit = self.read_bits::<u32>(1)?;
        match bit {
            1 => Ok(true),
            0 => Ok(false),
            _ => panic!("Unexpected value {}", bit),
        }
    }

    /// Read up to 31 bits from the stream. Note that we don't want to read 32
    /// bits even though we're returning a u32 because that would break the
    /// read_bits_signed() function. 31 bits should be overkill for compressed
    /// header parsing anyway.
    pub fn read_bits<U: TryFrom<u32>>(&mut self, num_bits: usize) -> Result<U, String> {
        if num_bits > 31 {
            return Err(ReadBitsError::TooManyBitsRequested(num_bits).to_string());
        }

        let mut bits_left = num_bits;
        let mut out = 0u32;

        while self.num_remaining_bits_in_curr_byte < bits_left {
            out |= (self.curr_byte as u32) << (bits_left - self.num_remaining_bits_in_curr_byte);
            bits_left -= self.num_remaining_bits_in_curr_byte;
            self.move_to_next_byte().map_err(|err| err.to_string())?;
        }

        out |= (self.curr_byte >> (self.num_remaining_bits_in_curr_byte - bits_left)) as u32;
        out &= (1 << num_bits) - 1;
        self.num_remaining_bits_in_curr_byte -= bits_left;
        self.position += num_bits as u64;

        U::try_from(out).map_err(|_| ReadBitsError::ConversionFailed.to_string())
    }

    /// Reads a two's complement signed integer of length |num_bits|.
    pub fn read_bits_signed<U: TryFrom<i32>>(&mut self, num_bits: usize) -> Result<U, String> {
        let mut out: i32 = self
            .read_bits::<u32>(num_bits)?
            .try_into()
            .map_err(|_| ReadBitsError::ConversionFailed.to_string())?;
        if out >> (num_bits - 1) != 0 {
            out |= -1i32 ^ ((1 << num_bits) - 1);
        }

        U::try_from(out).map_err(|_| ReadBitsError::ConversionFailed.to_string())
    }

    /// Reads an unsigned integer from the stream and checks if the stream is byte aligned.
    pub fn read_bits_aligned<U: TryFrom<u32>>(&mut self, num_bits: usize) -> Result<U, String> {
        if !self.num_remaining_bits_in_curr_byte.is_multiple_of(8) {
            return Err("Attempted unaligned read_le()".into());
        }

        self.read_bits(num_bits).map_err(|err| err.to_string())
    }

    /// Skip `num_bits` bits from the stream.
    pub fn skip_bits(&mut self, mut num_bits: usize) -> Result<(), String> {
        while num_bits > 0 {
            let n = std::cmp::min(num_bits, 31);
            self.read_bits::<u32>(n)?;
            num_bits -= n;
        }

        Ok(())
    }

    /// Returns the amount of bits left in the stream
    pub fn num_bits_left(&mut self) -> usize {
        let cur_pos = self.data.position();
        // This should always be safe to unwrap.
        let end_pos = self.data.seek(SeekFrom::End(0)).unwrap();
        let _ = self.data.seek(SeekFrom::Start(cur_pos));
        ((end_pos - cur_pos) as usize) * 8 + self.num_remaining_bits_in_curr_byte
    }

    /// Returns the number of emulation-prevention bytes read so far.
    pub fn num_epb(&self) -> usize {
        self.num_epb
    }

    /// Whether the stream still has RBSP data. Implements more_rbsp_data(). See
    /// the spec for more details.
    pub fn has_more_rsbp_data(&mut self) -> bool {
        if self.num_remaining_bits_in_curr_byte == 0 && self.move_to_next_byte().is_err() {
            // no more data at all in the rbsp
            return false;
        }

        // If the next bit is the stop bit, then we should only see unset bits
        // until the end of the data.
        if (self.curr_byte & ((1 << (self.num_remaining_bits_in_curr_byte - 1)) - 1)) != 0 {
            return true;
        }

        let mut buf = [0u8; 1];
        let orig_pos = self.data.position();
        while self.data.read_exact(&mut buf).is_ok() {
            if buf[0] != 0 {
                self.data.set_position(orig_pos);
                return true;
            }
        }
        false
    }

    /// Reads an Unsigned Exponential golomb coding number from the next bytes in the
    /// bitstream. This may advance the state of position within the bitstream even if the
    /// read operation is unsuccessful. See H264 Annex B specification 9.1 for details.
    pub fn read_ue<U: TryFrom<u32>>(&mut self) -> Result<U, String> {
        let mut num_bits = 0;

        while self.read_bits::<u32>(1)? == 0 {
            num_bits += 1;
            if num_bits > 31 {
                return Err("invalid stream".into());
            }
        }

        let value = ((1u32 << num_bits) - 1)
            .checked_add(self.read_bits::<u32>(num_bits)?)
            .ok_or::<String>("read number cannot fit in 32 bits".into())?;

        U::try_from(value).map_err(|_| "conversion error".into())
    }

    pub fn read_ue_bounded<U: TryFrom<u32>>(&mut self, min: u32, max: u32) -> Result<U, String> {
        let ue = self.read_ue()?;
        if ue > max || ue < min {
            Err(format!(
                "Value out of bounds: expected {} - {}, got {}",
                min, max, ue
            ))
        } else {
            Ok(U::try_from(ue).map_err(|_| String::from("Conversion error"))?)
        }
    }

    pub fn read_ue_max<U: TryFrom<u32>>(&mut self, max: u32) -> Result<U, String> {
        self.read_ue_bounded(0, max)
    }

    /// Reads a signed exponential golomb coding number. Instead of using two's
    /// complement, this scheme maps even integers to positive numbers and odd
    /// integers to negative numbers. The least significant bit indicates the
    /// sign. See H264 Annex B specification 9.1.1 for details.
    pub fn read_se<U: TryFrom<i32>>(&mut self) -> Result<U, String> {
        let ue = self.read_ue::<u32>()? as i32;

        if ue % 2 == 0 {
            Ok(U::try_from(-(ue / 2)).map_err(|_| String::from("Conversion error"))?)
        } else {
            Ok(U::try_from(ue / 2 + 1).map_err(|_| String::from("Conversion error"))?)
        }
    }

    pub fn read_se_bounded<U: TryFrom<i32>>(&mut self, min: i32, max: i32) -> Result<U, String> {
        let se = self.read_se()?;
        if se < min || se > max {
            Err(format!(
                "Value out of bounds, expected between {}-{}, got {}",
                min, max, se
            ))
        } else {
            Ok(U::try_from(se).map_err(|_| String::from("Conversion error"))?)
        }
    }

    /// Read little endian multi-byte integer.
    pub fn read_le<U: TryFrom<u32>>(&mut self, num_bits: u8) -> Result<U, String> {
        let mut t = 0;

        for i in 0..num_bits {
            let byte = self.read_bits_aligned::<u32>(8)?;
            t += byte << (i * 8)
        }

        U::try_from(t).map_err(|_| String::from("Conversion error"))
    }

    /// Return the position of this bitstream in bits.
    pub fn position(&self) -> u64 {
        self.position
    }

    fn get_byte(&mut self) -> Result<u8, GetByteError> {
        let mut buf = [0u8; 1];
        self.data
            .read_exact(&mut buf)
            .map_err(|_| GetByteError::OutOfBits)?;
        Ok(buf[0])
    }

    fn move_to_next_byte(&mut self) -> Result<(), GetByteError> {
        let mut byte = self.get_byte()?;

        if self.needs_epb {
            if self.prev_two_bytes == 0 && byte == 0x03 {
                // We found an epb
                self.num_epb += 1;
                // Read another byte
                byte = self.get_byte()?;
                // We need another 3 bytes before another epb can happen.
                self.prev_two_bytes = 0xffff;
            }
            self.prev_two_bytes = (self.prev_two_bytes << 8) | u16::from(byte);
        }

        self.num_remaining_bits_in_curr_byte = 8;
        self.curr_byte = byte;
        Ok(())
    }

    pub(crate) fn get_stream(&self) -> &Cursor<&[u8]> {
        &self.data
    }
}

#[derive(Debug)]
pub enum BitWriterError {
    InvalidBitCount,
    Io(std::io::Error),
}

impl fmt::Display for BitWriterError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            BitWriterError::InvalidBitCount => write!(f, "invalid bit count"),
            BitWriterError::Io(x) => write!(f, "{}", x),
        }
    }
}

impl From<std::io::Error> for BitWriterError {
    fn from(err: std::io::Error) -> Self {
        BitWriterError::Io(err)
    }
}

pub type BitWriterResult<T> = std::result::Result<T, BitWriterError>;

pub struct BitWriter<W: Write> {
    out: W,
    nth_bit: u8,
    curr_byte: u8,
}

impl<W: Write> BitWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            out: writer,
            curr_byte: 0,
            nth_bit: 0,
        }
    }

    /// Writes fixed bit size integer (up to 32 bit)
    pub fn write_f<T: Into<u32>>(&mut self, bits: usize, value: T) -> BitWriterResult<usize> {
        let value = value.into();

        if bits > 32 {
            return Err(BitWriterError::InvalidBitCount);
        }

        let mut written = 0;
        for bit in (0..bits).rev() {
            let bit = (1 << bit) as u32;

            self.write_bit((value & bit) == bit)?;
            written += 1;
        }

        Ok(written)
    }

    /// Takes a single bit that will be outputed to [`std::io::Write`]
    pub fn write_bit(&mut self, bit: bool) -> BitWriterResult<()> {
        self.curr_byte |= (bit as u8) << (7u8 - self.nth_bit);
        self.nth_bit += 1;

        if self.nth_bit == 8 {
            self.out.write_all(&[self.curr_byte])?;
            self.nth_bit = 0;
            self.curr_byte = 0;
        }

        Ok(())
    }

    /// Immediately outputs any cached bits to [`std::io::Write`]
    pub fn flush(&mut self) -> BitWriterResult<()> {
        if self.nth_bit != 0 {
            self.out.write_all(&[self.curr_byte])?;
            self.nth_bit = 0;
            self.curr_byte = 0;
        }

        self.out.flush()?;
        Ok(())
    }

    /// Returns `true` if ['Self`] hold data that wasn't written to [`std::io::Write`]
    pub fn has_data_pending(&self) -> bool {
        self.nth_bit != 0
    }

    pub(crate) fn inner(&self) -> &W {
        &self.out
    }

    pub(crate) fn inner_mut(&mut self) -> &mut W {
        &mut self.out
    }
}

impl<W: Write> Drop for BitWriter<W> {
    fn drop(&mut self) {
        if let Err(e) = self.flush() {
            trace!("Unable to flush bits {e:?}");
        }
    }
}
