//! The site's persisted query manifest: operation name → hash (and query text)

use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    str::Chars,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::sync::{Mutex, RwLock};

use super::{GqlError, SITE_URL};
use crate::client::log;

pub const MARKER: &str = "apollo-persisted-query-manifest";
const JSON_PARSE: &str = "JSON.parse('";
const CHUNKS: &str = "static/chunks/";
const APP_VERSION: &str = "getAppVersion\",0,function(){return\"";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub client_version: String,
    pub fetched_at: DateTime<Utc>,
    pub operations: BTreeMap<String, ManifestOp>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ManifestOp {
    pub id: String,
    pub body: String,
}

#[derive(Deserialize)]
struct SiteManifest {
    operations: Vec<SiteOp>,
}

#[derive(Deserialize)]
struct SiteOp {
    id: String,
    name: String,
    body: String,
}

impl Manifest {
    /// From the site chunk that has the manifest. Every `id` must be the sha256 of its `body`
    pub fn from_chunk(
        js: &str,
        client_version: String,
        fetched_at: DateTime<Utc>,
    ) -> Result<Self, GqlError> {
        let literal = json_parse_literal(js)
            .ok_or_else(|| GqlError::Manifest("no JSON.parse('…') before the marker".into()))?;
        let site: SiteManifest = serde_json::from_str(&js_unescape(literal)?)
            .map_err(|e| GqlError::Manifest(format!("site manifest JSON: {e}")))?;
        let bad: Vec<&str> = site
            .operations
            .iter()
            .filter(|o| sha256_hex(&o.body) != o.id)
            .map(|o| o.name.as_str())
            .collect();
        if !bad.is_empty() {
            return Err(GqlError::Manifest(format!(
                "hash is not sha256(body) for {}",
                bad.join(", ")
            )));
        }
        let operations = site
            .operations
            .into_iter()
            .map(|o| {
                (
                    o.name,
                    ManifestOp {
                        id: o.id,
                        body: o.body,
                    },
                )
            })
            .collect();
        Ok(Self {
            client_version,
            fetched_at,
            operations,
        })
    }

    pub fn load(path: &Path) -> Result<Self, GqlError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| GqlError::Manifest(format!("can't read {}: {e}", path.display())))?;
        serde_json::from_str(&text)
            .map_err(|e| GqlError::Manifest(format!("can't parse {}: {e}", path.display())))
    }

    /// Writes `<path>.tmp` and renames it, so a crash never leaves half a file
    pub fn save(&self, path: &Path) -> Result<(), GqlError> {
        let mut tmp = OsString::from(path.as_os_str());
        tmp.push(".tmp");
        let tmp = PathBuf::from(tmp);
        let io =
            |e: std::io::Error| GqlError::Manifest(format!("can't write {}: {e}", path.display()));
        let text = serde_json::to_string_pretty(self).map_err(GqlError::Parse)?;
        std::fs::write(&tmp, text).map_err(io)?;
        std::fs::rename(&tmp, path).map_err(io)
    }

    /// Operations in `used` whose query text isn't the same in `newer` (or missing in one of them)
    pub fn changed_ops(&self, newer: &Manifest, used: &[&str]) -> Vec<String> {
        used.iter()
            .filter(|name| {
                self.operations.get(**name).map(|o| &o.body)
                    != newer.operations.get(**name).map(|o| &o.body)
            })
            .map(|name| name.to_string())
            .collect()
    }
}

pub fn sha256_hex(s: &str) -> String {
    Sha256::digest(s.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The raw text of the `JSON.parse('…')` string that holds the manifest
pub fn json_parse_literal(js: &str) -> Option<&str> {
    let marker = js.find(MARKER)?;
    let start = js[..marker].rfind(JSON_PARSE)? + JSON_PARSE.len();
    let bytes = js.as_bytes();
    let mut i = start;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => i += 2,
            b'\'' => return Some(&js[start..i]),
            _ => i += 1,
        }
    }
    None
}

fn hex(chars: &mut Chars, n: usize) -> Result<u32, GqlError> {
    let s: String = chars.by_ref().take(n).collect();
    if s.len() != n {
        return Err(GqlError::Manifest(format!("short escape \\{s}")));
    }
    u32::from_str_radix(&s, 16).map_err(|_| GqlError::Manifest(format!("bad escape \\{s}")))
}

