use std::{env, fs};

use marshal_rs::{Arena, load};

fn main() {
    let mut args = env::args().skip(1);
    let input = args.next().expect("usage: yaml <input> <output.yaml>");
    let output = args.next().expect("usage: yaml <input> <output.yaml>");

    let bytes = fs::read(&input).expect("read input");
    let arena: Arena = load(&bytes).expect("valid Marshal data");

    let yaml = serde_norway::to_string(&arena).expect("serialize to YAML");
    fs::write(&output, yaml).expect("write output");

    println!("wrote {output} (from {} bytes of Marshal data)", bytes.len());
}
