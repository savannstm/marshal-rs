use marshal_rs::{Dumper, Loader, Value};
use serde_json::{from_str, to_string};
use std::{
    env::var,
    fs::{create_dir_all, read, read_dir, read_to_string, write},
    path::PathBuf,
};

#[test]
fn main() {
    let game_path = PathBuf::from(var("GAME_PATH").unwrap());
    let data_path = game_path.join("Data");
    let loaded_path = game_path.join("loaded");
    let dumped_path = game_path.join("dumped");

    create_dir_all(&loaded_path).unwrap();
    create_dir_all(&dumped_path).unwrap();

    let entries = read_dir(&data_path).unwrap();
    let mut dumper = Dumper::new();

    for entry in entries.flatten() {
        let path = entry.path();
        let filename = entry.file_name();

        let buf = read(path).unwrap();
        let mut loader = Loader::new();
        let loaded = loader.load(&buf).unwrap();

        if filename != "Scripts.rvdata2" {
            write(loaded_path.join(&filename), to_string(&loaded).unwrap())
                .unwrap();
            from_str::<Value>(
                &read_to_string(loaded_path.join(&filename)).unwrap(),
            )
            .unwrap();
        }

        let dumped = dumper.dump(loaded);

        write(dumped_path.join(filename), dumped).unwrap();
    }
}
