//! marshal-rs against data written by real Ruby.
//!
//! Every fixture here is produced by a Ruby interpreter when the tests run (`tests/fixtures/generate.rb` with Ruby 4
//! on `PATH`, and `tests/fixtures/generate_ruby18.rb` with Ruby 1.8.7 when it can be found - set `RUBY187`, or
//! install it in `C:\Ruby187`), together with a description of what Ruby put in each one. The tests then require:
//!
//! * `load` + `dump` to give the fixture back **bit for bit**;
//! * the same through every serde shape (JSON, JSON with byte arrays, YAML) - except that serde copies shared values
//!   instead of linking them, so there the dump must only hold the same data;
//! * the `Arena` to be exactly what Ruby said it is (kinds, values, encodings, names, float bytes);
//! * Ruby itself - the interpreter that wrote the fixture - to load everything marshal-rs dumps, and see the same
//!   data (`tests/fixtures/verify.rb`).
//!
//! Without a Ruby these tests print a note and pass. Fixtures that fail are listed by name, with the reason, so a
//! failing run is a bug list.
#![cfg(not(miri))]

use marshal_rs::{
    Arena, Kind, ValueRef, dump, load, load_many,
    ser::{BinaryMode, Envelope},
};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::OnceLock,
};

// ----------------------------------------------------------------------------------------------------------
// Fixture sets
// ----------------------------------------------------------------------------------------------------------

struct Fixture {
    name: String,
    group: String,
    /// What Ruby says the value is (`None` for what it does not model: Time, Range, cycles, ...).
    desc: Option<Value>,
    entry: Value,
}

struct Set {
    label: &'static str,
    interpreter: String,
    flags: &'static [&'static str],
    classes: &'static str,
    dir: PathBuf,
    fixtures: Vec<Fixture>,
}

impl Set {
    fn bytes(&self, fixture: &Fixture) -> Vec<u8> {
        fs::read(self.dir.join(format!("{}.bin", fixture.name))).unwrap()
    }

    fn fixture(&self, name: &str) -> &Fixture {
        self.fixtures
            .iter()
            .find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("{}: no fixture {name}", self.label))
    }

    fn named(&self, name: &str) -> Vec<u8> {
        self.bytes(self.fixture(name))
    }

    fn group<'s>(&'s self, group: &'s str) -> impl Iterator<Item = &'s Fixture> {
        self.fixtures.iter().filter(move |fixture| fixture.group == group)
    }

    /// A directory for what one test produced from this set's fixtures.
    fn produced_dir(&self, shape: &str) -> PathBuf {
        let dir = self.dir.join(format!("produced-{shape}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Has the interpreter that wrote the fixtures load everything in `produced`, and its verifier must have checked
    /// every file there, exiting cleanly.
    fn verify_with_ruby(&self, produced: &Path) -> Vec<String> {
        let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        let output = Command::new(&self.interpreter)
            .args(self.flags)
            .arg(tests.join("verify.rb"))
            .arg(tests.join(self.classes))
            .arg(&self.dir)
            .arg(produced)
            .output()
            .expect("run the verifier");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let mut problems: Vec<String> = stdout
            .lines()
            .chain(stderr.lines())
            .filter(|line| line.starts_with("FAIL") || (line.contains("Error") && !line.starts_with("SAME-FAILURE")))
            .map(|line| format!("[{}] {line}", self.label))
            .collect();
        // Streams Ruby cannot read even as its original: only the listed ones may say so, and each listed one must.
        let same: Vec<&str> = stdout
            .lines()
            .filter_map(|line| line.strip_prefix("SAME-FAILURE "))
            .filter_map(|rest| rest.split(':').next())
            .map(|file| file.trim_end_matches(".bin"))
            .collect();
        // Only a known stream that was written here can fail here: the others are not part of this check.
        for known in KNOWN_OWN_READ_FAILURES
            .iter()
            .filter(|known| known.0 == self.label && produced.join(format!("{}.bin", known.1)).exists())
        {
            if !same.contains(&known.1) {
                problems.push(format!(
                    "[{}] {}: the known failure is gone; remove it from KNOWN_OWN_READ_FAILURES",
                    self.label, known.1
                ));
            }
        }
        for name in &same {
            if !KNOWN_OWN_READ_FAILURES
                .iter()
                .any(|known| known.0 == self.label && known.1 == *name)
            {
                problems.push(format!(
                    "[{}] {name}: Ruby cannot load it, nor its original, and it is not a known failure",
                    self.label
                ));
            }
        }
        let written = fs::read_dir(produced)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "bin"))
            .count();
        let checked = stdout
            .lines()
            .find_map(|line| line.strip_prefix("verified "))
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(|count| count.parse::<usize>().ok());
        if checked != Some(written) {
            problems.push(format!(
                "[{}] the verifier checked {checked:?} files, {written} were written",
                self.label
            ));
        }
        if !output.status.success() {
            problems.push(format!("[{}] the verifier exited with {}", self.label, output.status));
        }
        problems
    }
}

fn run_generator(interpreter: &str, flags: &[&str], script: &str, dir: &Path) -> Option<Vec<Fixture>> {
    let script = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(script);
    let output = Command::new(interpreter)
        .args(flags)
        .arg(&script)
        .arg(dir)
        .output()
        .ok()?;
    assert!(
        output.status.success(),
        "{} failed:\n{}{}",
        script.display(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let manifest: Value = serde_json::from_slice(&fs::read(dir.join("manifest.json")).unwrap()).unwrap();
    Some(
        manifest["fixtures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| Fixture {
                name: entry["name"].as_str().unwrap().to_owned(),
                group: entry["group"].as_str().unwrap().to_owned(),
                desc: Some(entry["desc"].clone()).filter(|desc| !desc.is_null()),
                entry: entry.clone(),
            })
            .collect(),
    )
}

/// The major version of `interpreter`, from `ruby -v` (`ruby 4.0.7 (...)`).
fn ruby_major(interpreter: &str) -> Option<u32> {
    let output = Command::new(interpreter).arg("-v").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .strip_prefix("ruby ")?
        .split('.')
        .next()?
        .parse()
        .ok()
}

fn ruby_187() -> Option<String> {
    let candidates = [
        std::env::var("RUBY187").unwrap_or_default(),
        "C:/Ruby187/bin/ruby.exe".to_owned(),
        "ruby1.8".to_owned(),
        "ruby187".to_owned(),
    ];
    candidates
        .into_iter()
        .filter(|candidate| !candidate.is_empty())
        .find(|candidate| {
            Command::new(candidate)
                .arg("-v")
                .output()
                .is_ok_and(|output| String::from_utf8_lossy(&output.stdout).contains("1.8.7"))
        })
}

/// The interpreter named by `env`, if it reports itself as `version` (`ruby 2.7.8p225 ...`).
fn ruby_named(env: &str, version: &str) -> Option<String> {
    let path = std::env::var(env).ok().filter(|path| !path.is_empty())?;
    let output = Command::new(&path).arg("-v").output().ok()?;
    String::from_utf8_lossy(&output.stdout)
        .starts_with(&format!("ruby {version}"))
        .then_some(path)
}

fn sets() -> &'static [Set] {
    static SETS: OnceLock<Vec<Set>> = OnceLock::new();
    SETS.get_or_init(|| {
        let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ruby-fixtures");
        let mut sets = Vec::new();

        // Ruby 4 is the reference: Ruby 3.3 cannot load its own `Café` class references, so its output never verifies.
        let dir = root.join("ruby");
        let generated = match ruby_major("ruby") {
            Some(major) if major >= 4 => run_generator("ruby", &[], "generate.rb", &dir),
            _ => None,
        };
        match generated {
            Some(fixtures) => sets.push(Set {
                label: "ruby",
                interpreter: "ruby".to_owned(),
                flags: &[],
                classes: "classes.rb",
                dir,
                fixtures,
            }),
            None if std::env::var_os("CI").is_some() => panic!("CI has no Ruby 4 on PATH to write the fixtures with"),
            None => eprintln!("the Ruby 4 fixture tests are skipped: they need Ruby 4 on PATH"),
        }

        match ruby_187() {
            Some(interpreter) => {
                let dir = root.join("ruby187");
                let fixtures = run_generator(&interpreter, &["-Ks"], "generate_ruby18.rb", &dir).unwrap();
                sets.push(Set {
                    label: "ruby1.8.7",
                    interpreter,
                    flags: &["-Ks"],
                    classes: "classes18.rb",
                    dir,
                    fixtures,
                });
            }
            None if std::env::var_os("RUBY187").is_some_and(|path| !path.is_empty()) => {
                panic!("RUBY187 does not name a Ruby 1.8.7 interpreter")
            }
            None => eprintln!("the Ruby 1.8.7 fixtures are skipped: no Ruby 1.8.7 (set RUBY187)"),
        }

        // Ruby 2.7.8, 3.3.8 and 1.9.3 get the same generator as Ruby 4, where their syntax and classes allow it;
        // generate.rb loads the version files they can read.
        for (label, dir_name, env, version) in [
            ("ruby2.7.8", "ruby278", "RUBY278", "2.7.8"),
            ("ruby3.3.8", "ruby338", "RUBY338", "3.3.8"),
            ("ruby1.9.3", "ruby193", "RUBY193", "1.9.3"),
        ] {
            match ruby_named(env, version) {
                Some(interpreter) => {
                    let dir = root.join(dir_name);
                    let fixtures = run_generator(&interpreter, &[], "generate.rb", &dir).unwrap();
                    sets.push(Set {
                        label,
                        interpreter,
                        flags: &[],
                        classes: "classes.rb",
                        dir,
                        fixtures,
                    });
                }
                None if std::env::var_os(env).is_some_and(|path| !path.is_empty()) => {
                    panic!("{env} does not name a Ruby {version} interpreter")
                }
                None => eprintln!("the Ruby {version} fixture tests are skipped: no Ruby {version} (set {env})"),
            }
        }
        sets
    })
}

fn from_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "odd hex length");
    (0..hex.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&hex[at..at + 2], 16).unwrap())
        .collect()
}

fn hex_of(desc: &Value, key: &str) -> Vec<u8> {
    from_hex(desc[key].as_str().unwrap_or_else(|| panic!("{desc} has no {key}")))
}

fn report(problems: &[String]) {
    assert!(
        problems.is_empty(),
        "{} problems:\n{}",
        problems.len(),
        problems.join("\n")
    );
}

// ----------------------------------------------------------------------------------------------------------
// Round trips
// ----------------------------------------------------------------------------------------------------------

/// Ruby 1.9.3 writes a binary (ASCII-8BIT) symbol with non-ASCII bytes inside an instance-variable wrapper that has no
/// variables, and it cannot read such a symbol back. 1.8.7 writes it bare, and 2.7.8 and later write it bare as well, so
/// the form is 1.9.3's own, and marshal-rs writes the bare form. These fixtures are known differences: they are not
/// written for Ruby to read, and each must still differ from marshal-rs's output, or the list is stale.
const RUBY193_BINARY_SYMBOLS: &[&str] = &[
    "sym_binary_high",
    "sym_binary_sjis_bytes",
    "sym_binary_with_nul",
    "sym_latin1_binary",
    "sym_same_bytes_three_encodings",
    "sym_links",
];

