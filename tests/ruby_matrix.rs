//! Which Ruby reads which Ruby's data. Every interpreter in `RUBIES` (a list separated by `:`, or `;` on Windows) writes the values in
//! `tests/fixtures/matrix.rb`. marshal-rs must dump each of those files back to the same bytes. Then every
//! interpreter must read the marshal-rs output of its own version and of every version below it, and see the same
//! data as it sees in the original. Opt-in: without `RUBIES` the test prints a note and passes.
//!
//! Ruby's own bugs are listed in `KNOWN_FAILURES`. Such a reader fails on the original too, and it must fail on the
//! marshal-rs output as well. An entry whose failure no longer happens is stale and fails the test.
#![cfg(all(feature = "alloc", not(miri)))]

use marshal_rs::{dump, load_many};
use std::{
    collections::BTreeMap,
    fmt, fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Version(u32, u32, u32);

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

struct Interpreter {
    path: String,
    version: Version,
}

/// The version from `ruby -v`, which starts `ruby 1.9.3p551 ...` or `ruby 4.0.9 (...)`.
fn version_of(path: &str) -> Option<Version> {
    let output = Command::new(path).arg("-v").output().ok()?;
    let text = String::from_utf8_lossy(&output.stdout);
    let number: String = text
        .strip_prefix("ruby ")?
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let mut parts = number.split('.').map(|part| part.parse::<u32>().ok());
    Some(Version(parts.next()??, parts.next()??, parts.next()??))
}

/// The separator of `RUBIES`: `:` as in PATH, except on Windows, where a path can have a drive letter `:` in it.
const RUBIES_SEPARATOR: char = if cfg!(windows) { ';' } else { ':' };

fn interpreters() -> Vec<Interpreter> {
    let Ok(list) = std::env::var("RUBIES") else {
        return Vec::new();
    };
    let mut rubies: Vec<Interpreter> = list
        .split(RUBIES_SEPARATOR)
        .filter(|path| !path.is_empty())
        .map(|path| Interpreter {
            path: path.to_owned(),
            version: version_of(path).unwrap_or_else(|| panic!("{path} is not a Ruby interpreter")),
        })
        .collect();
    rubies.sort_by_key(|ruby| ruby.version);
    rubies
}

/// Ruby's own bugs: (reader, writer, file, why). The reader cannot load `file` as `writer` wrote it, original or not.
const KNOWN_FAILURES: &[(&str, &str, &str, &str)] = &[
    (
        "1.9.3",
        "1.9.3",
        "symbol_utf8.bin",
        "1.9.3 writes this binary symbol without an encoding, and reads an unencoded symbol as US-ASCII, which raises \
         EncodingError for non-ASCII bytes (2.7.8 and later read it back as binary)",
    ),
    (
        "1.9.3",
        "1.8.7",
        "symbol_utf8.bin",
        "1.8.7 writes a non-ASCII symbol without an encoding, which 1.9.3 reads as US-ASCII and rejects with \
         EncodingError",
    ),
];

/// Dumps that differ from the writer's original although the same structures are dumped differently by other versions,
/// so the difference is Ruby's form, not marshal-rs's: (writer, file, why). An entry whose dump matches again is stale.
const KNOWN_DUMP_DIFFERENCES: &[(&str, &str, &str)] = &[(
    "1.9.3",
    "symbol_utf8.bin",
    "the same literal is a binary symbol on 1.9.3 and a UTF-8 one on 2.7.8 and later. 1.9.3 writes the binary symbol \
     with an empty instance-variable wrapper, 2.7.8 and later write it bare, and 1.8.7 writes it bare. marshal-rs \
     writes it bare, as 2.7.8 and later do",
)];

fn known_dump_difference(writer: Version, file: &str) -> Option<&'static str> {
    KNOWN_DUMP_DIFFERENCES
        .iter()
        .find(|&&(w, f, _)| w == writer.to_string() && f == file)
        .map(|&(.., why)| why)
}

fn known_reason(reader: Version, writer: Version, file: &str) -> Option<&'static str> {
    KNOWN_FAILURES
        .iter()
        .find(|&&(r, w, f, _)| r == reader.to_string() && w == writer.to_string() && f == file)
        .map(|&(.., why)| why)
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

/// Runs `script` with `ruby` on `dir`, returning its stdout, or its stderr if it failed.
fn run(ruby: &Interpreter, script: &Path, dir: &Path) -> Result<String, String> {
    let output = Command::new(&ruby.path)
        .arg(script)
        .arg(dir)
        .output()
        .expect("run the Ruby interpreter");
    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).into_owned())
    }
}

/// What a reader made of one stream of one file.
#[derive(Debug, PartialEq)]
enum Outcome {
    Described(String),
    Unmodeled,
    Failed(String),
}

/// Runs `matrix_read.rb` on `dir` with `reader`. Keys are `file#stream`.
fn read_all(reader: &Interpreter, dir: &Path) -> Result<BTreeMap<String, Outcome>, String> {
    let stdout = run(reader, &fixture("matrix_read.rb"), dir)?;
    let mut outcomes = BTreeMap::new();
    let mut finished = false;
    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("DESC ") {
            let (key, desc) = rest.split_once(' ').ok_or_else(|| format!("bad line: {line}"))?;
            outcomes.insert(key.to_owned(), Outcome::Described(desc.to_owned()));
        } else if let Some(key) = line.strip_prefix("UNMODELED ") {
            outcomes.insert(key.to_owned(), Outcome::Unmodeled);
        } else if let Some(rest) = line.strip_prefix("FAIL ") {
            let (key, message) = rest.split_once(": ").unwrap_or((rest, ""));
            outcomes.insert(key.to_owned(), Outcome::Failed(message.to_owned()));
        } else if line.starts_with("loaded ") {
            finished = true;
        }
    }
    if finished {
        Ok(outcomes)
    } else {
        Err(format!("matrix_read.rb did not finish on {}", dir.display()))
    }
}

