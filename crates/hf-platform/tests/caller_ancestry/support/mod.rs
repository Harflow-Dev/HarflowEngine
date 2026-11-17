use std::{
    collections::VecDeque,
    io,
    time::{Duration, Instant},
};

use hf_platform::task_runtime::caller::{CallerEvidenceSource, CallerProcessRecord};

#[derive(Clone, Copy, Debug)]
pub struct Record {
    pub pid: u32,
    pub parent: u32,
    pub uid: u32,
    pub status: u32,
    pub seconds: u64,
}

impl Record {
    pub fn process(self) -> CallerProcessRecord {
        CallerProcessRecord {
            pid: self.pid,
            parent: self.parent,
            uid: self.uid,
            status: self.status,
            start_seconds: self.seconds,
            start_microseconds: 12,
        }
    }
}

pub struct Script {
    pub records: VecDeque<Result<CallerProcessRecord, ()>>,
    pub boots: VecDeque<Result<String, ()>>,
    pub clock: VecDeque<Instant>,
    pub reads: Vec<String>,
    pub fallback_time: Instant,
}

impl Script {
    pub fn new(records: Vec<CallerProcessRecord>, boots: Vec<String>) -> Self {
        Self {
            records: records.into_iter().map(Ok).collect(),
            boots: boots.into_iter().map(Ok).collect(),
            clock: VecDeque::new(),
            reads: Vec::new(),
            fallback_time: Instant::now(),
        }
    }

    pub fn fail_record(&mut self, index: usize) {
        self.records[index] = Err(());
    }

    pub fn fail_boot(&mut self, index: usize) {
        self.boots[index] = Err(());
    }

    pub fn clock_at(&mut self, samples: impl IntoIterator<Item = Instant>) {
        self.clock = samples.into_iter().collect();
    }
}

impl CallerEvidenceSource for Script {
    fn now(&mut self) -> Instant {
        self.clock.pop_front().unwrap_or(self.fallback_time)
    }

    fn boot_uuid(&mut self) -> io::Result<String> {
        self.reads.push("boot".to_owned());
        self.boots
            .pop_front()
            .unwrap_or(Err(()))
            .map_err(|_| io::Error::other("scripted boot failure"))
    }

    fn record(&mut self, pid: u32) -> io::Result<CallerProcessRecord> {
        self.reads.push(format!("record:{pid}"));
        self.records
            .pop_front()
            .unwrap_or(Err(()))
            .map_err(|_| io::Error::other("scripted record failure"))
    }

    fn real_uid(&mut self) -> u32 {
        392
    }
}

pub fn deadline() -> Instant {
    Instant::now() + Duration::from_secs(60)
}

pub fn chain(length: usize) -> Vec<Record> {
    (0..length)
        .map(|index| Record {
            pid: 1000 + index as u32,
            parent: if index + 1 == length {
                1
            } else {
                1001 + index as u32
            },
            uid: if index == 0 { 392 } else { 0 },
            status: 2,
            seconds: 10_000 + index as u64,
        })
        .collect()
}

pub fn observations(records: &[Record], init: Record) -> Vec<CallerProcessRecord> {
    let mut result = vec![init.process()];
    result.extend(records.iter().map(|record| record.process()));
    // Every caller link is observed again before the final init anchor.
    result.extend(records.iter().map(|record| record.process()));
    result.push(init.process());
    result
}

pub fn repeated_boots(count: usize) -> Vec<String> {
    std::iter::repeat_n("AbCdEf01-2345-6789-AbCd-Ef0123456789".to_owned(), count).collect()
}

pub fn repeated_records(records: &[Record], init: Record) -> Vec<CallerProcessRecord> {
    observations(records, init)
}
