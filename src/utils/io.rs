use bytes::Bytes;
use std::{collections::VecDeque, ops::Range, time::Duration};
use tracing::trace;
use webrtc::{
    media::Sample,
    rtp::{header::Header, packet::Packet, packetizer::Depacketizer},
};

#[derive(Debug)]
struct Entry {
    header: Header,
    payload: Bytes,
    head: bool,
    tail: bool,
}

#[derive(Debug)]
#[allow(clippy::type_complexity)]
pub struct SampleBuilder<T: Depacketizer> {
    hold_back: usize,
    depack: T,
    queue: VecDeque<Entry>,
    segments: Vec<(usize, usize)>,
    last_emitted: Option<u16>,
    depack_cache: Option<(Range<usize>, (u32, Vec<u8>))>,
    ready: Option<(u32, Vec<u8>)>,
    sample_rate: u32,
    samples: u32,
}

impl<T: Depacketizer> SampleBuilder<T> {
    pub fn new(depack: T, hold_back: usize, sample_rate: u32) -> Self {
        Self {
            hold_back,
            depack,
            queue: VecDeque::new(),
            segments: Vec::new(),
            last_emitted: None,
            depack_cache: None,
            ready: None,
            sample_rate,
            samples: 0,
        }
    }

    pub fn push(&mut self, p: Packet) -> bool {
        if let Some(last) = self.last_emitted
            && p.header.sequence_number <= last
            && self.hold_back > 0
        {
            trace!(
                "Drop before emitted: {} <= {}",
                p.header.sequence_number, last
            );
            return false;
        }

        match self
            .queue
            .binary_search_by_key(&p.header.sequence_number, |r| r.header.sequence_number)
        {
            Ok(_) => {
                trace!("Drop exactly same packet: {}", p.header.sequence_number);
            }
            Err(i) => {
                let head = self.depack.is_partition_head(&p.payload);
                let tail = self.depack.is_partition_tail(p.header.marker, &p.payload);

                let entry = Entry {
                    header: p.header,
                    payload: p.payload,
                    head,
                    tail,
                };
                self.queue.insert(i, entry);
            }
        };

        true
    }

    pub fn pop(&mut self) -> Option<Sample> {
        self.update_segments();

        let (start, stop) = *self.segments.first()?;

        let seq = {
            let last = self.queue.get(stop).expect("entry for stop index");
            last.header.sequence_number
        };

        let dep = match self.depacketize(start, stop, seq) {
            Ok(d) => d,
            Err(_) => {
                self.last_emitted = Some(seq);
                self.queue.drain(0..=stop);
                return None;
            }
        };

        let more_than_hold_back = self.segments.len() >= self.hold_back;
        let contiguous_seq = self.is_following_last(start);
        let wait_for_contiguity = !contiguous_seq && !more_than_hold_back;

        if wait_for_contiguity {
            self.depack_cache = Some((start..stop, dep));
            return None;
        }

        let last = self
            .queue
            .get(stop)
            .expect("entry for stop index")
            .header
            .sequence_number;

        self.queue.drain(0..=stop);

        self.last_emitted = Some(last);

        let after_timestamp = dep.0;
        let ready = self.ready.take();
        self.ready = Some(dep);

        ready.map(|(sample_timestamp, data)| {
            let samples = after_timestamp.saturating_sub(sample_timestamp);
            if samples > 0 {
                self.samples = samples;
            }
            Sample {
                data: Bytes::copy_from_slice(&data),
                duration: Duration::from_secs_f64(
                    (self.samples as f64) / (self.sample_rate as f64),
                ),
                ..Default::default()
            }
        })
    }

    fn depacketize(
        &mut self,
        start: usize,
        stop: usize,
        _seq: u16,
    ) -> Result<(u32, Vec<u8>), webrtc::rtp::Error> {
        if let Some(cached) = self.depack_cache.take()
            && cached.0 == (start..stop)
        {
            trace!("depack cache hit for segment start {}", start);
            return Ok(cached.1);
        }

        let timestamp = self
            .queue
            .get(start)
            .expect("entry for stop index")
            .header
            .timestamp;

        let mut data: Vec<u8> = Vec::new();

        for entry in self.queue.range_mut(start..=stop) {
            let p = self.depack.depacketize(&entry.payload)?;
            data.extend_from_slice(&p);
        }

        Ok((timestamp, data))
    }

    fn update_segments(&mut self) -> Option<(usize, usize)> {
        self.segments.clear();

        #[derive(Clone, Copy)]
        struct Start {
            index: i64,
            time: u32,
            offset: i64,
        }

        let mut start: Option<Start> = None;

        for (index, entry) in self.queue.iter().enumerate() {
            let index = index as i64;
            let iseq = entry.header.sequence_number as i64;
            let expected_seq = start.map(|s| s.offset.saturating_add(index));

            let is_expected_seq = expected_seq == Some(iseq);
            let is_same_timestamp = start.map(|s| s.time) == Some(entry.header.timestamp);
            let is_defacto_tail = is_expected_seq && !is_same_timestamp;

            if start.is_some() && is_defacto_tail {
                let segment = (start.unwrap().index as usize, index as usize - 1);
                self.segments.push(segment);
                start = None;
            }

            if start.is_some() && (!is_expected_seq || !is_same_timestamp) {
                start = None;
            }

            if start.is_none() && entry.head {
                start = Some(Start {
                    index,
                    time: entry.header.timestamp,
                    offset: iseq.saturating_sub(index),
                });
            }

            if start.is_some() && entry.tail {
                let segment = (start.unwrap().index as usize, index as usize);
                self.segments.push(segment);
                start = None;
            }
        }

        None
    }

    fn is_following_last(&self, start: usize) -> bool {
        let Some(last) = self.last_emitted else {
            return true;
        };

        let mut seq = last;

        for entry in self.queue.range(0..start) {
            let is_next =
                seq < entry.header.sequence_number && entry.header.sequence_number - seq == 1;
            if !is_next {
                return false;
            }
            seq = entry.header.sequence_number;

            let is_padding = entry.payload.is_empty() && !entry.head && !entry.tail;
            if !is_padding {
                return false;
            }
        }

        let start_entry = self.queue.get(start).expect("entry for start index");

        seq < start_entry.header.sequence_number && start_entry.header.sequence_number - seq == 1
    }
}