/// Decodes a JS string literal body (`\'`, `\\`, `\n`, `\uXXXX`, `\xXX`…)
pub fn js_unescape(lit: &str) -> Result<String, GqlError> {
    let bad = |m: &str| GqlError::Manifest(m.to_string());
    let mut out = String::with_capacity(lit.len());
    let mut chars = lit.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('b') => out.push('\u{8}'),
            Some('f') => out.push('\u{c}'),
            Some('v') => out.push('\u{b}'),
            Some('0') => out.push('\0'),
            Some('x') => {
                let code = hex(&mut chars, 2)?;
                out.push(char::from_u32(code).ok_or_else(|| bad("bad \\x escape"))?);
            }
            Some('u') => {
                let hi = hex(&mut chars, 4)?;
                let code = if (0xD800..0xDC00).contains(&hi) {
                    if chars.next() != Some('\\') || chars.next() != Some('u') {
                        return Err(bad("lone surrogate"));
                    }
                    let lo = hex(&mut chars, 4)?;
                    if !(0xDC00..0xE000).contains(&lo) {
                        return Err(bad("bad surrogate pair"));
                    }
                    0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                } else {
                    hi
                };
                out.push(char::from_u32(code).ok_or_else(|| bad("bad \\u escape"))?);
            }
            Some('\n') => {}
            Some(other) => out.push(other),
            None => return Err(bad("string ends with \\")),
        }
    }
    Ok(out)
}

/// `static/chunks/<name>.js` paths in a page or chunk, no duplicates, in order
pub fn chunk_paths(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find(CHUNKS) {
        let after = &rest[pos + CHUNKS.len()..];
        let len = after
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
            .count();
        if len > 0 && after[len..].starts_with(".js") {
            let path = format!("{CHUNKS}{}.js", &after[..len]);
            if !out.contains(&path) {
                out.push(path);
            }
        }
        rest = &after[len..];
    }
    out
}

/// The site build (`apollographql-client-version`), e.g. `a93ba50`
pub fn find_client_version(text: &str) -> Option<String> {
    let start = text.find(APP_VERSION)? + APP_VERSION.len();
    let version: String = text[start..]
        .chars()
        .take_while(|c| c.is_ascii_hexdigit())
        .collect();
    (!version.is_empty() && text[start + version.len()..].starts_with('"')).then_some(version)
}

pub const REFRESH_EVERY: Duration = Duration::from_secs(3600);
pub const MAX_DOWNLOADS: usize = 200;
pub const DEFAULT_PATH: &str = "gql-manifest.json";

pub fn path_from_env() -> PathBuf {
    std::env::var_os("TRIFORCE_GQL_MANIFEST")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
}

/// Failed attempts count too, so a broken site isn't hit on every call
pub fn refresh_due(last: Option<Instant>, now: Instant) -> bool {
    last.is_none_or(|t| now.duration_since(t) >= REFRESH_EVERY)
}

async fn fetch_text(http: &reqwest::Client, url: &str) -> Result<String, GqlError> {
    let response = http
        .get(url)
        .header(reqwest::header::REFERER, format!("{SITE_URL}/"))
        .send()
        .await
        .map_err(GqlError::Network)?;
    let status = response.status();
    if !status.is_success() {
        return Err(GqlError::Http {
            status: status.as_u16(),
        });
    }
    response.text().await.map_err(GqlError::Network)
}

/// Same steps as the investigation: the page, its chunks, the chunks they load
pub async fn fetch_from_site(
    http: &reqwest::Client,
    previous_version: Option<&str>,
) -> Result<Manifest, GqlError> {
    let page = fetch_text(http, &format!("{SITE_URL}/en-US/")).await?;
    let mut version = find_client_version(&page);
    let mut queue = chunk_paths(&page);
    let mut seen: std::collections::HashSet<String> = queue.iter().cloned().collect();
    let mut downloads = 0;
    while !queue.is_empty() {
        let mut next = Vec::new();
        for path in queue {
            if downloads >= MAX_DOWNLOADS {
                return Err(GqlError::Manifest(format!(
                    "not found in {MAX_DOWNLOADS} chunks"
                )));
            }
            downloads += 1;
            let js = match fetch_text(http, &format!("{SITE_URL}/_next/{path}")).await {
                Ok(js) => js,
                Err(e) => {
                    log(format!("GQL_MANIFEST: can't download {path}: {e}"));
                    continue;
                }
            };
            if version.is_none() {
                version = find_client_version(&js);
            }
            if js.contains(MARKER) {
                let client_version = match version {
                    Some(v) => v,
                    None => {
                        let keep = previous_version.unwrap_or("unknown").to_string();
                        log(format!(
                            "GQL_MANIFEST: client version not found, keeping {keep}"
                        ));
                        keep
                    }
                };
                return Manifest::from_chunk(&js, client_version, Utc::now());
            }
            for p in chunk_paths(&js) {
                if seen.insert(p.clone()) {
                    next.push(p);
                }
            }
        }
        // the manifest chunk is loaded by a chunk of the page, 2 levels are enough today
        queue = next;
    }
    Err(GqlError::Manifest(format!(
        "no chunk with {MARKER} ({downloads} downloaded)"
    )))
}

