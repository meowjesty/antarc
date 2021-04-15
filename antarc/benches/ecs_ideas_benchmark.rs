use criterion::{black_box, criterion_group, criterion_main, Criterion};
use hecs::{Entity, World};

#[derive(Debug)]
struct Data {
    x: f32,
    y: f32,
    z: f32,
    day: u32,
    month: u16,
    year: u32,
}

#[derive(Debug)]
struct Marker;

#[derive(Debug)]
struct Extra(u32);

#[derive(Debug)]
struct Event {
    id: Entity,
}

const MAX: usize = 1000;

fn build_data() -> Vec<Data> {
    let mut data_list = Vec::with_capacity(MAX);
    for i in 0..MAX {
        data_list.push(Data {
            x: 1.0 + i as f32,
            y: 1.1 + i as f32,
            z: 1.2 + i as f32,
            day: 2 + i as u32,
            month: 3 + i as u16,
            year: 4 + i as u32,
        });
    }

    data_list
}

fn prepare_ecs() -> World {
    let mut world = World::new();
    let data_list = build_data();

    let spawned_data = world
        .spawn_batch(data_list.into_iter().map(|data| (data,)))
        .collect::<Vec<_>>();
    assert!(spawned_data.len() == MAX);

    world.flush();

    let spawned_events = world
        .spawn_batch(spawned_data.into_iter().map(|id| (Event { id },)))
        .collect::<Vec<_>>();
    assert!(spawned_events.len() == MAX);

    world.flush();
    world
}

fn insert() {
    let mut world = prepare_ecs();
    let mut to_change: Vec<Entity> = Vec::with_capacity(MAX / 2);

    let mut entered = 0;

    for (id, (data,)) in world.query::<(&Data,)>().iter() {
        to_change.push(id);
        entered += 1;
    }

    assert_eq!(entered, MAX);

    while let Some(id) = to_change.pop() {
        let _ = world.insert(id, (Marker, Extra(0xed))).unwrap();
    }
}

fn spawn_despawn() {
    let mut world = prepare_ecs();
    let mut to_change: Vec<Entity> = Vec::with_capacity(MAX / 2);
    let mut to_despawn: Vec<Entity> = Vec::with_capacity(MAX / 2);

    let mut entered = 0;

    for (id, (event,)) in world.query::<(&Event,)>().iter() {
        to_change.push(event.id);
        to_despawn.push(id);
        entered += 1;
    }

    assert_eq!(entered, MAX);

    while let Some(id) = to_change.pop() {
        let _ = world.insert(id, (Marker, Extra(0xed))).unwrap();
    }

    while let Some(id) = to_despawn.pop() {
        let _ = world.despawn(id).unwrap();
    }
}

fn bench_insert(c: &mut Criterion) {
    c.bench_function("insert", |b| b.iter(|| insert()));
}

fn bench_spawn_despawn(c: &mut Criterion) {
    c.bench_function("spawn_despawn", |b| b.iter(|| spawn_despawn()));
}

criterion_group!(benches, bench_spawn_despawn, bench_insert);
criterion_main!(benches);
