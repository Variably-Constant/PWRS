//! Criterion benches for `pwrs::simd::checksums`: CRC-32 through the
//! widest body this machine runs against its own table, and CRC-32C's
//! instruction beside them, over one buffer per size. Every arm reads the
//! same bytes, so their ratio is the kernel's.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use pwrs::simd::checksums::{crc32, crc32_append_scalar, crc32c, crc32c_append_scalar};
use std::hint::black_box;

fn buffer(len: usize) -> Vec<u8> {
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    (0..len)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn benches(c: &mut Criterion) {
    let mut group = c.benchmark_group("crc");
    for len in [64usize, 1024, 65536] {
        let data = buffer(len);
        group.throughput(Throughput::Bytes(len as u64));
        group.bench_with_input(BenchmarkId::new("crc32_folded", len), &data, |b, d| b.iter(|| black_box(crc32(black_box(d)))));
        group.bench_with_input(BenchmarkId::new("crc32_table", len), &data, |b, d| b.iter(|| black_box(crc32_append_scalar(0, black_box(d)))));
        group.bench_with_input(BenchmarkId::new("crc32c_instruction", len), &data, |b, d| b.iter(|| black_box(crc32c(black_box(d)))));
        group.bench_with_input(BenchmarkId::new("crc32c_table", len), &data, |b, d| b.iter(|| black_box(crc32c_append_scalar(0, black_box(d)))));
    }
    group.finish();
}

criterion_group!(checksums, benches);
criterion_main!(checksums);
