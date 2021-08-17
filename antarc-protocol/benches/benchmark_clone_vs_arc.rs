use std::sync::Arc;

use criterion::{criterion_group, criterion_main, Criterion};
use rand::Rng;

const BIG_RANGE: core::ops::Range<usize> = 0..(1500 * 10);
const MEDIUM_RANGE: core::ops::Range<usize> = 0..(1500 * 3);
const SMALL_RANGE: core::ops::Range<usize> = 0..(1500 * 1);

const FEW_PEERS: core::ops::Range<usize> = 0..10;
const MANY_PEERS: core::ops::Range<usize> = 0..100;
const BUNCH_PEERS: core::ops::Range<usize> = 0..1000;

/// NOTE(alex): Arc is definitely the way to go, calling clone multiple times on a `Vec` around the
/// Payload size will always be slower than using Arc.

fn bench_clone_vs_arc(c: &mut Criterion) {
    let big_data_vec: Vec<u8> = BIG_RANGE.into_iter().map(|val| val as u8).collect();
    let medium_data_vec: Vec<u8> = MEDIUM_RANGE.into_iter().map(|val| val as u8).collect();
    let small_data_vec: Vec<u8> = SMALL_RANGE.into_iter().map(|val| val as u8).collect();

    let big_data_arc: Arc<Vec<u8>> = Arc::new(big_data_vec.clone());
    let medium_data_arc: Arc<Vec<u8>> = Arc::new(medium_data_vec.clone());
    let small_data_arc: Arc<Vec<u8>> = Arc::new(small_data_vec.clone());

    let mut group = c.benchmark_group("Vec::clone vs Arc::clone");

    group.bench_function("Vec / Small / Few peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in FEW_PEERS.into_iter() {
                let cloned = small_data_vec.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Vec / Small / Many peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in MANY_PEERS.into_iter() {
                let cloned = small_data_vec.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Vec / Small / Bunch peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in BUNCH_PEERS.into_iter() {
                let cloned = small_data_vec.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Arc / Small / Few peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in FEW_PEERS.into_iter() {
                let cloned = small_data_arc.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Arc / Small / Many peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in MANY_PEERS.into_iter() {
                let cloned = small_data_arc.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Arc / Small / Bunch peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in BUNCH_PEERS.into_iter() {
                let cloned = small_data_arc.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Vec / Medium / Few peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in FEW_PEERS.into_iter() {
                let cloned = medium_data_vec.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Vec / Medium / Many peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in MANY_PEERS.into_iter() {
                let cloned = medium_data_vec.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }

            val
        })
    });

    group.bench_function("Vec / Medium / Bunch peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in BUNCH_PEERS.into_iter() {
                let cloned = medium_data_vec.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }
            val
        })
    });

    group.bench_function("Arc / Medium / Few peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in FEW_PEERS.into_iter() {
                let cloned = medium_data_arc.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }
            val
        })
    });

    group.bench_function("Arc / Medium / Many peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in MANY_PEERS.into_iter() {
                let cloned = medium_data_arc.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }
            val
        })
    });

    group.bench_function("Arc / Medium / Bunch peers / clone", |b| {
        b.iter(|| {
            let mut rng = rand::thread_rng();
            let mut val = 0;
            for _ in BUNCH_PEERS.into_iter() {
                let cloned = medium_data_arc.clone();

                if cloned.len() % 2 == 0 {
                    val = rng.gen_range(0..cloned.len());
                }
            }
            val
        })
    });

    group.finish();
}

criterion_group!(benches, bench_clone_vs_arc);
criterion_main!(benches);
