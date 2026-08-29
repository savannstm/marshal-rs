//! Wall-clock comparison of marshal-rs against Ruby's stock `Marshal`
//! (`marshal.c`), run via the real `ruby` interpreter - see
//! `benches/marshal_c_compare.rb` for the Ruby-side timing. Requires `ruby`
//! on `PATH`.

use marshal_rs::{Arena, dump, load};
use std::{
    process::Command,
    time::Instant,
    {collections::HashMap, hint::black_box},
};

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

fn time_ns(iters: u32, mut f: impl FnMut()) -> u128 {
    for _ in 0..3 {
        f();
    }
    let start = Instant::now();
    for _ in 0..iters {
        f();
    }
    start.elapsed().as_nanos() / u128::from(iters)
}

fn run_rust(records: usize, iters: u32) -> (u128, u128) {
    let bytes = build_fixture(records);
    let arena = load(&bytes).unwrap().into_owned();

    let dump_ns = time_ns(iters, || {
        black_box(dump(black_box(&arena)));
    });
    let load_ns = time_ns(iters, || {
        black_box(load(black_box(&bytes)).unwrap());
    });

    (dump_ns, load_ns)
}

fn run_ruby() -> HashMap<(&'static str, usize), u128> {
    let script = concat!(env!("CARGO_MANIFEST_DIR"), "/benches/marshal_c_compare.rb");
    let output = Command::new("ruby")
        .arg(script)
        .output()
        .expect("failed to run `ruby` - is it on PATH?");
    assert!(
        output.status.success(),
        "ruby script failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let mut results = HashMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.trim().split(',');
        let (Some(op), Some(records), Some(ns)) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };
        let op = match op {
            "dump" => "dump",
            "load" => "load",
            _ => continue,
        };
        results.insert((op, records.parse().unwrap()), ns.parse().unwrap());
    }
    results
}

fn main() {
    const SIZES: [(usize, u32); 2] = [(100, 2000), (5000, 200)];

    println!("running Ruby's Marshal (marshal.c) via `ruby benches/marshal_c_compare.rb`...\n");
    let ruby_ns = run_ruby();

    println!(
        "{:<6} {:>8} {:>16} {:>18} {:>10}",
        "op", "records", "marshal-rs", "ruby (marshal.c)", "speedup"
    );
    for (records, iters) in SIZES {
        let (dump_ns, load_ns) = run_rust(records, iters);
        for (op, rust_ns) in [("dump", dump_ns), ("load", load_ns)] {
            let ruby = *ruby_ns
                .get(&(op, records))
                .expect("missing ruby result for this op/size");
            println!(
                "{op:<6} {records:>8} {rust_ns:>13} ns {ruby:>15} ns {:>9.2}x",
                ruby as f64 / rust_ns as f64
            );
        }
    }
}