/// The manifest in memory and on disk, with the refresh rules
#[derive(Debug)]
pub struct Store {
    path: PathBuf,
    current: RwLock<Option<Manifest>>,
    // held during a refresh, so only one runs at a time
    last_refresh: Mutex<Option<Instant>>,
}

impl Store {
    /// A missing or broken file isn't an error here, the first call refreshes it
    pub fn open(path: PathBuf) -> Self {
        let current = match Manifest::load(&path) {
            Ok(m) => Some(m),
            Err(e) => {
                log(format!("GQL_MANIFEST: {} not loaded ({e})", path.display()));
                None
            }
        };
        Self {
            path,
            current: RwLock::new(current),
            last_refresh: Mutex::new(None),
        }
    }

    /// (hash, client version) for `op`
    pub async fn lookup(&self, op: &'static str) -> Result<(String, String), GqlError> {
        let current = self.current.read().await;
        let manifest = current.as_ref().ok_or(GqlError::UnknownHash { op })?;
        let found = manifest
            .operations
            .get(op)
            .ok_or(GqlError::UnknownHash { op })?;
        Ok((found.id.clone(), manifest.client_version.clone()))
    }

    /// `Ok(false)` if the last try was less than an hour ago
    pub async fn refresh_if_due(&self, http: &reqwest::Client) -> Result<bool, GqlError> {
        let mut last = self.last_refresh.lock().await;
        if !refresh_due(*last, Instant::now()) {
            return Ok(false);
        }
        *last = Some(Instant::now());
        self.refresh_now(http).await.map(|_| true)
    }

    /// Manual refresh (`refresh-manifest`), no time limit
    pub async fn refresh(&self, http: &reqwest::Client) -> Result<(), GqlError> {
        let mut last = self.last_refresh.lock().await;
        *last = Some(Instant::now());
        self.refresh_now(http).await?
    }

    /// Outer: the download failed. Inner: the new manifest is in memory, was it saved?
    async fn refresh_now(&self, http: &reqwest::Client) -> Result<Result<(), GqlError>, GqlError> {
        let previous = self.current.read().await.clone();
        let newer = fetch_from_site(http, previous.as_ref().map(|m| m.client_version.as_str()))
            .await
            .inspect_err(|e| log(format!("GQL_MANIFEST: refresh failed: {e}")))?;
        Ok(self.apply(newer).await)
    }

    /// Logs what changed, saves, and keeps `newer` in memory even if the save fails
    async fn apply(&self, newer: Manifest) -> Result<(), GqlError> {
        if let Some(previous) = &*self.current.read().await {
            for op in previous.changed_ops(&newer, super::ops::USED) {
                log(format!(
                    "GQL_MANIFEST: {op} query text changed, check its fields"
                ));
            }
        }
        let saved = newer.save(&self.path);
        if let Err(e) = &saved {
            log(format!("GQL_MANIFEST: kept in memory only, {e}"));
        }
        log(format!(
            "GQL_MANIFEST: refreshed, client version {}, {} operations",
            newer.client_version,
            newer.operations.len()
        ));
        *self.current.write().await = Some(newer);
        saved
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};

    const CHUNK: &str = include_str!("testdata/chunk.js");

    fn fixture() -> Manifest {
        let at = Utc.with_ymd_and_hms(2026, 10, 17, 0, 0, 0).unwrap();
        Manifest::from_chunk(CHUNK, "a93ba50".into(), at).unwrap()
    }

