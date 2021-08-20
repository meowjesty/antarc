use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use rand::Rng;

const BIG_RANGE: core::ops::Range<usize> = 0..(1500 * 10);
const MEDIUM_RANGE: core::ops::Range<usize> = 0..(1500 * 3);
const SMALL_RANGE: core::ops::Range<usize> = 0..(1500 * 1);

fn bench_array_concat(c: &mut Criterion) {
    let mut group = c.benchmark_group("Concat lists");

    // Bronze
    let medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    let medium_data_arc: Arc<Vec<u8>> = Arc::new(medium_data_vec.clone());
    let medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    group.bench_with_input(
        " medium_data / vec![slice.to_vec(), arc.to_vec()].concat()",
        medium_data_vec.as_slice(),
        |b, slice| {
            b.iter(|| {
                vec![slice.to_vec(), medium_data_arc.to_vec()].concat();
            })
        },
    );

    let big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    let big_data_arc: Arc<Vec<u8>> = Arc::new(big_data_vec.clone());
    let big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    group.bench_with_input(
        " big_data / vec![slice.to_vec(), arc.to_vec()].concat()",
        big_data_vec.as_slice(),
        |b, slice| {
            b.iter(|| {
                vec![slice.to_vec(), big_data_arc.to_vec()].concat();
            })
        },
    );

    // Unusably slow
    // let medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    // let medium_data_arc: Arc<Vec<u8>> = Arc::new(medium_data_vec.clone());
    // let mut medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    // group.bench_function(" medium_data / vector.extend_from_slice(&arc)", |b| {
    //     b.iter(|| {
    //         medium_data_vec.extend_from_slice(&medium_data_arc);
    //     })
    // });

    // let big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    // let big_data_arc: Arc<Vec<u8>> = Arc::new(big_data_vec.clone());
    // let mut big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    // group.bench_function(" big_data / vector.extend_from_slice(&arc)", |b| {
    //     b.iter(|| {
    //         big_data_vec.extend_from_slice(&big_data_arc);
    //     })
    // });

    // Silver
    let medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    let medium_data_arc: Arc<Vec<u8>> = Arc::new(medium_data_vec.clone());
    let medium_data_slice: &[u8] = &MEDIUM_RANGE
        .into_iter()
        .map(|val| val as u8)
        .collect::<Vec<u8>>();
    group.bench_function(
        " medium_data / vec![slice_a, arc.as_slice()].concat()",
        |b| {
            b.iter(|| {
                vec![medium_data_slice, medium_data_arc.as_slice()].concat();
            })
        },
    );

    let big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    let big_data_arc: Arc<Vec<u8>> = Arc::new(big_data_vec.clone());
    let big_data_slice: &[u8] = &BIG_RANGE
        .into_iter()
        .map(|val| val as u8)
        .collect::<Vec<_>>();
    group.bench_function(" big_data / vec![slice_a, arc.as_slice()].concat()", |b| {
        b.iter(|| {
            vec![big_data_slice, big_data_arc.as_slice()].concat();
        })
    });

    // Gold
    let medium_data_array: [u8; 1500 * 3] = [1; 1500 * 3];
    let medium_data_arc: Arc<Vec<u8>> = Arc::new(medium_data_vec.clone());
    group.bench_function(
        " medium_data / [array.as_ref(), arc.as_slice()].concat()",
        |b| {
            b.iter(|| {
                let _vec = [medium_data_array.as_ref(), medium_data_arc.as_slice()].concat();
            })
        },
    );

    let big_data_array: [u8; 1500 * 10] = [1; 1500 * 10];
    let big_data_arc: Arc<Vec<u8>> = Arc::new(big_data_vec.clone());
    group.bench_function(
        " big_data / [array.as_ref(), arc.as_slice()].concat()",
        |b| {
            b.iter(|| {
                let _vec = [big_data_array.as_ref(), big_data_arc.as_slice()].concat();
            })
        },
    );

    group.finish();
}

criterion_group!(benches, bench_array_concat);
criterion_main!(benches);