fn is_ruby193_binary_symbol(set: &Set, fixture: &Fixture) -> bool {
    set.label == "ruby1.9.3" && RUBY193_BINARY_SYMBOLS.contains(&fixture.name.as_str())
}

/// Ruby 2.7.8, 3.3.8 and 1.9.3 write a class name as plain bytes, where Ruby 4 wraps it in an encoding tag and they
/// cannot read the tagged form. marshal-rs builds Ruby 4's form, so a value with a non-ASCII class name differs from
/// their own dump: the same structure, written differently by different versions.
fn is_untagged_class_set(set: &Set) -> bool {
    matches!(set.label, "ruby2.7.8" | "ruby3.3.8" | "ruby1.9.3")
}

/// Ruby 1.8 writes instance variables in the order its build hashes their names, which differs between builds:
/// Windows writes a Range as `end`, `begin`, `excl`, and the descriptions list `excl`, `end`, `begin`. The values of
/// these fixtures are still checked by `core_classes_are_what_marshal_writes_for_them` and round-trip through Ruby.
const RUBY187_HASHED_IVAR_ORDER: &[&str] = &["r187_core_range", "r187_core_exceptions", "r187_core_exception_raised"];

fn is_ruby187_hashed_ivar_order(set: &Set, fixture: &Fixture) -> bool {
    set.label == "ruby1.8.7" && RUBY187_HASHED_IVAR_ORDER.contains(&fixture.name.as_str())
}

/// Whether `desc` holds a class or module reference (`c` or `m`) with a non-ASCII name, at any depth. Ruby 2.7.8 and
/// 1.9.3 write those names as plain bytes, where Ruby 4 tags them.
fn refers_to_non_ascii_class(desc: &Value) -> bool {
    match desc {
        Value::Object(map) => {
            let reference = matches!(map.get("t").and_then(Value::as_str), Some("class" | "module"));
            let non_ascii = map
                .get("name")
                .and_then(Value::as_str)
                .is_some_and(|hex| from_hex(hex).iter().any(|byte| !byte.is_ascii()));
            (reference && non_ascii) || map.values().any(refers_to_non_ascii_class)
        }
        Value::Array(items) => items.iter().any(refers_to_non_ascii_class),
        _ => false,
    }
}

/// Streams Ruby 2.7.8 or 3.3.8 cannot read even as it wrote them: marshal-rs's output is not the failure, so these are
/// listed by (set, fixture, why). Each must still be reported, or the entry is stale.
const KNOWN_OWN_READ_FAILURES: &[(&str, &str, &str)] = &[
    (
        "ruby3.3.8",
        "obj_class_and_module_refs",
        "3.3.8 writes the non-ASCII class names untagged, as 2.7.8 does, and cannot read them back (Café and the \
         other class references)",
    ),
    (
        "ruby2.7.8",
        "enc_big5_hkscs",
        "2.7.8 cannot read a symbol in this encoding, even its own dump (EncodingError)",
    ),
    (
        "ruby2.7.8",
        "enc_big5_uao",
        "2.7.8 cannot read a symbol in this encoding, even its own dump (EncodingError)",
    ),
    (
        "ruby2.7.8",
        "enc_cp951",
        "2.7.8 cannot read a symbol in this encoding, even its own dump (EncodingError)",
    ),
    (
        "ruby2.7.8",
        "obj_class_and_module_refs",
        "2.7.8 writes a class reference with a non-ASCII name plain, and cannot find the class from it (ArgumentError)",
    ),
];

/// Whether `error` says `words`, however the message is wrapped: YAML breaks long messages across lines.
fn contains_words(error: &str, words: &str) -> bool {
    error.split_whitespace().collect::<Vec<_>>().join(" ").contains(words)
}

/// Ruby fixtures with several streams in one file, as `load_many` reads them.
fn is_many(fixture: &Fixture) -> bool {
    fixture.group == "many"
}

fn marshal_dump_of(bytes: &[u8], many: bool) -> Result<Vec<u8>, String> {
    if many {
        let arenas = load_many(bytes).map_err(|error| format!("load_many: {error}"))?;
        Ok(arenas.iter().flat_map(|arena| dump(arena)).collect())
    } else {
        Ok(dump(&load(bytes).map_err(|error| format!("load: {error}"))?))
    }
}

/// Marshal -> `Arena` -> Marshal: the same bytes, and Ruby loads them.
#[test]
fn marshal_round_trips_are_bit_identical_and_ruby_loads_them() {
    let mut problems = Vec::new();
    for set in sets() {
        let produced = set.produced_dir("marshal");
        for fixture in &set.fixtures {
            let bytes = set.bytes(fixture);
            match marshal_dump_of(&bytes, is_many(fixture)) {
                Ok(redump) => {
                    if is_ruby193_binary_symbol(set, fixture) {
                        if redump == bytes {
                            problems.push(format!(
                                "[{}] {}: the known difference is gone; remove it",
                                set.label, fixture.name
                            ));
                        }
                        continue;
                    }
                    fs::write(produced.join(format!("{}.bin", fixture.name)), &redump).unwrap();
                    if redump != bytes {
                        let at = redump
                            .iter()
                            .zip(&bytes)
                            .position(|(a, b)| a != b)
                            .unwrap_or(redump.len().min(bytes.len()));
                        problems.push(format!(
                            "[{}] {}: the dump is not the input ({} vs {} bytes, first difference at {at})",
                            set.label,
                            fixture.name,
                            redump.len(),
                            bytes.len()
                        ));
                    }
                }
                Err(error) => problems.push(format!("[{}] {}: {error}", set.label, fixture.name)),
            }
        }
        problems.extend(set.verify_with_ruby(&produced));
    }
    report(&problems);
}

/// Whether one value is reachable twice (an object link in the stream). serde has no such thing: it copies.
fn has_shared_values(arena: &Arena<'_>) -> bool {
    fn walk(value: ValueRef<'_, '_>, seen: &mut HashSet<u32>) -> bool {
        // nil, booleans, fixnums and symbols are shared nodes by design, not by a link.
        if matches!(
            value.kind(),
            Kind::Nil | Kind::True | Kind::False | Kind::Fixnum | Kind::Symbol
        ) {
            return false;
        }
        if !seen.insert(value.id()) {
            return true;
        }
        let inside = match value.kind() {
            Kind::Array => value.array().any(|item| walk(item, seen)),
            Kind::Hash => {
                value.entries().any(|(key, item)| walk(key, seen) || walk(item, seen))
                    || value.default_value().is_some_and(|default| walk(default, seen))
            }
            Kind::Object | Kind::Struct => value.members().any(|(_, item)| walk(item, seen)),
            _ => false,
        };
        inside
            || value.extra_ivars().any(|(_, item)| walk(item, seen))
            || value.body().is_some_and(|body| walk(body, seen))
    }
    walk(ValueRef::root(arena), &mut HashSet::new())
}