    #[test]
    fn unescape() {
        assert_eq!(js_unescape(r#"a\\nb\'c\"d\/e"#).unwrap(), "a\\nb'c\"d/e");
        assert_eq!(js_unescape(r"é\x41\n\t").unwrap(), "éA\n\t");
        assert_eq!(js_unescape(r"😀").unwrap(), "😀");
        assert_eq!(js_unescape(r"\ud83d\ude00").unwrap(), "😀");
        assert!(js_unescape(r"\ud83d").is_err());
        assert!(js_unescape(r"\ud83dA").is_err());
        assert!(js_unescape(r"\u12").is_err());
        assert!(js_unescape("end\\").is_err());
    }

    #[test]
    fn literal_next_to_marker() {
        let lit = json_parse_literal(CHUNK).unwrap();
        assert!(lit.starts_with(r#"{"format":"apollo-persisted-query-manifest""#));
        assert!(lit.ends_with("]}"));
        assert_eq!(json_parse_literal("no manifest here"), None);
    }

    #[test]
    fn from_chunk() {
        let m = fixture();
        assert_eq!(m.client_version, "a93ba50");
        assert_eq!(m.operations.len(), 2);
        let op = &m.operations["GetOtherLeagues"];
        assert_eq!(op.body, "query GetOtherLeagues {\n  leagues\n}");
        assert_eq!(op.id, sha256_hex(&op.body));
        assert_eq!(m.operations["Other"].body, "query Other { it's é }");
    }

    #[test]
    fn from_chunk_rejects_bad_hash() {
        let bad = CHUNK.replace("e5cb032c", "00000000");
        let at = Utc.with_ymd_and_hms(2026, 10, 17, 0, 0, 0).unwrap();
        match Manifest::from_chunk(&bad, "x".into(), at) {
            Err(GqlError::Manifest(msg)) => assert!(msg.contains("GetOtherLeagues")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn changed_ops() {
        let old = fixture();
        let mut newer = old.clone();
        newer.operations.get_mut("Other").unwrap().body = "query Other { changed }".into();
        assert_eq!(
            old.changed_ops(&newer, &["GetOtherLeagues"]),
            Vec::<String>::new()
        );
        assert_eq!(
            old.changed_ops(&newer, &["GetOtherLeagues", "Other"]),
            vec!["Other"]
        );
        newer.operations.remove("GetOtherLeagues");
        assert_eq!(
            old.changed_ops(&newer, &["GetOtherLeagues"]),
            vec!["GetOtherLeagues"]
        );
    }

    #[test]
    fn save_and_load() {
        let dir = std::env::temp_dir().join(format!("triforce-manifest-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gql-manifest.json");
        let m = fixture();
        m.save(&path).unwrap();
        assert!(!dir.join("gql-manifest.json.tmp").exists());
        assert_eq!(Manifest::load(&path).unwrap(), m);
        assert!(Manifest::load(&dir.join("missing.json")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn chunks_and_version() {
        let html = r#"<script src="/_next/static/chunks/2vmy32u554l_d.js"></script><script src="/_next/static/chunks/2vmy32u554l_d.js"></script>"x+"static/chunks/"+e"#;
        assert_eq!(chunk_paths(html), vec!["static/chunks/2vmy32u554l_d.js"]);
        assert_eq!(chunk_paths(CHUNK), vec!["static/chunks/abc_def-1.js"]);
        let js = r#"e.s(["getAppVersion",0,function(){return"a93ba50"},"isLocal",0,function(){return false}])"#;
        assert_eq!(find_client_version(js).as_deref(), Some("a93ba50"));
        assert_eq!(find_client_version("nothing"), None);
    }

    #[test]
    fn refresh_once_an_hour() {
        let now = std::time::Instant::now();
        assert!(refresh_due(None, now));
        assert!(!refresh_due(
            Some(now),
            now + std::time::Duration::from_secs(3599)
        ));
        assert!(refresh_due(
            Some(now),
            now + std::time::Duration::from_secs(3600)
        ));
    }

    #[tokio::test]
    async fn apply_keeps_manifest_when_save_fails() {
        let dir = std::env::temp_dir().join(format!("triforce-nodir-{}", std::process::id()));
        let store = Store::open(dir.join("missing-dir").join("gql-manifest.json"));
        assert!(store.apply(fixture()).await.is_err());
        let (hash, _) = store.lookup("GetOtherLeagues").await.unwrap();
        assert_eq!(
            hash,
            "e5cb032cab501a5d0373c19bcbd93c0f116be54ab72accab019dfb34ca37f603"
        );
    }

    #[tokio::test]
    async fn store_lookup() {
        let dir = std::env::temp_dir().join(format!("triforce-store-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("gql-manifest.json");
        fixture().save(&path).unwrap();
        let store = Store::open(path);
        let (hash, version) = store.lookup("GetOtherLeagues").await.unwrap();
        assert_eq!(
            hash,
            "e5cb032cab501a5d0373c19bcbd93c0f116be54ab72accab019dfb34ca37f603"
        );
        assert_eq!(version, "a93ba50");
        assert!(matches!(
            store.lookup("Missing").await,
            Err(GqlError::UnknownHash { op: "Missing" })
        ));
        let empty = Store::open(dir.join("missing.json"));
        assert!(matches!(
            empty.lookup("GetOtherLeagues").await,
            Err(GqlError::UnknownHash { .. })
        ));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
