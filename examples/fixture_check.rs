use marshal_rs::{dump, load};
use std::{
    fs,
    path::{Path, PathBuf},
};

fn main() {
    let mut args = std::env::args().skip(1);
    let dir = args.next().expect("usage: fixture_check <dir> [out_dir]");
    let out_dir = args.next().map(PathBuf::from);

    let mut files = Vec::new();
    collect(Path::new(&dir), &mut files);
    files.sort();

    let mut ok_exact = 0;
    let mut ok_differs = 0;
    let mut failed = 0;

    for path in &files {
        let bytes = fs::read(path).expect("read fixture");
        match load(&bytes) {
            Ok(arena) => {
                let redumped = dump(&arena);
                if redumped == bytes {
                    ok_exact += 1;
                    println!("EXACT   {} ({} bytes)", path.display(), bytes.len());
                } else {
                    ok_differs += 1;
                    println!(
                        "DIFFERS {} (orig={} redump={})",
                        path.display(),
                        bytes.len(),
                        redumped.len()
                    );
                }
                if let Some(out_dir) = &out_dir {
                    let rel = path.strip_prefix(&dir).unwrap_or(path);
                    let dest = out_dir.join(rel);
                    fs::create_dir_all(dest.parent().unwrap()).unwrap();
                    fs::write(dest, &redumped).unwrap();
                }
            }
            Err(err) => {
                failed += 1;
                println!("ERROR   {}: {err}", path.display());
            }
        }
    }

    println!(
        "\n{} exact, {} differ, {} failed (of {})",
        ok_exact,
        ok_differs,
        failed,
        files.len()
    );
    assert_eq!(failed, 0, "some fixtures failed to load");
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("rxdata" | "rvdata" | "rvdata2")
        ) {
            out.push(path);
        }
    }
}
