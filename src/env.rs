//! Env vars: the process first, then the file given with `--env`

use std::{
    collections::HashMap,
    env::VarError,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::OnceLock,
};

use color_eyre::{Result, eyre::eyre};

/// Values from the `--env` file, set once by `load` (no `set_var`: unsafe with threads running)
static FILE: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Every setting besides the `TRIFORCE_CRON_<loop>` ones (`schedules::LOOPS`)
const SETTINGS: &[&str] = &[
    "TRIFORCE_SOURCE_LEAGUES",
    "TRIFORCE_SOURCE_TEAMS",
    "TRIFORCE_V2",
    "TRIFORCE_V2_DETAILS",
    "TRIFORCE_V2_LIVE",
    "TRIFORCE_LOCALES",
    "TRIFORCE_GQL_MANIFEST",
];

fn is_known(name: &str) -> bool {
    SETTINGS.contains(&name)
        || name
            .strip_prefix("TRIFORCE_CRON_")
            .is_some_and(|l| crate::schedules::LOOPS.iter().any(|(n, _)| *n == l))
}

#[derive(Debug, Default, PartialEq)]
pub struct Cli {
    pub env_file: Option<PathBuf>,
    pub refresh_manifest: bool,
}

/// `[--env <file>] [refresh-manifest]`, in any order
pub fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Cli> {
    let mut cli = Cli::default();
    let mut args = args.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--env" => {
                let file = args.next().ok_or_else(|| eyre!("--env needs a file"))?;
                if file == "refresh-manifest" || file.starts_with("--") {
                    return Err(eyre!("--env needs a file, got {file:?}"));
                }
                if cli.env_file.replace(file.into()).is_some() {
                    return Err(eyre!("--env given twice"));
                }
            }
            "refresh-manifest" => cli.refresh_manifest = true,
            other => {
                return Err(eyre!(
                    "unknown argument {other:?}, expected [--env <file>] [refresh-manifest]"
                ));
            }
        }
    }
    Ok(cli)
}

/// `KEY=value` lines. A bad line is shown by the key before it, never by its text: it could
/// hold a secret
fn parse(text: impl std::io::Read, what: &str) -> Result<HashMap<String, String>> {
    let mut vars = HashMap::new();
    let mut last: Option<String> = None;
    for item in dotenvy::from_read_iter(text) {
        match item {
            Ok((k, v)) => {
                last = Some(k.clone());
                vars.insert(k, v);
            }
            Err(dotenvy::Error::LineParse(_, at)) => {
                let after = last.map_or("at the start".to_string(), |k| format!("after {k}"));
                return Err(eyre!("{what}: bad line {after} (char {at})"));
            }
            Err(e) => return Err(eyre!("{what}: {e}")),
        }
    }
    Ok(vars)
}

/// `None` when the file doesn't exist
fn read_file(path: &Path) -> Result<Option<HashMap<String, String>>> {
    match std::fs::File::open(path) {
        Ok(f) => parse(f, &path.display().to_string()).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(eyre!("{}: {e}", path.display())),
    }
}

/// Reads the `--env` file. It must exist: without `--env` nothing is read
pub fn load(path: &Path) -> Result<()> {
    let shown = path.display();
    // a file named on the command line must be there: a typo would run on the defaults
    let vars = read_file(path)?.ok_or_else(|| eyre!("env file {shown} not found"))?;
    crate::client::log(format!("env file {shown}: {} vars", vars.len()));
    let mut unknown: Vec<&str> = vars
        .keys()
        .map(String::as_str)
        .filter(|k| k.starts_with("TRIFORCE_") && !is_known(k))
        .collect();
    unknown.sort_unstable();
    for k in unknown {
        crate::client::log(format!("env file {shown}: unknown var {k}, ignored"));
    }
    FILE.set(vars).map_err(|_| eyre!("env file already loaded"))
}

#[allow(clippy::disallowed_methods)] // the one place that reads the process env
fn lookup(file: Option<&HashMap<String, String>>, name: &str) -> Result<String, VarError> {
    match std::env::var(name) {
        Err(VarError::NotPresent) => file
            .and_then(|f| f.get(name).cloned())
            .ok_or(VarError::NotPresent),
        other => other,
    }
}

/// Like `std::env::var`, falling back to the `--env` file
/// An on/off setting like `TRIFORCE_V2_DETAILS`: `on` (default when unset) or `off`
pub fn parse_switch(name: &str, value: Option<&str>) -> Result<bool> {
    match value.map(|v| v.trim().to_ascii_lowercase()).as_deref() {
        None | Some("on") => Ok(true),
        Some("off") => Ok(false),
        Some(_) => Err(eyre!("{name} must be on or off, got {value:?}")),
    }
}

/// An on/off setting from the environment (`parse_switch`)
pub fn switch(name: &str) -> Result<bool> {
    match var(name) {
        Err(VarError::NotPresent) => parse_switch(name, None),
        Err(e) => Err(eyre!("{name}: {e}")),
        Ok(v) => parse_switch(name, Some(&v)),
    }
}

pub fn var(name: &str) -> Result<String, VarError> {
    lookup(FILE.get(), name)
}

