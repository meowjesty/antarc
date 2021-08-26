use crc32fast::*;
use criterion::{criterion_group, criterion_main, Criterion};

const BIG_RANGE: core::ops::Range<usize> = 0..(1500 * 10);
const MEDIUM_RANGE: core::ops::Range<usize> = 0..(1500 * 3);
const SMALL_RANGE: core::ops::Range<usize> = 0..(1500 * 1);
const TINY_RANGE: core::ops::Range<usize> = 0..(1024);

/// NOTE(alex): I've tried to benchmark with rayon, but it only made the performance way worse, this
/// is probably related to how crc32fast uses SIMD.
///
/// The difference in performance between 1024 bytes, versus 1500 bytes is kinda big (around 2x).
/// It gets even if the 500 (or so) bytes are 0, pushing forward the idea that calculating the crc32
/// only on a part of the packet makes sense (performance-wise).
///
/// Not every packet has the same size though, so the smaller ones must be padded to some minimum
/// size (with 0). This has to happen both on encode and decode.
///
/// But in the end, I remain unconvinced, hashing the whole packet makes more sense in a protocol,
/// as an error detecting tool.
fn bench_hash_packet_vs_part(c: &mut Criterion) {
    let big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    let medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    let small_data_vec: Vec<u8> = SMALL_RANGE.into_iter().map(|val| val as u8).collect();
    let tiny_data_vec: Vec<u8> = TINY_RANGE.into_iter().map(|val| val as u8).collect();
    let small_zeroed_data_vec: Vec<u8> = TINY_RANGE
        .into_iter()
        .enumerate()
        .map(|(index, val)| if index > 1024 { 0u8 } else { val as u8 })
        .collect();

    let mut group = c.benchmark_group("Full data hash vs smaller hash");

    let hasher = Hasher::new();
    group.bench_function("Tiny data hashing / complete", |b| {
        b.iter(|| {
            let mut hasher = hasher.clone();
            hasher.update(&tiny_data_vec);
            let hash = hasher.finalize();
            hash
        })
    });

    let hasher = Hasher::new();
    group.bench_function("Small data hashing / complete", |b| {
        b.iter(|| {
            let mut hasher = hasher.clone();
            hasher.update(&small_data_vec);
            let hash = hasher.finalize();
            hash
        })
    });

    let hasher = Hasher::new();
    group.bench_function(
        "Small data hashing with zeroes after 1024 / complete",
        |b| {
            b.iter(|| {
                let mut hasher = hasher.clone();
                hasher.update(&small_zeroed_data_vec);
                let hash = hasher.finalize();
                hash
            })
        },
    );

    let hasher = Hasher::new();
    group.bench_function("Medium data hashing / complete", |b| {
        b.iter(|| {
            let mut hasher = hasher.clone();
            hasher.update(&medium_data_vec);
            let hash = hasher.finalize();
            hash
        })
    });

    let hasher = Hasher::new();
    group.bench_function("Big data hashing / complete", |b| {
        b.iter(|| {
            let mut hasher = hasher.clone();
            hasher.update(&big_data_vec);
            let hash = hasher.finalize();
            hash
        })
    });

    group.finish();
}

criterion_group!(benches, bench_hash_packet_vs_part);
criterion_main!(benches);
