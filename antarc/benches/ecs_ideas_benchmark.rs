#![allow(unused_variables, dead_code)]

use criterion::{criterion_group, criterion_main, Criterion};
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
struct FirstState;

#[derive(Debug)]
struct SecondState;

#[derive(Debug)]
struct Event {
    id: Entity,
}

const MAX: usize = 10000;

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
        .spawn_batch(data_list.into_iter().map(|data| (data, FirstState)))
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

#[allow(dead_code)]
fn insert() {
    let mut world = prepare_ecs();
    let mut to_change: Vec<Entity> = Vec::with_capacity(MAX / 2);

    let mut entered = 0;

    for (id, (_data,)) in world.query::<(&Data,)>().iter() {
        to_change.push(id);
        entered += 1;
    }

    assert_eq!(entered, MAX);

    while let Some(id) = to_change.pop() {
        let _ = world.insert(id, (Marker, Extra(0xed))).unwrap();
    }
}

#[allow(dead_code)]
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

#[allow(dead_code)]
fn hecs_insert_remove() {
    let mut world = World::new();
    let mut ids = world
        .spawn_batch((0..MAX).into_iter().map(|i| {
            (Data {
                x: 1.0 + i as f32,
                y: 1.1 + i as f32,
                z: 1.2 + i as f32,
                day: 2 + i as u32,
                month: 3 + i as u16,
                year: 4 + i as u32,
            },)
        }))
        .collect::<Vec<_>>();

    let mut ids_copy = ids.clone();

    while let Some(id) = ids.pop() {
        let _ = world.insert(id, (Marker, Extra(0xed))).unwrap();
    }

    while let Some(id) = ids_copy.pop() {
        let _ = world.remove::<(Marker, Extra)>(id).unwrap();
    }
}

#[allow(dead_code)]
fn hecs_spawn_despawn() {
    let mut world = World::new();
    let mut ids = world
        .spawn_batch((0..MAX).into_iter().map(|i| {
            (Data {
                x: 1.0 + i as f32,
                y: 1.1 + i as f32,
                z: 1.2 + i as f32,
                day: 2 + i as u32,
                month: 3 + i as u16,
                year: 4 + i as u32,
            },)
        }))
        .collect::<Vec<_>>();

    let mut ids_copy = ids.clone();
    while let Some(id) = ids.pop() {
        let _ = world.spawn((Event { id },));
    }

    while let Some(id) = ids_copy.pop() {
        let _ = world.despawn(id).unwrap();
    }
}

fn swap_state() {
    let mut world = prepare_ecs();
    let mut stale_states: Vec<Entity> = Vec::with_capacity(MAX / 2);

    let mut entered = 0;
    for (id, (_data, _first)) in world.query::<(&Data, &FirstState)>().iter() {
        stale_states.push(id);
        entered += 1;
    }
    assert_eq!(entered, MAX);

    while let Some(id) = stale_states.pop() {
        let _first = world.remove::<(FirstState,)>(id).unwrap();
        let _second = world.insert(id, (SecondState,)).unwrap();
    }

    let mut entered = 0;
    for (id, (data, second)) in world.query::<(&Data, &SecondState)>().iter() {
        entered += 1;
    }
    assert_eq!(entered, MAX);
}

fn insert_second_state_use_without() {
    let mut world = prepare_ecs();
    let mut stale_states: Vec<Entity> = Vec::with_capacity(MAX / 2);

    let mut entered = 0;
    for (id, (data, first)) in world
        .query::<(&Data, &FirstState)>()
        .without::<SecondState>()
        .iter()
    {
        stale_states.push(id);
        entered += 1;
    }
    assert_eq!(entered, MAX);

    while let Some(id) = stale_states.pop() {
        let _second = world.insert(id, (SecondState,)).unwrap();
    }

    let mut entered = 0;
    for (id, (data, second)) in world.query::<(&Data, &SecondState)>().iter() {
        entered += 1;
    }
    assert_eq!(entered, MAX);
}

fn bench_insert(c: &mut Criterion) {
    c.bench_function("insert", |b| b.iter(|| insert()));
}

fn bench_spawn_despawn(c: &mut Criterion) {
    c.bench_function("spawn_despawn", |b| b.iter(|| spawn_despawn()));
}

fn bench_hecs_insert_remove(c: &mut Criterion) {
    c.bench_function("hecs_insert_remove", |b| b.iter(|| hecs_insert_remove()));
}

fn bench_hecs_spawn_despawn(c: &mut Criterion) {
    c.bench_function("hecs_spawn_despawn", |b| b.iter(|| hecs_spawn_despawn()));
}

fn bench_swap_state(c: &mut Criterion) {
    c.bench_function("swap_state", |b| b.iter(|| swap_state()));
}

fn bench_insert_second_state_use_without(c: &mut Criterion) {
    c.bench_function("insert_second_state_use_without", |b| {
        b.iter(|| insert_second_state_use_without())
    });
}

criterion_group!(
    benches,
    // bench_spawn_despawn,
    // bench_insert,
    // bench_hecs_insert_remove,
    // bench_hecs_spawn_despawn,
    bench_swap_state,
    bench_insert_second_state_use_without
);
criterion_main!(benches);