/// Like `std::env::var_os`, falling back to the `--env` file
#[allow(clippy::disallowed_methods)] // the one place that reads the process env
pub fn var_os(name: &str) -> Option<OsString> {
    std::env::var_os(name).or_else(|| FILE.get()?.get(name).map(OsString::from))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch() {
        assert!(parse_switch("TRIFORCE_V2_DETAILS", None).unwrap());
        assert!(!parse_switch("TRIFORCE_V2_DETAILS", Some(" OFF ")).unwrap());
        assert!(parse_switch("TRIFORCE_V2_DETAILS", Some("on")).unwrap());
        assert!(parse_switch("TRIFORCE_V2_DETAILS", Some("yes")).is_err());
    }

    fn p(args: &[&str]) -> Result<Cli> {
        parse_args(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn args() {
        assert_eq!(p(&[]).unwrap(), Cli::default());
        assert_eq!(
            p(&["--env", "env/dev.env"]).unwrap().env_file,
            Some(PathBuf::from("env/dev.env"))
        );
        let cli = p(&["--env", "x.env", "refresh-manifest"]).unwrap();
        assert!(cli.refresh_manifest && cli.env_file.is_some());
        assert!(
            p(&["refresh-manifest", "--env", "x.env"])
                .unwrap()
                .refresh_manifest
        );
        assert!(p(&["--env"]).is_err());
        assert!(p(&["--env", "refresh-manifest"]).is_err());
        assert!(p(&["--env", "--env"]).is_err());
        assert!(p(&["--env", "a.env", "--env", "b.env"]).is_err());
        assert!(p(&["foo"]).is_err());
    }

    #[test]
    fn file_values_and_the_process_wins() {
        let vars = parse(
            "# comment\nTRIFORCE_TEST_ONLY_A=from-file\nPATH=nope\n".as_bytes(),
            "t",
        )
        .unwrap();
        assert_eq!(
            lookup(Some(&vars), "TRIFORCE_TEST_ONLY_A").unwrap(),
            "from-file"
        );
        // PATH is set in the process: the file doesn't change it
        assert_ne!(lookup(Some(&vars), "PATH").unwrap(), "nope");
        assert_eq!(
            lookup(Some(&vars), "TRIFORCE_TEST_ONLY_B"),
            Err(VarError::NotPresent)
        );
        // no file loaded: only the process
        assert_eq!(
            lookup(None, "TRIFORCE_TEST_ONLY_A"),
            Err(VarError::NotPresent)
        );
        assert!(
            read_file(Path::new("env/does-not-exist.env"))
                .unwrap()
                .is_none()
        );
        let e = load(Path::new("env/does-not-exist.env")).unwrap_err();
        assert!(e.to_string().contains("not found"), "{e}");
    }

    #[test]
    fn bad_line_is_an_error_without_its_text() {
        let e = parse("SECRET_LINE oops\n".as_bytes(), "bad.env").unwrap_err();
        let msg = format!("{e:#}");
        assert!(msg.starts_with("bad.env: bad line at the start"), "{msg}");
        assert!(!msg.contains("SECRET_LINE"), "{msg}");
        let e = parse("A=1\nB=2\nSECRET_LINE oops\n".as_bytes(), "bad.env").unwrap_err();
        assert!(format!("{e:#}").starts_with("bad.env: bad line after B"));
    }

    #[test]
    fn known_names() {
        assert!(is_known("TRIFORCE_V2"));
        assert!(is_known("TRIFORCE_CRON_LIVE"));
        assert!(!is_known("TRIFORCE_CRON_NOPE"));
        assert!(!is_known("TRIFORCE_LOCALE"));
    }

    #[test]
    fn example_file_has_the_defaults() {
        use crate::{
            client::{Source, gql::manifest::DEFAULT_PATH},
            service::v2::{parse_locales, parse_v2},
        };
        // every commented TRIFORCE_ line, uncommented
        let vars: String = include_str!("../env/example.env")
            .lines()
            .filter_map(|l| l.strip_prefix("#TRIFORCE_"))
            .map(|l| format!("TRIFORCE_{l}\n"))
            .collect();
        let read = parse(vars.as_bytes(), "example").unwrap();
        // every setting is in the example, and nothing else
        let mut keys: Vec<&str> = read.keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut known: Vec<String> = SETTINGS.iter().map(|s| s.to_string()).collect();
        known.extend(
            crate::schedules::LOOPS
                .iter()
                .map(|(n, _)| format!("TRIFORCE_CRON_{n}")),
        );
        known.sort_unstable();
        assert_eq!(keys, known);
        for v in ["TRIFORCE_SOURCE_LEAGUES", "TRIFORCE_SOURCE_TEAMS"] {
            assert_eq!(Source::parse(&read[v]), Some(Source::default()), "{v}");
        }
        assert_eq!(
            parse_v2(Some(&read["TRIFORCE_V2"])).unwrap(),
            parse_v2(None).unwrap()
        );
        assert_eq!(
            parse_locales(Some(&read["TRIFORCE_LOCALES"])),
            parse_locales(None)
        );
        assert_eq!(read["TRIFORCE_GQL_MANIFEST"], DEFAULT_PATH);
        assert!(parse_switch("TRIFORCE_V2_LIVE", Some(&read["TRIFORCE_V2_LIVE"])).unwrap());
        assert_eq!(
            parse_switch("TRIFORCE_V2_DETAILS", Some(&read["TRIFORCE_V2_DETAILS"])).unwrap(),
            parse_switch("TRIFORCE_V2_DETAILS", None).unwrap()
        );
        for (name, default) in crate::schedules::LOOPS {
            assert_eq!(read[&format!("TRIFORCE_CRON_{name}")], *default, "{name}");
        }
    }
}