type Shape = fn(&Arena<'_>) -> Result<Arena<'static>, String>;

fn through_json(arena: &Arena<'_>) -> Result<Arena<'static>, String> {
    let json = serde_json::to_string(arena).map_err(|error| error.to_string())?;
    serde_json::from_str(&json).map_err(|error| format!("from json: {error}"))
}

/// The format's own byte type: arrays of numbers in `serde_json::Value`.
fn through_byte_arrays(arena: &Arena<'_>) -> Result<Arena<'static>, String> {
    let value = serde_json::to_value(Envelope {
        arena,
        binary: BinaryMode::Bytes,
    })
    .map_err(|error| error.to_string())?;
    serde_json::from_value(value).map_err(|error| format!("from value: {error}"))
}

fn through_yaml(arena: &Arena<'_>) -> Result<Arena<'static>, String> {
    let yaml = yaml_serde::to_string(arena).map_err(|error| error.to_string())?;
    yaml_serde::from_str(&yaml).map_err(|error| format!("from yaml: {error}"))
}

/// Marshal -> `Arena` -> serde -> `Arena` -> Marshal. A value that contains itself has no tree form, and shared
/// values are copied (`ser.rs`), so those dumps have no object links; everything else must be the input, bit for bit.
/// Whatever comes out has to load in Ruby and be the same data.
fn serde_shape(shape_name: &str, shape: Shape) {
    let mut problems = Vec::new();
    // Streams this shape cannot express are not checked, and are printed (`--nocapture`) rather than dropped quietly.
    let mut skipped = Vec::new();
    for set in sets() {
        let produced = set.produced_dir(shape_name);
        for fixture in set.fixtures.iter().filter(|fixture| !is_many(fixture)) {
            let bytes = set.bytes(fixture);
            let arena = load(&bytes).unwrap();
            let back = match shape(&arena) {
                Ok(back) => back,
                Err(error) if contains_words(&error, "refers back to itself") => {
                    skipped.push(format!("[{}] {}: {error}", set.label, fixture.name));
                    continue;
                }
                // serde_json and YAML refuse to read deeper than their own limits (about 128 levels of
                // nesting, which is a few dozen of these envelopes): not marshal-rs's to fix.
                Err(error) if contains_words(&error, "recursion limit") => {
                    skipped.push(format!("[{}] {}: {error}", set.label, fixture.name));
                    continue;
                }
                Err(error) => {
                    problems.push(format!("[{}] {}: {error}", set.label, fixture.name));
                    continue;
                }
            };
            let redump = dump(&back);
            if is_ruby193_binary_symbol(set, fixture) {
                if redump == bytes {
                    problems.push(format!(
                        "[{}] {}: the known difference is gone; remove it",
                        set.label, fixture.name
                    ));
                }
                continue;
            }
            fs::write(produced.join(format!("{}.bin", fixture.name)), &redump).unwrap();
            if has_shared_values(&arena) {
                // Shared values were copied: the dump has no links, but every value has to be there, unchanged.
                if serde_json::to_string(&back).ok() != serde_json::to_string(&arena).ok() {
                    problems.push(format!("[{}] {}: the data changed", set.label, fixture.name));
                }
            } else if redump != bytes {
                problems.push(format!(
                    "[{}] {}: the dump after {shape_name} is not the input ({} vs {} bytes)",
                    set.label,
                    fixture.name,
                    redump.len(),
                    bytes.len()
                ));
            }
        }
        problems.extend(set.verify_with_ruby(&produced));
    }
    if !skipped.is_empty() {
        eprintln!(
            "{shape_name} did not check {} streams:\n{}",
            skipped.len(),
            skipped.join("\n")
        );
    }
    report(&problems);
}

#[test]
fn json_round_trips_and_ruby_loads_them() {
    serde_shape("json", through_json);
}

#[test]
fn json_with_byte_arrays_round_trips_and_ruby_loads_them() {
    serde_shape("json-bytes", through_byte_arrays);
}

#[test]
fn yaml_round_trips_and_ruby_loads_them() {
    serde_shape("yaml", through_yaml);
}

// ----------------------------------------------------------------------------------------------------------
// What Ruby says is in the fixtures
// ----------------------------------------------------------------------------------------------------------

fn check(value: ValueRef<'_, '_>, desc: &Value, at: &str) -> Result<(), String> {
    let kind = desc["t"].as_str().unwrap();
    let bad = |what: String| Err(format!("{at}: {what}"));
    match kind {
        "nil" if value.is_nil() => {}
        "true" if value.as_bool() == Some(true) => {}
        "false" if value.as_bool() == Some(false) => {}
        "int" => {
            if value.as_i64() != desc["v"].as_i64() {
                return bad(format!("fixnum {:?}, expected {}", value.as_i64(), desc["v"]));
            }
        }
        "big" => {
            if value.as_bigint_decimal().as_deref() != desc["v"].as_str() {
                return bad(format!(
                    "bignum {:?}, expected {}",
                    value.as_bigint_decimal(),
                    desc["v"]
                ));
            }
        }
        "float" => {
            let expected = hex_of(desc, "hex");
            if value.as_float_bytes() != Some(expected.as_slice()) {
                return bad(format!(
                    "float bytes {:?}, expected {expected:?}",
                    value.as_float_bytes()
                ));
            }
            // As a number only the text before the NUL counts; it must give back the double Ruby had.
            let number = value.as_f64();
            if desc["nan"].as_bool() == Some(true) {
                if !number.is_some_and(f64::is_nan) {
                    return bad(format!("float {number:?}, expected NaN"));
                }
            } else {
                let bits = u64::from_le_bytes(hex_of(desc, "bits").try_into().unwrap());
                if number.map(f64::to_bits) != Some(bits) {
                    return bad(format!("float {number:?}, expected {}", f64::from_bits(bits)));
                }
            }
        }
        "sym" => {
            let expected = hex_of(desc, "hex");
            if value.as_symbol_bytes() != Some(expected.as_slice()) {
                return bad(format!("symbol {:?}, expected {expected:?}", value.as_symbol_bytes()));
            }
            let encoding = desc["enc"].as_str().unwrap();
            if value.encoding_name() != Some(encoding.as_bytes()) {
                return bad(format!(
                    "symbol encoding {:?}, expected {encoding}",
                    value.encoding_name()
                ));
            }
        }
        "str" => {
            let expected = hex_of(desc, "hex");
            let encoding = desc["enc"].as_str().unwrap();
            if !matches!(value.kind(), Kind::Str | Kind::Bytes) || value.as_bytes() != Some(expected.as_slice()) {
                return bad(format!("string {:?}, expected {expected:?}", value.as_bytes()));
            }
            if value.encoding_name() != Some(encoding.as_bytes()) {
                return bad(format!("encoding {:?}, expected {encoding}", value.encoding_name()));
            }
            // `as_str` is the one place text is validated: UTF-8 or US-ASCII declared, and the bytes agree.
            let text = matches!(encoding, "UTF-8" | "US-ASCII")
                .then(|| std::str::from_utf8(&expected).ok())
                .flatten();
            if value.as_str() != text {
                return bad(format!("as_str {:?}, expected {text:?}", value.as_str()));
            }
            check_class(value, desc, at)?;
            check_extras(value, desc, at)?;
        }
        "arr" => {
            let items = desc["v"].as_array().unwrap();
            if value.kind() != Kind::Array || value.len() != items.len() {
                return bad(format!("array of {}, expected {}", value.len(), items.len()));
            }
            for (index, (item, expected)) in value.array().zip(items).enumerate() {
                check(item, expected, &format!("{at}[{index}]"))?;
            }
            check_class(value, desc, at)?;
            check_extras(value, desc, at)?;
        }
        "hash" => {
            let pairs = desc["v"].as_array().unwrap();
            if value.kind() != Kind::Hash || value.len() != pairs.len() {
                return bad(format!("hash of {}, expected {}", value.len(), pairs.len()));
            }
            for (index, ((key, item), expected)) in value.entries().zip(pairs).enumerate() {
                check(key, &expected[0], &format!("{at}.key{index}"))?;
                check(item, &expected[1], &format!("{at}.value{index}"))?;
            }
            if desc["identity"].as_bool() == Some(true) && value.class_name() != Some(&b"Hash"[..]) {
                return bad(format!(
                    "a hash by identity is a `Hash` user class, not {:?}",
                    value.class_name()
                ));
            }
            match (value.default_value(), desc.get("default")) {
                (Some(default), Some(expected)) => check(default, expected, &format!("{at}.default"))?,
                (None, None) => {}
                (found, expected) => {
                    return bad(format!("default {:?}, expected {expected:?}", found.map(|v| v.kind())));
                }
            }
            check_class(value, desc, at)?;
            check_extras(value, desc, at)?;
        }
        "regexp" => {
            let expected = hex_of(desc, "source");
            let options = desc["opts"].as_u64().unwrap() as u8;
            if value.as_regexp() != Some((expected.as_slice(), options)) {
                return bad(format!(
                    "regexp {:?}, expected {expected:?} /{options}",
                    value.as_regexp()
                ));
            }
            let encoding = desc["enc"].as_str().unwrap();
            if value.encoding_name() != Some(encoding.as_bytes()) {
                return bad(format!(
                    "regexp encoding {:?}, expected {encoding}",
                    value.encoding_name()
                ));
            }
            check_class(value, desc, at)?;
            check_extras(value, desc, at)?;
        }
        "class" | "module" => {
            let expected = hex_of(desc, "name");
            let expected_kind = if kind == "class" { Kind::Class } else { Kind::Module };
            if value.kind() != expected_kind || value.as_path() != Some(expected.as_slice()) {
                return bad(format!(
                    "{:?} {:?}, expected {expected:?}",
                    value.kind(),
                    value.as_path()
                ));
            }
        }
        "struct" => {
            check_class(value, desc, at)?;
            let members = desc["members"].as_array().unwrap();
            if value.kind() != Kind::Struct || value.len() != members.len() {
                return bad(format!("struct of {}, expected {}", value.len(), members.len()));
            }
            for ((name, item), expected) in value.members().zip(members) {
                let expected_name = from_hex(expected[0].as_str().unwrap());
                // Struct members are written without the `@` an ivar has.
                if name != expected_name.as_slice() {
                    return bad(format!("member {name:?}, expected {expected_name:?}"));
                }
                check(item, &expected[1], &format!("{at}.{}", String::from_utf8_lossy(name)))?;
            }
            check_extras(value, desc, at)?;
        }
        "obj" => {
            check_class(value, desc, at)?;
            if value.kind() != Kind::Object {
                return bad(format!("{:?}, expected an object", value.kind()));
            }
            // Ruby 1.8 keeps ivars in a hash table, so the order is not stable: compare by name.
            let mut found: Vec<_> = value.members().collect();
            found.sort_by_key(|(name, _)| name.to_vec());
            let mut expected: Vec<_> = desc["ivars"]
                .as_array()
                .unwrap()
                .iter()
                .map(|pair| (from_hex(pair[0].as_str().unwrap()), &pair[1]))
                .collect();
            expected.sort_by(|a, b| a.0.cmp(&b.0));
            if found.len() != expected.len() {
                return bad(format!("{} ivars, expected {}", found.len(), expected.len()));
            }
            for ((name, item), (expected_name, expected_item)) in found.into_iter().zip(expected) {
                if name != expected_name.as_slice() {
                    return bad(format!("ivar {name:?}, expected {expected_name:?}"));
                }
                check(item, expected_item, &format!("{at}.{}", String::from_utf8_lossy(name)))?;
            }
            let mut modules: Vec<Vec<u8>> = value.extensions().map(<[u8]>::to_vec).collect();
            modules.sort();
            let mut expected_modules: Vec<Vec<u8>> = desc["ext"]
                .as_array()
                .map(|list| list.iter().map(|module| from_hex(module.as_str().unwrap())).collect())
                .unwrap_or_default();
            expected_modules.sort();
            if modules != expected_modules {
                return bad(format!("extended with {modules:?}, expected {expected_modules:?}"));
            }
        }
        "udump" => {
            check_class(value, desc, at)?;
            let expected = hex_of(desc, "hex");
            if !value.is_user_defined() || value.as_bytes() != Some(expected.as_slice()) {
                return bad(format!("_dump data {:?}, expected {expected:?}", value.as_bytes()));
            }
            if value.encoding_name() != desc["enc"].as_str().map(str::as_bytes) {
                return bad(format!(
                    "_dump encoding {:?}, expected {}",
                    value.encoding_name(),
                    desc["enc"]
                ));
            }
        }
        "umarshal" => {
            if !value.is_user_marshal() {
                return bad("not a marshal_dump value".to_owned());
            }
            check_class(value, desc, at)?;
            check(value.body().unwrap_or(value), &desc["v"], &format!("{at}.marshal_dump"))?;
        }
        "nil" | "true" | "false" => return bad(format!("{:?}, expected {kind}", value.kind())),
        other => return bad(format!("unknown description {other}")),
    }
    Ok(())
}

/// The instance variables and `extend`s a string, array, hash, regexp or struct carries besides its own content.
fn check_extras(value: ValueRef<'_, '_>, desc: &Value, at: &str) -> Result<(), String> {
    let mut found: Vec<_> = value.extra_ivars().collect();
    found.sort_by_key(|(name, _)| name.to_vec());
    let mut expected: Vec<_> = desc["ivars"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|pair| (from_hex(pair[0].as_str().unwrap()), &pair[1]))
        .collect();
    expected.sort_by(|a, b| a.0.cmp(&b.0));
    if found.len() != expected.len() {
        return Err(format!(
            "{at}: {} extra ivars, expected {}",
            found.len(),
            expected.len()
        ));
    }
    for ((name, item), (expected_name, expected_item)) in found.into_iter().zip(expected) {
        if name != expected_name.as_slice() {
            return Err(format!("{at}: ivar {name:?}, expected {expected_name:?}"));
        }
        check(item, expected_item, &format!("{at}.{}", String::from_utf8_lossy(name)))?;
    }
    let mut modules: Vec<Vec<u8>> = value.extensions().map(<[u8]>::to_vec).collect();
    modules.sort();
    let mut expected_modules: Vec<Vec<u8>> = desc["ext"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|module| from_hex(module.as_str().unwrap()))
        .collect();
    expected_modules.sort();
    if modules != expected_modules {
        return Err(format!(
            "{at}: extended with {modules:?}, expected {expected_modules:?}"
        ));
    }
    Ok(())
}

fn check_class(value: ValueRef<'_, '_>, desc: &Value, at: &str) -> Result<(), String> {
    match desc["class"].as_str() {
        Some(hex) if value.class_name() != Some(from_hex(hex).as_slice()) => Err(format!(
            "{at}: class {:?}, expected {:?}",
            value.class_name(),
            from_hex(hex)
        )),
        _ => Ok(()),
    }
}

#[test]
fn arenas_hold_exactly_what_ruby_wrote() {
    let mut problems = Vec::new();
    let mut checked = 0;
    for set in sets() {
        for fixture in &set.fixtures {
            let Some(desc) = &fixture.desc else { continue };
            let bytes = set.bytes(fixture);
            let arena = load(&bytes).unwrap();
            checked += 1;
            if let Err(error) = check(ValueRef::root(&arena), desc, "root") {
                problems.push(format!("[{}] {}: {error}", set.label, fixture.name));
            }
        }
    }
    report(&problems);
    assert!(checked == 0 || checked > 100, "only {checked} fixtures were described");
}

// ----------------------------------------------------------------------------------------------------------
// Floats
// ----------------------------------------------------------------------------------------------------------

/// Ruby 1.8.7 really does write floats this way: 17 digits, a NUL, and raw mantissa bytes.
#[test]
fn ruby_1_8_floats_carry_non_utf8_mantissa_bytes_and_survive() {
    let Some(set) = sets().iter().find(|set| set.label == "ruby1.8.7") else {
        return;
    };
    let bytes = set.named("r187_floats");
    let arena = load(&bytes).unwrap();
    let floats: Vec<&[u8]> = ValueRef::root(&arena)
        .array()
        .map(|float| float.as_float_bytes().unwrap())
        .collect();

    let with_tail: Vec<_> = floats.iter().filter(|float| float.contains(&0)).collect();
    let non_utf8_tail = with_tail
        .iter()
        .filter(|float| std::str::from_utf8(&float[float.iter().position(|&b| b == 0).unwrap() + 1..]).is_err())
        .count();
    // Without these the checks below would prove nothing about the data this crate is for.
    assert!(
        with_tail.len() >= 30,
        "only {} floats have a mantissa tail",
        with_tail.len()
    );
    assert!(non_utf8_tail >= 10, "only {non_utf8_tail} tails are not UTF-8");

    // Rebuilding the same array float by float from those bytes gives Ruby's bytes back.
    let mut rebuilt = Arena::builder();
    let ids: Vec<u32> = floats.iter().map(|float| rebuilt.push_float(float.to_vec())).collect();
    let array = rebuilt.push_array(&ids).unwrap();
    rebuilt.set_root(array);
    assert_eq!(dump(&rebuilt), bytes);
}

/// The floats Ruby 4 cannot write, in the layouts real VX Ace data has, and with every kind of invalid tail.
#[test]
fn synthetic_floats_are_kept_and_dumped_exactly() {
    let mut problems = Vec::new();
    let mut invalid_tails = 0;
    for set in sets() {
        for fixture in set.group("float_synthetic") {
            let bytes = set.bytes(fixture);
            let arena = load(&bytes).unwrap();
            let root = ValueRef::root(&arena);
            let text = fixture.entry["text"].as_str().unwrap();
            let tail = hex_of(&fixture.entry, "tail");
            let mut whole = text.as_bytes().to_vec();
            whole.extend_from_slice(&tail);
            if std::str::from_utf8(&tail).is_err() {
                invalid_tails += 1;
            }
            if root.as_float_bytes() != Some(whole.as_slice()) {
                problems.push(format!(
                    "{}: float bytes {:?}, expected {whole:?}",
                    fixture.name,
                    root.as_float_bytes()
                ));
            }
            if dump(&arena) != bytes {
                problems.push(format!("{}: dump differs", fixture.name));
            }
            if let Some(bits) = fixture.entry["value_bits"].as_str() {
                let expected = f64::from_bits(u64::from_le_bytes(from_hex(bits).try_into().unwrap()));
                if root.as_f64() != Some(expected) {
                    problems.push(format!(
                        "{}: as_f64 {:?}, expected {expected}",
                        fixture.name,
                        root.as_f64()
                    ));
                }
            }
        }
    }
    report(&problems);
    assert!(
        sets().is_empty() || invalid_tails >= 30,
        "only {invalid_tails} fixtures have a non-UTF-8 tail"
    );
}

#[test]
fn synthetic_floats_in_an_array_keep_their_links() {
    for set in sets().iter().filter(|set| set.label == "ruby") {
        let fixture = set.fixture("float_synthetic_array_with_links");
        let count = fixture.entry["count"].as_u64().unwrap() as usize;
        let bytes = set.bytes(fixture);
        let arena = load(&bytes).unwrap();
        let root = ValueRef::root(&arena);
        assert_eq!(root.len(), count * 2 + 1);
        for index in 0..count {
            let original = root.at(index).unwrap();
            let linked = root.at(count + index).unwrap();
            assert_eq!(original.id(), linked.id(), "link {index} is not the same node");
            assert!(
                original.as_float_bytes().unwrap().contains(&0),
                "float {index} has no mantissa bytes"
            );
        }
        assert_eq!(
            root.at(count * 2).unwrap().as_float_bytes(),
            Some(&b"2.5\0\xff\xfe"[..])
        );
    }
}

// ----------------------------------------------------------------------------------------------------------
// Names, and the rest of the API on real data
// ----------------------------------------------------------------------------------------------------------

/// Names that are not UTF-8 are carried as `{"__base64": ...}` instead of being rejected or replaced.
#[test]
fn names_that_are_not_utf8_are_bytes_in_the_envelope() {
    let Some(set) = sets().iter().find(|set| set.label == "ruby1.8.7") else {
        return;
    };
    let bytes = set.named("r187_obj_sjis_names");
    let arena = load(&bytes).unwrap();
    let json = serde_json::to_string(&arena).unwrap();
    assert!(json.contains(r#""__class":{"__base64":"#), "{json}");
    assert!(json.contains(r#"[{"__base64":"#), "{json}");

    // UTF-8 names stay plain strings.
    let plain_bytes = set.named("r187_obj_plain");
    let plain = load(&plain_bytes).unwrap();
    let json = serde_json::to_string(&plain).unwrap();
    assert!(
        json.contains(r#""__class":"Plain""#) && json.contains(r#"["@a",1]"#),
        "{json}"
    );
}

#[test]
fn marshal_dump_wrappers_are_nested_envelopes() {
    let Some(set) = sets().iter().find(|set| set.label == "ruby") else {
        return;
    };
    let bytes = set.named("obj_user_marshal");
    let arena = load(&bytes).unwrap();
    let json = serde_json::to_value(&arena).unwrap();
    assert_eq!(json["__type"], "user_marshal");
    assert_eq!(json["__class"], "Marshaled");
    assert_eq!(json["__value"]["__type"], "array");
}

#[test]
fn shared_values_load_as_one_node() {
    for set in sets().iter().filter(|set| set.label == "ruby") {
        let bytes = set.named("basic_links");
        let arena = load(&bytes).unwrap();
        let root = ValueRef::root(&arena);
        assert_eq!(root.at(0).unwrap().id(), root.at(1).unwrap().id());
        assert_eq!(root.at(2).unwrap().id(), root.at(3).unwrap().id());
        assert_ne!(root.at(0).unwrap().id(), root.at(2).unwrap().id());
    }
}

#[test]
fn symbol_lookups_on_a_real_hash() {
    let Some(set) = sets().iter().find(|set| set.label == "ruby") else {
        return;
    };
    let bytes = set.named("basic_symbol_link_keys");
    let arena = load(&bytes).unwrap();
    let hash = ValueRef::root(&arena).at(2).unwrap();
    assert_eq!(hash.lookup_symbol("alpha").and_then(|v| v.as_i64()), Some(5));
    assert_eq!(hash.lookup_symbol("beta").and_then(|v| v.as_i64()), Some(6));
    assert!(hash.lookup_symbol("gamma").is_none());
    let index = hash.symbol_index();
    assert_eq!(index.len(), 2);
    assert_eq!(index[&b"alpha"[..]].as_i64(), Some(5));
}

#[test]
fn an_owned_arena_outlives_its_input_and_still_dumps() {
    for set in sets() {
        let name = if set.label == "ruby1.8.7" {
            "r187_complex_actors"
        } else {
            "basic_hash"
        };
        let expected = set.named(name);
        let arena = {
            let bytes = expected.clone();
            load(&bytes).unwrap().into_owned()
        };
        assert_eq!(dump(&arena), expected);
    }
}

#[test]
fn load_many_splits_concatenated_streams() {
    let Some(set) = sets().iter().find(|set| set.label == "ruby") else {
        return;
    };
    let bytes = set.named("many_streams");
    let arenas = load_many(&bytes).unwrap();
    assert_eq!(arenas.len(), 4);
    assert_eq!(ValueRef::root(&arenas[0]).as_i64(), Some(1));
    assert_eq!(ValueRef::root(&arenas[1]).as_str(), Some("two"));
    assert_eq!(ValueRef::root(&arenas[2]).len(), 2);
    assert!(ValueRef::root(&arenas[3]).is_nil());
    assert!(load_many(&bytes[..bytes.len() - 1]).is_err(), "a cut-off last stream");
    assert!(load_many(&bytes[..bytes.len() - 4]).is_err());
    assert!(load_many(b"").unwrap().is_empty());
    assert!(load(&bytes).is_err(), "`load` wants exactly one stream");
}

#[test]
fn encodings_come_back_by_name() {
    let Some(set) = sets().iter().find(|set| set.label == "ruby") else {
        return;
    };
    for fixture in set.group("string") {
        let Some(desc) = &fixture.desc else { continue };
        let bytes = set.bytes(fixture);
        let arena = load(&bytes).unwrap();
        let root = ValueRef::root(&arena);
        let encoding = desc["enc"].as_str();
        if encoding == Some("ASCII-8BIT") {
            assert_eq!(root.kind(), Kind::Bytes, "{}", fixture.name);
        } else if desc["t"] == "str" {
            assert_eq!(root.kind(), Kind::Str, "{}", fixture.name);
        }
    }
}

// ----------------------------------------------------------------------------------------------------------
// Encodings: carried by symbols, and changed through the API - Ruby is the oracle for the bytes
// ----------------------------------------------------------------------------------------------------------

fn ruby_set() -> Option<&'static Set> {
    sets().iter().find(|set| set.label == "ruby")
}

/// Loads `name`, lets `change` edit the root, and expects the dump to be what Ruby wrote for `expected`.
fn changed_like_ruby(set: &Set, name: &str, expected: &str, change: impl FnOnce(&mut Arena<'_>, u32) -> bool) {
    let bytes = set.named(name);
    let mut arena = load(&bytes).unwrap();
    let root = arena.root();
    assert!(change(&mut arena, root), "{name}: the change was refused");
    assert_eq!(dump(&arena), set.named(expected), "{name} changed is not {expected}");
    // And the result is a value like any other: it loads, and says what it is.
    let again = dump(&arena);
    assert_eq!(dump(&load(&again).unwrap()), again);
}

/// A symbol keeps the encoding it was loaded with and dumps with it - Shift_JIS, EUC-JP, UTF-8, binary and none.
#[test]
fn symbols_keep_their_encoding() {
    for set in sets().iter().filter(|set| set.label == "ruby") {
        for (name, encoding) in [
            ("sym_sjis_tagged", "Shift_JIS"),
            ("sym_euc_jp_tagged", "EUC-JP"),
            ("sym_utf8", "UTF-8"),
            ("sym_ascii", "US-ASCII"),
            ("sym_binary_high", "ASCII-8BIT"),
            ("sym_latin1", "ISO-8859-1"),
            ("sym_win1252", "Windows-1252"),
        ] {
            let bytes = set.named(name);
            let arena = load(&bytes).unwrap();
            assert_eq!(
                ValueRef::root(&arena).encoding_name(),
                Some(encoding.as_bytes()),
                "{name}"
            );
            assert_eq!(dump(&arena), bytes, "{name}");
        }
    }
}

/// The same bytes in different encodings are different symbols: three here, and the repeats link to them.
#[test]
fn symbols_with_the_same_bytes_and_different_encodings_stay_apart() {
    let Some(set) = ruby_set() else { return };
    let bytes = set.named("sym_same_bytes_three_encodings");
    let arena = load(&bytes).unwrap();
    let root = ValueRef::root(&arena);
    let names: Vec<&[u8]> = root
        .array()
        .take(6)
        .map(|symbol| symbol.encoding_name().unwrap())
        .collect();
    let expected: [&[u8]; 6] = [
        b"ISO-8859-1",
        b"Windows-1252",
        b"ASCII-8BIT",
        b"ISO-8859-1",
        b"Windows-1252",
        b"ASCII-8BIT",
    ];
    assert_eq!(names, expected);
    assert_eq!(root.at(0).unwrap().id(), root.at(3).unwrap().id());
    assert_ne!(root.at(0).unwrap().id(), root.at(1).unwrap().id());
    assert_eq!(dump(&arena), bytes);

    // Building them from the API finds the same three.
    let mut built = Arena::builder();
    let latin = b"caf\xe9".to_vec();
    let one = built.push_symbol_with_encoding_name(latin.clone(), b"ISO-8859-1");
    let two = built.push_symbol_with_encoding_name(latin.clone(), b"Windows-1252");
    let three = built
        .push_symbol_with_encoding(latin.clone(), marshal_rs::ENCODING_ASCII_8BIT)
        .unwrap();
    let again = built.push_symbol_with_encoding_name(latin, b"ISO-8859-1");
    let array = built.push_array(&[one, two, three, again]).unwrap();
    built.set_root(array);
    let dumped = dump(&built);
    let rebuilt = load(&dumped).unwrap();
    let root = ValueRef::root(&rebuilt);
    assert_eq!(root.at(0).unwrap().encoding_name(), Some(&b"ISO-8859-1"[..]));
    assert_eq!(root.at(3).unwrap().encoding_name(), Some(&b"ISO-8859-1"[..]));
}

/// Strings and symbols that name an encoding share the name object, and links keep resolving past it.
#[test]
fn encoding_names_are_shared_between_strings_and_symbols() {
    let Some(set) = ruby_set() else { return };
    let bytes = set.named("sym_and_str_share_encoding_names");
    let arena = load(&bytes).unwrap();
    let root = ValueRef::root(&arena);
    let names: Vec<&[u8]> = root.array().map(|value| value.encoding_name().unwrap()).collect();
    let expected: [&[u8]; 8] = [
        b"Shift_JIS",
        b"Shift_JIS",
        b"Shift_JIS",
        b"EUC-JP",
        b"Shift_JIS",
        b"EUC-JP",
        b"EUC-JP",
        b"ISO-8859-1",
    ];
    assert_eq!(names, expected);
    assert_eq!(dump(&arena), bytes);
}

/// A symbol built through the API with an encoding is written the way Ruby writes it.
#[test]
fn symbols_built_with_an_encoding_dump_like_ruby() {
    for set in sets() {
        let mut checked = 0;
        for fixture in set
            .fixtures
            .iter()
            .filter(|fixture| fixture.desc.as_ref().is_some_and(|desc| desc["t"] == "sym"))
        {
            let desc = fixture.desc.as_ref().unwrap();
            let name = hex_of(desc, "hex");
            let encoding = desc["enc"].as_str().unwrap();
            let mut arena = Arena::builder();
            // An ASCII symbol is written bare, which is what "no tag" is.
            let symbol = if encoding == "US-ASCII" {
                arena
                    .push_symbol_with_encoding(name, marshal_rs::ENCODING_ASCII_8BIT)
                    .unwrap()
            } else {
                arena.push_symbol_with_encoding_name(name, encoding.as_bytes())
            };
            arena.set_root(symbol);
            let built = dump(&arena);
            if is_ruby193_binary_symbol(set, fixture) {
                assert_ne!(
                    built,
                    set.bytes(fixture),
                    "[{}] {}: the known difference is gone",
                    set.label,
                    fixture.name
                );
                continue;
            }
            assert_eq!(built, set.bytes(fixture), "[{}] {}", set.label, fixture.name);
            checked += 1;
        }
        assert!(checked >= 3, "[{}] only {checked} symbols", set.label);
    }
}

/// Replacing what a string holds, and what it is tagged as, comes out the way Ruby writes the same string.
#[test]
fn changing_a_strings_text_and_encoding_matches_ruby() {
    let Some(set) = ruby_set() else { return };

    // The bytes stay, the tag changes (`force_encoding`) - including to and from "none".
    changed_like_ruby(set, "str_shift_jis", "str_force_euc_jp", |arena, id| {
        arena.set_encoding_name(id, b"EUC-JP")
    });
    changed_like_ruby(set, "str_shift_jis", "str_force_utf8", |arena, id| {
        arena.set_encoding(id, marshal_rs::ENCODING_UTF_8)
    });
    changed_like_ruby(set, "str_shift_jis", "str_force_binary", |arena, id| {
        arena.set_encoding(id, marshal_rs::ENCODING_ASCII_8BIT)
    });
    changed_like_ruby(set, "str_binary_sjis_bytes", "str_shift_jis_short", |arena, id| {
        arena.set_encoding_name(id, b"Shift_JIS")
    });

    // New text is UTF-8, whatever the old text was; new bytes keep the old tag unless told otherwise.
    changed_like_ruby(set, "str_shift_jis", "str_utf8", |arena, id| {
        arena
            .set_string_text(id, "héllo wörld 日本語のテキスト 🎉".to_owned())
            .is_some()
    });
    changed_like_ruby(set, "str_binary", "str_utf8", |arena, id| {
        arena
            .set_string_text(id, "héllo wörld 日本語のテキスト 🎉".to_owned())
            .is_some()
    });
    let other = hex_of(set.fixture("str_shift_jis_other").desc.as_ref().unwrap(), "hex");
    changed_like_ruby(set, "str_shift_jis", "str_shift_jis_other", |arena, id| {
        arena.set_str_content(id, other.clone()).is_some()
    });
    changed_like_ruby(set, "str_euc_jp", "str_shift_jis_other", |arena, id| {
        arena
            .set_str_with_encoding(id, other.clone(), marshal_rs::encoding_id(b"Shift_JIS").unwrap())
            .is_some()
    });
    changed_like_ruby(set, "str_utf8", "str_us_ascii", |arena, id| {
        arena
            .set_str_with_encoding(id, b"ascii only".to_vec(), marshal_rs::ENCODING_US_ASCII)
            .is_some()
    });

    // Things that are not strings, and encodings that do not exist, are refused and change nothing.
    let bytes = set.named("basic_array_mixed");
    let mut arena = load(&bytes).unwrap();
    let root = arena.root();
    assert!(!arena.set_encoding(root, marshal_rs::ENCODING_UTF_8));
    assert!(!arena.set_encoding_name(root, b"UTF-8"));
    assert!(arena.set_string_text(root, "x".to_owned()).is_none());
    assert_eq!(dump(&arena), bytes);
    let bytes = set.named("str_utf8");
    let mut arena = load(&bytes).unwrap();
    let root = arena.root();
    assert!(!arena.set_encoding(root, 200));
    assert!(arena.set_str_with_encoding(root, b"x".to_vec(), 200).is_none());
    assert_eq!(dump(&arena), bytes);
}

/// The same for a symbol: its bytes stay, the encoding is another one (and not another symbol's).
#[test]
fn changing_a_symbols_encoding_matches_ruby() {
    let Some(set) = ruby_set() else { return };
    changed_like_ruby(set, "sym_latin1", "sym_win1252", |arena, id| {
        arena.set_encoding_name(id, b"Windows-1252")
    });
    changed_like_ruby(set, "sym_latin1", "sym_latin1_binary", |arena, id| {
        arena.set_encoding(id, marshal_rs::ENCODING_ASCII_8BIT)
    });
    changed_like_ruby(set, "sym_latin1_binary", "sym_latin1", |arena, id| {
        arena.set_encoding_name(id, b"ISO-8859-1")
    });
    changed_like_ruby(set, "sym_sjis_tagged", "sym_binary_sjis_bytes", |arena, id| {
        arena.set_encoding(id, marshal_rs::ENCODING_ASCII_8BIT)
    });
}

/// The low-level reader hands a tagged symbol out as one token, with the encoding it declared.
#[test]
fn the_reader_reports_a_symbols_encoding() {
    use marshal_rs::{Reader, SymbolEncoding, Token};
    let Some(set) = ruby_set() else { return };
    for (name, expected) in [
        ("sym_sjis_tagged", SymbolEncoding::Named(b"Shift_JIS")),
        ("sym_utf8", SymbolEncoding::Utf8),
        ("sym_binary_high", SymbolEncoding::Bare),
        ("sym_ascii", SymbolEncoding::Bare),
    ] {
        let bytes = set.named(name);
        let mut table: Vec<marshal_rs::Span> = Vec::new();
        let mut reader = Reader::new(&bytes, &mut table).unwrap();
        let Token::Symbol(symbol) = reader.next().unwrap() else {
            panic!("{name}: not a single symbol token")
        };
        assert_eq!(
            symbol,
            ValueRef::root(&load(&bytes).unwrap()).as_symbol_bytes().unwrap(),
            "{name}"
        );
        assert_eq!(reader.last_symbol_encoding(), expected, "{name}");
        assert_eq!(reader.remaining(), 0, "{name}");
    }
}

// ----------------------------------------------------------------------------------------------------------
// Every encoding Ruby knows
// ----------------------------------------------------------------------------------------------------------

fn slug(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect()
}

/// The names of the encodings this Ruby has, as the fixtures say them.
fn ruby_encodings(set: &Set) -> Vec<String> {
    set.group("encoding_one")
        .map(|fixture| fixture.desc.as_ref().unwrap()["enc"].as_str().unwrap().to_owned())
        .collect()
}

/// Strings, symbols and regexps in every encoding Ruby 4 has (103): each is kept under its name, the name is
/// written once and linked after, and the dump is Ruby's own bytes. Ruby loads what marshal-rs writes, in the
/// generic tests (the `encoding` and `encoding_one` groups are in them).
#[test]
fn every_encoding_ruby_knows_is_kept_by_name() {
    let Some(set) = ruby_set() else { return };
    let names = ruby_encodings(set);
    assert!(names.len() >= 100, "only {} encodings", names.len());

    // The table has no name Ruby does not know (a typo would silently turn a name into a custom one) ...
    for table_name in marshal_rs::encoding::ENCODING_NAMES {
        let table_name = std::str::from_utf8(table_name).unwrap();
        assert!(
            names.iter().any(|name| name == table_name),
            "{table_name} is not a Ruby encoding"
        );
    }
    // ... and Ruby has none it lacks, or this is a Ruby with encodings the table has not seen: they still work
    // as custom names (below), but the table is what keeps ids stable, so say so.
    let missing: Vec<_> = names
        .iter()
        .filter(|name| marshal_rs::encoding_id(name.as_bytes()).is_none())
        .collect();
    assert!(missing.is_empty(), "encodings missing from ENCODING_NAMES: {missing:?}");

    for name in &names {
        let fixture = set.fixture(&format!("enc_one_{}", slug(name)));
        let bytes = set.bytes(fixture);
        let arena = load(&bytes).unwrap();
        let root = ValueRef::root(&arena);
        assert_eq!(root.encoding_name(), Some(name.as_bytes()), "{name}");
        let id = marshal_rs::encoding_id(name.as_bytes()).unwrap();
        assert_eq!(marshal_rs::encoding_name(id), Some(name.as_bytes()), "{name}");
        // Binary strings are the only ones that are `Bytes`, and their id is the first.
        assert_eq!(root.kind() == Kind::Bytes, name == "ASCII-8BIT", "{name}");
        assert_eq!(root.encoding_id(), Some(id), "{name}");
        assert_eq!(dump(&arena), bytes, "{name}");

        // The array fixture has every value twice: the second time is a link to the first, and the name is
        // written once for the whole document.
        let many = set.bytes(set.fixture(&format!("enc_{}", slug(name))));
        if name != "ASCII-8BIT" && name != "UTF-8" && name != "US-ASCII" {
            let occurrences = many
                .windows(name.len())
                .filter(|window| *window == name.as_bytes())
                .count();
            assert_eq!(occurrences, 1, "{name} is written {occurrences} times");
        }
        assert_eq!(dump(&load(&many).unwrap()), many, "{name}");
    }
}

/// Re-tagging a string with every one of the encodings leaves its bytes alone and writes what `force_encoding`
/// writes, from three different starting points.
#[test]
fn retagging_a_string_to_any_encoding_matches_ruby() {
    let Some(set) = ruby_set() else { return };
    let names = ruby_encodings(set);
    let mut problems = Vec::new();
    for start in ["ASCII-8BIT", "UTF-8", "Shift_JIS", "UTF-16LE"] {
        let before = set.named(&format!("retag_{}", slug(start)));
        for name in &names {
            let mut arena = load(&before).unwrap();
            let root = arena.root();
            let id = marshal_rs::encoding_id(name.as_bytes()).unwrap();
            // By id and by name, the same result.
            let changed_by_id = arena.set_encoding(root, id);
            let by_id = dump(&arena);
            let mut again = load(&before).unwrap();
            let changed_by_name = again.set_encoding_name(root, name.as_bytes());
            let expected = set.named(&format!("retag_{}", slug(name)));
            if !changed_by_id || !changed_by_name || by_id != expected || dump(&again) != expected {
                problems.push(format!("{start} -> {name}"));
            }
            // And the result is a value like any other.
            let reloaded = load(&expected).unwrap();
            if ValueRef::root(&arena).encoding_name() != ValueRef::root(&reloaded).encoding_name()
                || ValueRef::root(&arena).kind() != ValueRef::root(&reloaded).kind()
            {
                problems.push(format!("{start} -> {name}: not the value Ruby's bytes load as"));
            }
        }
    }
    report(&problems);
}

/// A name no table knows is kept as it is, shared between users, changeable and serde-able.
#[test]
fn unknown_encoding_names_are_kept_and_shared() {
    let mut arena = Arena::builder();
    let first = arena.push_str_with_encoding_name(b"hi".to_vec(), b"My-Enc");
    let second = arena.push_str_with_encoding_name(b"yo".to_vec(), b"Other");
    let symbol = arena.push_symbol_with_encoding_name(b"abc\xff".to_vec(), b"My-Enc");
    let regexp = arena.push_regexp_with_encoding_name(b"ab".to_vec(), 0, b"My-Enc");
    let again = arena.push_str_with_encoding_name(b"zz".to_vec(), b"Other");
    let array = arena
        .push_array(&[first, second, symbol, regexp, again, first])
        .unwrap();
    arena.set_root(array);
    assert_eq!(marshal_rs::encoding_id(b"My-Enc"), None);

    let bytes = dump(&arena);
    let count = |bytes: &[u8], name: &[u8]| bytes.windows(name.len()).filter(|window| *window == name).count();
    assert_eq!(count(&bytes, b"My-Enc"), 1);
    assert_eq!(count(&bytes, b"Other"), 1);

    let loaded = load(&bytes).unwrap();
    assert_eq!(dump(&loaded), bytes);
    let root = ValueRef::root(&loaded);
    let names: Vec<_> = root.array().map(|value| value.encoding_name().unwrap()).collect();
    let expected: [&[u8]; 6] = [b"My-Enc", b"Other", b"My-Enc", b"My-Enc", b"Other", b"My-Enc"];
    assert_eq!(names, expected);
    assert_eq!(root.at(2).unwrap().as_symbol_bytes(), Some(&b"abc\xff"[..]));

    for shape in [through_json, through_byte_arrays, through_yaml] {
        let back = shape(&loaded).unwrap();
        let names: Vec<_> = ValueRef::root(&back)
            .array()
            .map(|value| value.encoding_name().unwrap().to_vec())
            .collect();
        assert_eq!(names, expected.map(<[u8]>::to_vec));
    }

    // Changing to and from a custom name.
    let mut changed = load(&bytes).unwrap();
    let id = ValueRef::root(&changed).at(0).unwrap().id();
    assert!(changed.set_encoding_name(id, b"UTF-8"));
    assert_eq!(
        ValueRef::root(&changed).at(0).unwrap().encoding_name(),
        Some(&b"UTF-8"[..])
    );
    assert!(changed.set_encoding_name(id, b"Third"));
    assert_eq!(
        ValueRef::root(&changed).at(0).unwrap().encoding_name(),
        Some(&b"Third"[..])
    );
    assert_eq!(
        ValueRef::root(&changed).at(5).unwrap().encoding_name(),
        Some(&b"Third"[..]),
        "5 is the same string"
    );
    assert_eq!(dump(&load(&dump(&changed)).unwrap()), dump(&changed));
}

// ----------------------------------------------------------------------------------------------------------
// The builder API against Ruby: every described value, built from its description, dumps to Ruby's bytes
// ----------------------------------------------------------------------------------------------------------

/// Little-endian magnitude of a decimal number.
fn decimal_to_le(digits: &str) -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    for digit in digits.bytes() {
        let mut carry = u32::from(digit - b'0');
        for byte in &mut out {
            let value = u32::from(*byte) * 10 + carry;
            *byte = value as u8;
            carry = value >> 8;
        }
        while carry > 0 {
            out.push(carry as u8);
            carry >>= 8;
        }
    }
    out
}

/// Builds the value `desc` describes with the `push_*` API. `None`: not something the API can say (a `Time`'s
/// zone lives in instance variables of its `_dump` data, which the description does not list).
fn build(arena: &mut Arena<'static>, desc: &Value) -> Option<u32> {
    let class = desc["class"].as_str().map(from_hex);
    // The subclass, then what the value carries besides its content: instance variables and `extend`s.
    let with_class = |arena: &mut Arena<'static>, id: u32| {
        if let Some(class) = &class {
            if !arena.set_user_class(id, class.clone()) {
                return None;
            }
        }
        for module in desc["ext"].as_array().into_iter().flatten() {
            assert!(arena.push_extension(id, from_hex(module.as_str()?)));
        }
        if desc["t"] != "obj" {
            for pair in desc["ivars"].as_array().into_iter().flatten() {
                let value = build(arena, &pair[1])?;
                assert!(arena.push_extra_ivar(id, &from_hex(pair[0].as_str()?), value));
            }
        }
        Some(id)
    };
    Some(match desc["t"].as_str()? {
        "nil" => arena.push_nil(),
        "true" => arena.push_bool(true),
        "false" => arena.push_bool(false),
        "int" => arena.push_fixnum(desc["v"].as_i64()? as i32),
        "big" => {
            let text = desc["v"].as_str()?;
            let (negative, digits) = text.strip_prefix('-').map_or((false, text), |digits| (true, digits));
            arena.push_bignum(negative, decimal_to_le(digits))
        }
        "float" => arena.push_float(hex_of(desc, "hex")),
        "sym" => {
            let bytes = hex_of(desc, "hex");
            match desc["enc"].as_str()? {
                // An ASCII symbol is written bare, which is what "no tag" is.
                "US-ASCII" => arena.push_symbol_with_encoding(bytes, marshal_rs::ENCODING_ASCII_8BIT)?,
                name => arena.push_symbol_with_encoding_name(bytes, name.as_bytes()),
            }
        }
        "str" => {
            let bytes = hex_of(desc, "hex");
            let id = match desc["enc"].as_str()? {
                "ASCII-8BIT" => arena.push_bytes(bytes),
                name => arena.push_str_with_encoding_name(bytes, name.as_bytes()),
            };
            with_class(arena, id)?
        }
        "arr" => {
            let items: Vec<u32> = desc["v"]
                .as_array()?
                .iter()
                .map(|item| build(arena, item))
                .collect::<Option<_>>()?;
            let id = arena.push_array(&items)?;
            with_class(arena, id)?
        }
        "hash" => {
            let mut pairs = Vec::new();
            for pair in desc["v"].as_array()? {
                pairs.push((build(arena, &pair[0])?, build(arena, &pair[1])?));
            }
            let default = match desc.get("default") {
                Some(default) => Some(build(arena, default)?),
                None => None,
            };
            let id = arena.push_hash(&pairs, default)?;
            if desc["identity"].as_bool() == Some(true) {
                assert!(arena.set_user_class(id, b"Hash".to_vec()));
            }
            with_class(arena, id)?
        }
        "regexp" => {
            let source = hex_of(desc, "source");
            let options = desc["opts"].as_u64()? as u8;
            let id = match desc["enc"].as_str()? {
                "ASCII-8BIT" => arena.push_regexp(source, options),
                name => arena.push_regexp_with_encoding_name(source, options, name.as_bytes()),
            };
            with_class(arena, id)?
        }
        "class" => arena.push_class(hex_of(desc, "name"))?,
        "module" => arena.push_module(hex_of(desc, "name"), false)?,
        "struct" | "obj" => {
            let mut members = Vec::new();
            for pair in desc[if desc["t"] == "struct" { "members" } else { "ivars" }].as_array()? {
                members.push((from_hex(pair[0].as_str()?), build(arena, &pair[1])?));
            }
            let class = class?;
            let id = if desc["t"] == "struct" {
                arena.push_struct(class, &members)?
            } else {
                arena.push_object(class, &members)?
            };
            for module in desc["ext"].as_array().into_iter().flatten() {
                assert!(arena.push_extension(id, from_hex(module.as_str()?)));
            }
            if desc["t"] == "struct" {
                for pair in desc["ivars"].as_array().into_iter().flatten() {
                    let value = build(arena, &pair[1])?;
                    assert!(arena.push_extra_ivar(id, &from_hex(pair[0].as_str()?), value));
                }
            }
            id
        }
        "udump" => {
            if desc.get("time").is_some() {
                return None;
            }
            let bytes = hex_of(desc, "hex");
            let id = match desc["enc"].as_str()? {
                "ASCII-8BIT" => arena.push_bytes(bytes),
                name => arena.push_str_with_encoding_name(bytes, name.as_bytes()),
            };
            assert!(arena.set_user_defined(id, class?));
            id
        }
        "umarshal" => {
            let body = build(arena, &desc["v"])?;
            arena.push_user_marshal(class?, body)?
        }
        other => panic!("unknown description {other}"),
    })
}

/// The `push_*` API says what Ruby says: the same values built by hand come out as the same bytes - strings in
/// every encoding, symbols, floats with tails, bignums, structs, objects with extensions, `_dump` and
/// `marshal_dump` wrappers. (Values Ruby shares by object are skipped: a description does not say which are one.)
#[test]
fn values_built_with_the_api_dump_like_ruby() {
    let mut problems = Vec::new();
    let mut compared = 0;
    for set in sets() {
        let produced = set.produced_dir("built");
        for fixture in set.fixtures.iter().filter(|fixture| !is_many(fixture)) {
            let Some(desc) = &fixture.desc else { continue };
            let bytes = set.bytes(fixture);
            if has_shared_values(&load(&bytes).unwrap()) {
                continue;
            }
            let mut arena = Arena::builder();
            let Some(root) = build(&mut arena, desc) else { continue };
            arena.set_root(root);
            let built = dump(&arena);
            if is_ruby193_binary_symbol(set, fixture) || (is_untagged_class_set(set) && refers_to_non_ascii_class(desc))
            {
                if built == bytes {
                    problems.push(format!(
                        "[{}] {}: the known difference is gone; remove it",
                        set.label, fixture.name
                    ));
                }
                continue;
            }
            compared += 1;
            if built != bytes && !is_ruby187_hashed_ivar_order(set, fixture) {
                let at = built
                    .iter()
                    .zip(&bytes)
                    .position(|(a, b)| a != b)
                    .unwrap_or(built.len().min(bytes.len()));
                problems.push(format!(
                    "[{}] {}: built {} bytes, Ruby wrote {}, first difference at {at}",
                    set.label,
                    fixture.name,
                    built.len(),
                    bytes.len()
                ));
            }
            fs::write(produced.join(format!("{}.bin", fixture.name)), &built).unwrap();
        }
        problems.extend(set.verify_with_ruby(&produced));
    }
    report(&problems);
    assert!(compared == 0 || compared > 250, "only {compared} fixtures were built");
}

// ----------------------------------------------------------------------------------------------------------
// The mutating API against Ruby: change a loaded value, get what Ruby writes for the changed one
// ----------------------------------------------------------------------------------------------------------

fn element(arena: &Arena<'_>, index: usize) -> u32 {
    ValueRef::root(arena).at(index).unwrap().id()
}

/// The API calls that make the same change `generate.rb` makes in Ruby, by case name. `false`: refused.
fn apply_change(name: &str, arena: &mut Arena<'_>) -> bool {
    let root = arena.root();
    let text = |arena: &mut Arena<'_>, text: &str| arena.push_string(text.to_owned());
    match name {
        "array_value" => {
            let (seven, eight) = (arena.push_fixnum(7), arena.push_fixnum(8));
            let list = arena.push_array(&[seven, eight]).unwrap();
            arena.set_array_value(root, 1, list).is_some()
        }
        "array_string" => arena.set_array_string(root, 1, "ünï".to_owned()).is_some(),
        "array_bytes" => arena.set_array_bytes(root, 1, vec![0xff, 0]).is_some(),
        "array_symbol" => arena
            .set_array_symbol(root, 2, "シンボル".as_bytes().to_vec())
            .is_some(),
        "array_fixnum" => arena.set_array_fixnum(root, 0, -70_000).is_some(),
        "array_bool" => arena.set_array_bool(root, 4, true).is_some(),
        "array_link" => {
            let two = element(arena, 1);
            arena.set_array_value(root, 3, two).is_some()
        }
        "array_cycle" => arena.set_array_value(root, 5, root).is_some(),
        "array_to_symbol_seen" => arena.set_array_symbol(root, 0, b"three".to_vec()).is_some(),

        "hash_key" => {
            let key = arena.push_symbol(b"z".to_vec());
            arena.set_hash_key(root, 0, key).is_some()
        }
        "hash_value" => {
            let new = text(arena, "new");
            arena.set_hash_value(root, 1, new).is_some()
        }
        "hash_default" => {
            let new = text(arena, "other");
            arena.set_hash_default(root, new).is_some()
        }

        "member_value" | "struct_member_value" => {
            let (one, two) = (arena.push_fixnum(1), arena.push_fixnum(2));
            let hash = arena.push_hash(&[(one, two)], None).unwrap();
            arena
                .set_member_value(root, if name.starts_with("struct") { b"d" } else { b"@d" }, hash)
                .is_some()
        }
        "member_string" => arena.set_member_string(root, b"@b", "ß".to_owned()).is_some(),
        "member_bytes" => arena.set_member_bytes(root, b"@b", vec![0xff]).is_some(),
        "member_symbol" => arena
            .set_member_symbol(root, b"@c", "記号".as_bytes().to_vec())
            .is_some(),
        "member_fixnum" => arena.set_member_fixnum(root, b"@a", 99).is_some(),
        "member_bool" => arena.set_member_bool(root, b"@e", true).is_some(),
        "struct_member_string" => arena.set_member_string(root, b"b", "ß".to_owned()).is_some(),
        "struct_member_fixnum" => arena.set_member_fixnum(root, b"a", 99).is_some(),
        "struct_member_symbol" => arena
            .set_member_symbol(root, b"c", "記号".as_bytes().to_vec())
            .is_some(),

        "bytes_content" => arena.set_bytes_content(root, vec![0xff, 0xfe, 0xfd]).is_some(),
        "bytes_content_shared" => {
            let shared = element(arena, 0);
            arena.set_bytes_content(shared, b"cdef".to_vec()).is_some()
        }
        "string_text_shared" => {
            let shared = element(arena, 0);
            arena.set_string_text(shared, "änder".to_owned()).is_some()
        }
        "string_text_empty" => arena.set_string_text(root, String::new()).is_some(),
        "fixnum_value" => arena.set_fixnum_value(root, 70_000) == Some(5),
        "fixnum_negative" => arena.set_fixnum_value(root, -1) == Some(5),
        "take_bytes" => {
            let first = element(arena, 0);
            arena.take_bytes_content(first).as_deref() == Some(&b"ab"[..])
        }
        "take_regexp" => {
            let first = element(arena, 0);
            arena.take_regexp_source(first).as_deref() == Some(&b"abc"[..])
        }

        "user_class_string" => arena.set_user_class(root, "Ｓ文字列".as_bytes().to_vec()),
        "user_class_array" => arena.set_user_class(root, "Ａ配列".as_bytes().to_vec()),
        "user_class_hash" => arena.set_user_class(root, "Ｈハッシュ".as_bytes().to_vec()),
        "user_class_regexp" => arena.set_user_class(root, "Ｒ正規表現".as_bytes().to_vec()),
        "user_class_cleared" | "user_class_cleared_array" => arena.clear_user_class(root),
        "user_defined_binary" => arena.set_user_defined(root, b"DumpyBinary".to_vec()),
        "user_defined_sjis" => arena.set_user_defined(root, b"Dumpy".to_vec()),

        "extend_object" | "extend_string" => arena.push_extension(root, "Módulo".as_bytes().to_vec()),
        // Stream order: the module extended last comes first (Ruby extends with the last one listed first).
        "extend_twice" => {
            arena.push_extension(root, b"Extra2".to_vec()) && arena.push_extension(root, b"Extra".to_vec())
        }
        "extend_array" => arena.push_extension(root, b"Extra".to_vec()),

        "ivar_string" | "ivar_string_sjis" => {
            let note = text(arena, "メモ");
            arena.push_extra_ivar(root, b"@note", note)
        }
        "ivar_array" => {
            let label = text(arena, "ラベル");
            arena.push_extra_ivar(root, b"@label", label)
        }
        "ivar_hash" => {
            let label = arena.push_symbol(b"sym".to_vec());
            arena.push_extra_ivar(root, b"@label", label)
        }
        "ivar_object" => {
            let one = arena.push_fixnum(1);
            let list = arena.push_array(&[one]).unwrap();
            arena.push_extra_ivar(root, b"@z", list)
        }
        "ivar_two" => {
            let one = arena.push_fixnum(1);
            let x = text(arena, "x");
            arena.push_extra_ivar(root, b"@a", one) && arena.push_extra_ivar(root, b"@b", x)
        }
        "ivar_shared_value" => {
            let (value, target) = (element(arena, 0), element(arena, 1));
            arena.push_extra_ivar(target, b"@v", value)
        }
        "root_element" => {
            let second = element(arena, 1);
            arena.set_root(second)
        }
        other => panic!("no API calls for the change {other}"),
    }
}

/// Every change Ruby made in `generate.rb`, made through the API: the dump is what Ruby wrote for the result, Ruby
/// loads it and sees the same data, and it survives another round trip (and JSON).
#[test]
fn changes_made_with_the_api_match_what_ruby_writes() {
    let Some(set) = ruby_set() else { return };
    let produced = set.produced_dir("changes");
    let mut problems = Vec::new();
    let mut count = 0;
    for fixture in set
        .group("mutation")
        .filter(|fixture| fixture.name.ends_with("_before"))
    {
        let name = fixture
            .name
            .strip_prefix("mut_")
            .unwrap()
            .strip_suffix("_before")
            .unwrap();
        let after = set.named(&format!("mut_{name}_after"));
        let before = set.bytes(fixture);
        let mut arena = load(&before).unwrap();
        if !apply_change(name, &mut arena) {
            problems.push(format!("{name}: the change was refused"));
            continue;
        }
        let changed = dump(&arena);
        count += 1;
        fs::write(produced.join(format!("mut_{name}_after.bin")), &changed).unwrap();
        if changed != after {
            let at = changed
                .iter()
                .zip(&after)
                .position(|(a, b)| a != b)
                .unwrap_or(changed.len().min(after.len()));
            problems.push(format!(
                "{name}: Ruby wrote {} bytes, the changed arena dumps {} (first difference at {at})",
                after.len(),
                changed.len()
            ));
            continue;
        }
        if dump(&load(&changed).unwrap()) != changed {
            problems.push(format!("{name}: the changed value does not round-trip"));
        }
        if !has_shared_values(&arena) {
            if let Ok(back) = through_json(&arena) {
                if dump(&back) != changed {
                    problems.push(format!("{name}: the changed value does not survive JSON"));
                }
            }
        }
    }
    problems.extend(set.verify_with_ruby(&produced));
    report(&problems);
    assert!(count >= 50, "only {count} changes");
}

/// Changes the API refuses leave the value as it was.
#[test]
fn refused_changes_leave_the_value_alone() {
    let Some(set) = ruby_set() else { return };
    let check = |name: &str, refuse: &dyn Fn(&mut Arena<'_>, u32) -> bool| {
        let bytes = set.named(name);
        let mut arena = load(&bytes).unwrap();
        let root = arena.root();
        assert!(refuse(&mut arena, root), "{name}: the change was not refused");
        assert_eq!(dump(&arena), bytes, "{name}: a refused change changed the value");
    };

    check("mut_array_value_before", &|a, r| {
        let nil = element(a, 4);
        let fixnum = element(a, 0);
        a.set_array_value(r, 99, r).is_none() // past the end
            && a.set_array_value(r, 0, 9999).is_none() // not a node
            && a.set_array_string(fixnum, 0, "x".into()).is_none() // not an array
            && a.set_hash_key(r, 0, r).is_none()
            && a.set_hash_value(r, 0, r).is_none()
            && a.set_hash_default(r, r).is_none()
            && a.set_member_value(r, b"@a", r).is_none()
            && a.set_bytes_content(r, vec![1]).is_none()
            && a.set_string_text(r, "x".into()).is_none()
            && a.set_fixnum_value(r, 1).is_none()
            && a.take_bytes_content(r).is_none()
            && a.take_bignum_magnitude(r).is_none()
            && a.take_float_ascii(r).is_none()
            && a.take_regexp_source(r).is_none()
            && a.take_class_path(r).is_none()
            && a.take_module_path(r).is_none()
            && !a.set_user_class(r, vec![]) // no class
            && !a.set_user_defined(r, b"X".to_vec()) // an array is not a `_dump` value
            && !a.clear_user_class(r) // it has none
            && !a.push_extension(nil, b"M".to_vec()) // nil cannot be extended
            && !a.push_extension(fixnum, b"M".to_vec())
            && !a.push_extension(r, vec![]) // no module
            && !a.push_extra_ivar(nil, b"@x", r)
            && !a.push_extra_ivar(r, b"", r) // no name
            && !a.push_extra_ivar(r, b"@x", 9999) // no value
            && !a.set_root(9999)
            && !a.set_encoding(r, 66)
            && !a.set_encoding_name(r, b"UTF-8")
    });
    check("mut_hash_value_before", &|a, r| {
        a.set_array_value(r, 0, r).is_none()
            && a.set_hash_key(r, 99, r).is_none()
            && a.set_hash_value(r, 99, r).is_none()
    });
    check("basic_hash", &|a, r| a.set_hash_default(r, r).is_none()); // there is no default to change
    check("mut_member_value_before", &|a, r| {
        let one = a.push_fixnum(1);
        a.set_member_value(r, b"@missing", one).is_none() // never adds a member
            && a.set_member_string(r, b"@missing", "x".into()).is_none()
            && a.set_member_value(r, b"@a", 9999).is_none()
            && a.set_array_value(r, 0, one).is_none()
    });
    check("mut_bytes_content_before", &|a, r| {
        !a.set_user_class(r, vec![]) && a.set_hash_default(r, r).is_none() && !a.set_encoding(r, 200)
    });
    check("mut_fixnum_value_before", &|a, r| {
        a.set_bytes_content(r, vec![]).is_none()
            && a.set_string_text(r, String::new()).is_none()
            && !a.push_extension(r, b"M".to_vec())
    });
}

// ----------------------------------------------------------------------------------------------------------
// Core classes: what they are in the stream
// ----------------------------------------------------------------------------------------------------------

/// Time, Range, exceptions, Set, Rational, Complex, Encoding: loaded as the plain objects and wrappers they are in
/// Marshal's own format - nothing is interpreted - and kept exactly.
#[test]
fn core_classes_are_what_marshal_writes_for_them() {
    let Some(set) = ruby_set() else { return };

    let arena = load(&set.named("core_time_offset")).unwrap().into_owned();
    let time = ValueRef::root(&arena);
    assert!(time.is_user_defined());
    assert_eq!(time.class_name(), Some(&b"Time"[..]));
    assert_eq!(time.as_bytes().map(<[u8]>::len), Some(8));
    let ivars: Vec<_> = time.extra_ivars().map(|(name, value)| (name.to_vec(), value)).collect();
    let ivar = |name: &[u8]| ivars.iter().find(|(found, _)| found == name).map(|(_, value)| *value);
    assert_eq!(
        ivar(b"offset").and_then(|offset| offset.as_i64()),
        Some(32_400),
        "{} ivars",
        ivars.len()
    );
    assert!(ivar(b"zone").is_some_and(|zone| zone.is_nil()));
    assert!(ivar(b"nano_num").is_some() && ivar(b"nano_den").is_some() && ivar(b"submicro").is_some());

    let arena = load(&set.named("core_range_exclusive")).unwrap().into_owned();
    let range = ValueRef::root(&arena);
    assert_eq!((range.kind(), range.class_name()), (Kind::Object, Some(&b"Range"[..])));
    assert_eq!(range.get("begin").and_then(|v| v.as_i64()), Some(1));
    assert_eq!(range.get("end").and_then(|v| v.as_i64()), Some(5));
    assert_eq!(range.get("excl").and_then(|v| v.as_bool()), Some(true));
    let arena = load(&set.named("core_range_endless")).unwrap().into_owned();
    assert!(ValueRef::root(&arena).get("end").unwrap().is_nil());

    let arena = load(&set.named("core_exc_runtime")).unwrap().into_owned();
    let error = ValueRef::root(&arena);
    assert_eq!(error.class_name(), Some(&b"RuntimeError"[..]));
    assert_eq!(error.get("mesg").and_then(|v| v.as_str()), Some("メッセージ"));
    assert!(error.get("bt").unwrap().is_nil());
    let arena = load(&set.named("core_exc_custom")).unwrap().into_owned();
    assert_eq!(ValueRef::root(&arena).get("@code").and_then(|v| v.as_i64()), Some(7));

    let arena = load(&set.named("core_rational")).unwrap().into_owned();
    let rational = ValueRef::root(&arena).at(0).unwrap();
    assert!(rational.is_user_marshal());
    assert_eq!(rational.class_name(), Some(&b"Rational"[..]));
    let body = rational.body().unwrap();
    assert_eq!(
        (body.at(0).and_then(|v| v.as_i64()), body.at(1).and_then(|v| v.as_i64())),
        (Some(3), Some(4))
    );
    let arena = load(&set.named("core_complex")).unwrap().into_owned();
    assert_eq!(
        ValueRef::root(&arena).at(0).unwrap().class_name(),
        Some(&b"Complex"[..])
    );

    let arena = load(&set.named("core_set")).unwrap().into_owned();
    let hash = ValueRef::root(&arena).at(1).unwrap().get("@hash").unwrap();
    assert_eq!((hash.kind(), hash.len()), (Kind::Hash, 3));

    let arena = load(&set.named("core_encoding_objects")).unwrap().into_owned();
    let utf8 = ValueRef::root(&arena).at(0).unwrap();
    assert!(utf8.is_user_defined());
    assert_eq!(
        (utf8.class_name(), utf8.as_bytes()),
        (Some(&b"Encoding"[..]), Some(&b"UTF-8"[..]))
    );
    assert_eq!(utf8.encoding_name(), Some(&b"US-ASCII"[..]));

    // A hash that compares by identity is an ordinary hash with a `Hash` user class.
    let arena = load(&set.named("core_hash_identity")).unwrap().into_owned();
    let identity = ValueRef::root(&arena).at(0).unwrap();
    assert_eq!(
        (identity.kind(), identity.class_name()),
        (Kind::Hash, Some(&b"Hash"[..]))
    );
}

// ----------------------------------------------------------------------------------------------------------
// Damaged fixtures: nothing panics, and whatever still loads dumps to something that loads to the same dump
// ----------------------------------------------------------------------------------------------------------

/// `cargo fuzz` does not build on every machine; this walks the same ground from real streams: each small fixture
/// with bytes changed, inserted, removed or cut off, from a fixed seed. A stream that still loads must dump to bytes
/// that load and dump to themselves - which is where a disagreement between how `load` and `dump` number objects and
/// symbols would show.
/// Runs `accepts.rb` on the streams in `dir`: whether Ruby's `Marshal.load` reads each one whole, by file name.
fn ruby_accepts(set: &Set, dir: &Path) -> HashMap<String, bool> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let output = Command::new(&set.interpreter)
        .args(set.flags)
        .arg(tests.join("accepts.rb"))
        .arg(tests.join(set.classes))
        .arg(dir)
        .output()
        .expect("run the reader");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            if let Some(name) = line.strip_prefix("ACCEPT ") {
                Some((name.to_owned(), true))
            } else {
                let rest = line.strip_prefix("REJECT ")?;
                Some((rest.split(':').next()?.to_owned(), false))
            }
        })
        .collect()
}

/// Damaged streams. marshal-rs must never panic on them, and what it accepts must dump to a fixed point. Whether Ruby
/// accepts them is written to `damaged-report.txt` and not asserted: Ruby's leniency is not a specification.
#[test]
fn damaged_fixtures_never_panic_and_dump_to_a_fixed_point() {
    let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    let mut loaded = 0;
    let mut report = String::new();
    for set in sets() {
        let dir = set.produced_dir("damaged");
        // Every damaged stream written, by name, with whether marshal-rs's `load` accepted it.
        let mut verdicts: HashMap<String, bool> = HashMap::new();
        for fixture in set.fixtures.iter().filter(|fixture| !is_many(fixture)) {
            let original = set.bytes(fixture);
            if original.len() > 3000 {
                continue;
            }
            for n in 0..15 {
                let mut bytes = original.clone();
                for _ in 0..1 + next() % 3 {
                    let at = (next() % bytes.len() as u64) as usize;
                    match next() % 5 {
                        0 => bytes[at] = next() as u8,
                        1 => bytes[at] ^= 1 << (next() % 8),
                        2 => bytes.insert(at, next() as u8),
                        3 => drop(bytes.remove(at)),
                        _ => bytes.truncate(at.max(2)),
                    }
                    if bytes.len() < 3 {
                        break;
                    }
                }
                let name = format!("{}-{n}.bin", fixture.name);
                fs::write(dir.join(&name), &bytes).unwrap();
                let accepted = match load(&bytes) {
                    Ok(arena) => {
                        loaded += 1;
                        let once = dump(&arena);
                        let again = load(&once).unwrap_or_else(|error| {
                            panic!(
                                "[{}] {}: what was dumped does not load ({error}): {once:?}",
                                set.label, fixture.name
                            )
                        });
                        assert_eq!(
                            dump(&again),
                            once,
                            "[{}] {}: not a fixed point; input {bytes:?}",
                            set.label,
                            fixture.name
                        );
                        true
                    }
                    Err(_) => false,
                };
                verdicts.insert(name, accepted);
            }
        }

        let ruby = ruby_accepts(set, &dir);
        let verdict = |accepted: bool| if accepted { "accepts" } else { "refuses" };
        let mut disagreements = 0;
        for (name, accepted) in &verdicts {
            let ruby_said = ruby.get(name).copied();
            if ruby_said != Some(*accepted) {
                disagreements += 1;
                let ruby_word = ruby_said.map_or("reports nothing", verdict);
                report.push_str(&format!(
                    "[{}] {name}: marshal-rs {}, Ruby {ruby_word}\n",
                    set.label,
                    verdict(*accepted)
                ));
            }
        }
        report.push_str(&format!(
            "[{}] {disagreements} of {} damaged streams differ from Ruby\n",
            set.label,
            verdicts.len()
        ));
    }
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("damaged-report.txt");
    fs::write(&path, &report).unwrap();
    eprintln!("damaged streams: Ruby agreement report written to {}", path.display());
    assert!(
        loaded > 0 || sets().is_empty(),
        "no damaged stream loaded; the mutations are not reaching the loader"
    );
}