fn file_of(key: &str) -> &str {
    key.split('#').next().unwrap_or(key)
}

#[test]
fn every_ruby_reads_its_own_data_and_the_data_of_the_versions_below_it() {
    let rubies = interpreters();
    if rubies.is_empty() {
        // The CI job that builds the Rubies sets this, so a failed build fails the job instead of passing with nothing tested.
        assert!(
            std::env::var_os("MARSHAL_REQUIRE_RUBIES").is_none(),
            "MARSHAL_REQUIRE_RUBIES is set, but RUBIES names no interpreter"
        );
        eprintln!(
            "the Ruby version matrix is skipped: set RUBIES to a list of interpreters, separated by \
             `{RUBIES_SEPARATOR}`"
        );
        return;
    }
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("ruby-matrix");
    let mut problems = Vec::new();

    // Each version writes the matrix. marshal-rs must dump every file back to the same bytes; the redump goes to
    // `marshal/`, and the readers below read it there.
    for ruby in &rubies {
        let dir = root.join(ruby.version.to_string());
        let _ = fs::remove_dir_all(&dir);
        let original = dir.join("original");
        let produced = dir.join("marshal");
        fs::create_dir_all(&original).unwrap();
        fs::create_dir_all(&produced).unwrap();
        if let Err(stderr) = run(ruby, &fixture("matrix.rb"), &original) {
            problems.push(format!("{} cannot write the matrix: {stderr}", ruby.version));
            continue;
        }

        let mut files: Vec<PathBuf> = fs::read_dir(&original)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        files.sort();
        for path in files {
            let bytes = fs::read(&path).unwrap();
            let redump: Vec<u8> = match load_many(&bytes) {
                Ok(arenas) => arenas.iter().flat_map(|arena| dump(arena)).collect(),
                Err(error) => {
                    problems.push(format!(
                        "{} {}: marshal-rs cannot load it: {error}",
                        ruby.version,
                        path.display()
                    ));
                    continue;
                }
            };
            let file = path.file_name().unwrap().to_str().unwrap();
            match (redump != bytes, known_dump_difference(ruby.version, file)) {
                (true, None) => problems.push(format!(
                    "{} {}: marshal-rs dumps it as {} bytes, Ruby wrote {}",
                    ruby.version,
                    path.display(),
                    redump.len(),
                    bytes.len()
                )),
                (false, Some(_)) => problems.push(format!(
                    "{} {file}: the known dump difference no longer happens; remove it from KNOWN_DUMP_DIFFERENCES",
                    ruby.version
                )),
                _ => {}
            }
            fs::write(produced.join(path.file_name().unwrap()), &redump).unwrap();
        }
    }

    // A reader must read every stream of its own version's and the lower versions' marshal-rs output, with the same
    // data as its reading of the original.
    for reader in &rubies {
        for writer in rubies.iter().filter(|writer| writer.version <= reader.version) {
            let label = format!("{} reads {}'s data", reader.version, writer.version);
            let written = root.join(writer.version.to_string());
            let original = match read_all(reader, &written.join("original")) {
                Ok(outcomes) => outcomes,
                Err(error) => {
                    problems.push(format!("{label}: {error}"));
                    continue;
                }
            };
            let marshal = match read_all(reader, &written.join("marshal")) {
                Ok(outcomes) => outcomes,
                Err(error) => {
                    problems.push(format!("{label}: {error}"));
                    continue;
                }
            };

            // A known failure has to happen on the original, or it is stale.
            for &(_, _, file, _) in KNOWN_FAILURES
                .iter()
                .filter(|&&(r, w, _, _)| r == reader.version.to_string() && w == writer.version.to_string())
            {
                let fails = original
                    .iter()
                    .any(|(key, outcome)| file_of(key) == file && matches!(outcome, Outcome::Failed(_)));
                if !fails {
                    problems.push(format!(
                        "{label}: the known failure of {file} no longer happens; remove it"
                    ));
                }
            }

            for (key, original_outcome) in &original {
                let Some(marshal_outcome) = marshal.get(key) else {
                    problems.push(format!("{label}: {key} is missing from the marshal-rs output"));
                    continue;
                };
                match (original_outcome, marshal_outcome) {
                    (Outcome::Failed(_), Outcome::Failed(message)) => {
                        if known_reason(reader.version, writer.version, file_of(key)).is_none() {
                            problems.push(format!("{label}: {key} does not load, as Ruby's original: {message}"));
                        }
                    }
                    (_, Outcome::Failed(message)) => problems.push(format!(
                        "{label}: {key} does not load from the marshal-rs output, though the original loads: {message}"
                    )),
                    // Ruby cannot read its own original; marshal-rs's output is the same bytes, so this is Ruby's bug.
                    (Outcome::Failed(_), _) => {}
                    (loaded, redumped) => {
                        if loaded != redumped {
                            problems.push(format!(
                                "{label}: {key} reads different data from the marshal-rs output"
                            ));
                        }
                    }
                }
            }
        }
    }

    assert!(problems.is_empty(), "{}", problems.join("\n"));
}
