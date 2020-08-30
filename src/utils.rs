// Copyright 2020 TiKV Project Authors. Licensed under Apache-2.0.

#[inline]
pub fn real_time_ns() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::SystemTime::UNIX_EPOCH)
        .expect("SystemTime before UNIX EPOCH!")
        .as_nanos() as u64
}

#[inline]
pub fn cycles_to_ns(cycles: u64) -> u64 {
    (cycles as u128 * 1_000_000_000 / minstant::cycles_per_second() as u128) as u64
}
