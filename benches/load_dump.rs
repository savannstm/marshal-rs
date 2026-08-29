//! Load/dump throughput on a synthetic payload shaped like a real game's
//! data file: a wide array of small objects, each with a handful of string
//! and integer ivars sharing a small pool of repeated symbol names and
//! class names (the common case - RPG Maker-style save/map data is
//! thousands of small records, not one huge one).
//!
//! Built synthetically (not from a real game file) so the benchmark has no
//! external fixture dependency; `examples/fixture_check.rs` is what
//! exercises real RPG Maker data.

use criterion::{Criterion, criterion_group, criterion_main};
use marshal_rs::{Arena, dump, load};
use std::hint::black_box;

fn build_fixture(records: usize) -> Vec<u8> {
    let mut arena = Arena::builder();
    let mut elements = Vec::with_capacity(records);

    for i in 0..records {
        let name = arena.push_string(format!("Record {i}"));
        let hp = arena.push_fixnum((i % 9999) as i32);
        let mp = arena.push_fixnum((i % 999) as i32);
        let tag = arena.push_symbol(b"active".to_vec());
        let ivars = [
            (b"@name".to_vec(), name),
            (b"@hp".to_vec(), hp),
            (b"@mp".to_vec(), mp),
            (b"@tag".to_vec(), tag),
        ];
        elements.push(arena.push_object(b"Record".to_vec(), &ivars));
    }

    let root = arena.push_array(&elements);
    arena.set_root(root);
    dump(&arena)
}

fn bench_load(c: &mut Criterion) {
    let small = build_fixture(100);
    let large = build_fixture(5_000);

    let mut group = c.benchmark_group("load");
    group.bench_function("100 records", |b| {
        b.iter(|| black_box(load(black_box(&small)).unwrap()));
    });
    group.bench_function("5000 records", |b| {
        b.iter(|| black_box(load(black_box(&large)).unwrap()));
    });
    group.finish();
}

fn bench_dump(c: &mut Criterion) {
    let small = load(&build_fixture(100)).unwrap().into_owned();
    let large = load(&build_fixture(5_000)).unwrap().into_owned();

    let mut group = c.benchmark_group("dump");
    group.bench_function("100 records", |b| {
        b.iter(|| black_box(dump(black_box(&small))));
    });
    group.bench_function("5000 records", |b| {
        b.iter(|| black_box(dump(black_box(&large))));
    });
    group.finish();
}

fn bench_roundtrip(c: &mut Criterion) {
    let bytes = build_fixture(5_000);
    c.bench_function("roundtrip 5000 records", |b| {
        b.iter(|| {
            let arena = load(black_box(&bytes)).unwrap();
            black_box(dump(black_box(&arena)))
        });
    });
}

criterion_group!(benches, bench_load, bench_dump, bench_roundtrip);
criterion_main!(benches);
