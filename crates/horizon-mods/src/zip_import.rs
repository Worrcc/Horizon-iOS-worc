//! Import embedded `.hmod` packages from an arbitrary ZIP, without extracting its paths.
use crate::{
    Manifest,
    manifest::{MANIFEST, Side},
};
use serde::Serialize;
use std::{
    cmp::Ordering,
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{self, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering as AtomicOrdering},
};

const MAX_BYTES: u64 = 2 << 30;
const MAX_ENTRIES: usize = 512;
const MAX_RATIO: u64 = 1000;
const MAX_MANIFEST: u64 = 1 << 20;
const JAVA: &str =
    "This is a Java modpack. Horizon loads its own mods (.hmod files); Java mods can't run.";
const EMPTY: &str = "No Horizon mods (.hmod) in this zip.";

/// Installed packages: id, display name, version. Installation never changes enabled settings.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Report {
    pub added: Vec<(String, String, String)>,
    pub replaced: Vec<(String, String, String)>,
    pub skipped: Vec<(String, String)>,
}
impl Report {
    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.added
            .iter()
            .chain(&self.replaced)
            .map(|m| m.0.as_str())
    }
}

struct Temp(PathBuf);
impl Temp {
    fn new(dir: &Path) -> Result<(Self, File), String> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = dir.join(format!(
                ".import-{}-{}.tmp",
                std::process::id(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            match OpenOptions::new().write(true).create_new(true).open(&path) {
                Ok(file) => return Ok((Self(path), file)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.to_string()),
            }
        }
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Read only the manifest, with a separate bound: no component or content is loaded into RAM.
fn manifest(path: &Path) -> Result<Manifest, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|_| "not a Horizon mod file".to_string())?;
    let mut text = String::new();
    zip.by_name(MANIFEST)
        .map_err(|e| format!("{MANIFEST}: {e}"))?
        .take(MAX_MANIFEST + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() as u64 > MAX_MANIFEST {
        return Err(format!("{MANIFEST}: exceeds 1 MiB"));
    }
    Manifest::parse(&text)
}

// Manifests also allow versions such as "1" and arbitrary release names. Use SemVer for
// semantic versions (including short numeric releases), natural ordering otherwise.
fn version_cmp(a: &str, b: &str) -> Ordering {
    fn semantic(s: &str) -> Option<semver::Version> {
        let s = s.strip_prefix('v').unwrap_or(s);
        let mut s = s.to_owned();
        if s.bytes().all(|c| c.is_ascii_digit() || c == b'.') {
            while s.matches('.').count() < 2 {
                s.push_str(".0");
            }
        }
        semver::Version::parse(&s).ok()
    }
    if let (Some(a), Some(b)) = (semantic(a), semantic(b)) {
        return a.cmp_precedence(&b);
    }
    fn parts(s: &str) -> Vec<&str> {
        let mut start = 0;
        let mut parts = Vec::new();
        for (i, c) in s.char_indices().skip(1) {
            if c.is_ascii_digit() != s.as_bytes()[i - 1].is_ascii_digit() {
                parts.push(&s[start..i]);
                start = i;
            }
        }
        parts.push(&s[start..]);
        parts
    }
    for (a, b) in parts(a).into_iter().zip(parts(b)) {
        let order =
            if a.bytes().all(|c| c.is_ascii_digit()) && b.bytes().all(|c| c.is_ascii_digit()) {
                let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
                a.len().cmp(&b.len()).then_with(|| a.cmp(b))
            } else {
                a.cmp(b)
            };
        if order != Ordering::Equal {
            return order;
        }
    }
    parts(a).len().cmp(&parts(b).len())
}

/// Stage and validate every package before installing the winning versions. ZIP entry names
/// are report labels only; destination names come exclusively from validated manifest ids.
/// Progress counts examined `.hmod` entries. The caller owns deleting the source ZIP.
pub fn import(
    zip: &Path,
    mods_dir: &Path,
    progress: impl FnMut(usize, usize),
) -> Result<Report, String> {
    import_limited(zip, mods_dir, progress, MAX_BYTES, MAX_ENTRIES, MAX_RATIO)
}

fn import_limited(
    path: &Path,
    mods_dir: &Path,
    mut progress: impl FnMut(usize, usize),
    max_bytes: u64,
    max_entries: usize,
    max_ratio: u64,
) -> Result<Report, String> {
    let file = File::open(path).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut entries = Vec::new();
    let (mut bytes, mut java) = (0u64, false);
    for i in 0..zip.len() {
        let entry = zip.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = entry.name().map_err(|e| e.to_string())?.into_owned();
        // `..` is harmless as a label and is intentionally allowed. Hidden files and Mac
        // metadata are ignored at any depth, with either separator convention.
        if entry.is_dir()
            || name
                .split(['/', '\\'])
                .any(|p| p == "__MACOSX" || p.starts_with('.') && p != "..")
        {
            continue;
        }
        let leaf = name.rsplit(['/', '\\']).next().unwrap_or(&name);
        java |= leaf.ends_with(".jar") || matches!(leaf, "modrinth.index.json" | "manifest.json");
        if !leaf.ends_with(".hmod") {
            continue;
        }
        entries.push((i, name.clone()));
        if entries.len() > max_entries {
            return Err(format!(
                "{name}: .hmod entry cap hit (maximum {max_entries})"
            ));
        }
        bytes = bytes
            .checked_add(entry.size())
            .ok_or(".hmod byte cap hit (2 GiB)")?;
        if bytes > max_bytes {
            return Err(format!("{name}: .hmod byte cap hit (2 GiB)"));
        }
        if entry.size() > entry.compressed_size().max(1).saturating_mul(max_ratio) {
            return Err(format!(
                "{name}: compression ratio cap hit (maximum {max_ratio}:1)"
            ));
        }
    }
    if entries.is_empty() {
        return Err(if java { JAVA } else { EMPTY }.into());
    }
    fs::create_dir_all(mods_dir).map_err(|e| e.to_string())?;
    let total = entries.len();
    progress(0, total);
    let mut report = Report::default();
    let mut winners: BTreeMap<String, (Manifest, String, Temp)> = BTreeMap::new();
    let mut actual = 0u64;
    for (done, (index, name)) in entries.into_iter().enumerate() {
        let (temp, mut out) = Temp::new(mods_dir)?;
        let mut entry = zip.by_index(index).map_err(|e| e.to_string())?;
        let copied = io::copy(&mut (&mut entry).take(max_bytes - actual + 1), &mut out)
            .map_err(|e| e.to_string())?;
        actual += copied;
        if actual > max_bytes {
            return Err(format!("{name}: .hmod byte cap hit (2 GiB)"));
        }
        if copied > entry.compressed_size().max(1).saturating_mul(max_ratio) {
            return Err(format!(
                "{name}: compression ratio cap hit (maximum {max_ratio}:1)"
            ));
        }
        drop(out);
        match manifest(&temp.0) {
            Err(e) => report.skipped.push((name, e)),
            Ok(m) => {
                if let Some((previous, previous_name, _)) = winners.get(&slot(&m)) {
                    if version_cmp(&m.version, &previous.version) != Ordering::Greater {
                        report.skipped.push((
                            name,
                            format!(
                                "Duplicate id {}; keeping version {} from {previous_name}",
                                m.id, previous.version
                            ),
                        ));
                        progress(done + 1, total);
                        continue;
                    }
                    report.skipped.push((
                        previous_name.clone(),
                        format!(
                            "Duplicate id {}; keeping higher version {} from {name}",
                            m.id, m.version
                        ),
                    ));
                }
                winners.insert(slot(&m), (m, name, temp));
            }
        }
        progress(done + 1, total);
    }
    let existing: Vec<_> = crate::find(mods_dir)
        .into_iter()
        .filter(|p| p.is_file())
        .filter_map(|p| Some((slot(&manifest(&p).ok()?), p)))
        .collect();
    for (key, (m, _, temp)) in winners {
        let id = m.id.clone();
        let target = mods_dir.join(if m.side == Side::Server {
            format!("{id}_server.hmod")
        } else {
            format!("{id}.hmod")
        });
        let replaced = target.exists() || existing.iter().any(|(other, _)| other == &key);
        fs::rename(&temp.0, &target).map_err(|e| format!("{}: {e}", target.display()))?;
        for (_, path) in existing
            .iter()
            .filter(|(other, p)| other == &key && *p != target)
        {
            fs::remove_file(path).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        let entry = (id, m.name, m.version);
        if replaced {
            report.replaced.push(entry);
        } else {
            report.added.push(entry);
        }
    }
    Ok(report)
}

/// A client half and a server half may share one id (they pair by it), so the
/// import keeps one file per id and side.
fn slot(m: &Manifest) -> String {
    format!("{}/{}", m.id, m.side == Side::Server)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "horizon-import-test-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
        fn pack(&self, files: &[(&str, &[u8])]) -> PathBuf {
            let path = self.0.join("pack.zip");
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            for (name, bytes) in files {
                zip.start_file(
                    *name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
                zip.write_all(bytes).unwrap();
            }
            zip.finish().unwrap();
            path
        }
        fn mods(&self) -> PathBuf {
            self.0.join("horizon-mods")
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn package(id: &str, version: &str) -> Vec<u8> {
        crate::Package::write(
            &format!("id = {id:?}\nname = {id:?}\nversion = {version:?}\napi = \"0.1\"\n"),
            b"component",
        )
        .unwrap()
    }
    #[test]
    fn client_and_server_halves_with_one_id_are_both_kept() {
        let t = Scratch::new();
        let half = |side: &str, wasm: &[u8]| {
            let api = if side == "server" { "0.3" } else { "0.1" };
            crate::Package::write(
                &format!("id = \"pair\"\nname = \"pair\"\nversion = \"1\"\napi = \"{api}\"\nside = \"{side}\"\n"),
                wasm,
            )
            .unwrap()
        };
        let client = half("client", b"component-c");
        let server = half("server", b"component-s");
        let zip = t.pack(&[("c.hmod", &client), ("s.hmod", &server)]);
        let r = import(&zip, &t.mods(), |_, _| {}).unwrap();
        assert_eq!(r.added.len(), 2);
        assert!(r.skipped.is_empty());
        assert_eq!(fs::read(t.mods().join("pair.hmod")).unwrap(), client);
        assert_eq!(fs::read(t.mods().join("pair_server.hmod")).unwrap(), server);
        // Importing again replaces each half and keeps both files.
        let r = import(&zip, &t.mods(), |_, _| {}).unwrap();
        assert_eq!(r.replaced.len(), 2);
        assert_eq!(fs::read_dir(t.mods()).unwrap().count(), 2);
    }
    #[test]
    fn imports_nested_and_traversal_labels_but_no_other_files() {
        let t = Scratch::new();
        let a = package("a", "1");
        let b = package("b", "1");
        let zip = t.pack(&[
            ("nested/deep/a.hmod", &a),
            ("../../x.hmod", &b),
            ("__MACOSX/c.hmod", &a),
            ("hidden/.cache/c.hmod", &a),
            (".hidden.hmod", &a),
            ("options.txt", b"no"),
        ]);
        let mut progress = Vec::new();
        let r = import(&zip, &t.mods(), |d, t| progress.push((d, t))).unwrap();
        assert_eq!(r.added.len(), 2);
        assert!(r.skipped.is_empty());
        assert_eq!(progress, [(0, 2), (1, 2), (2, 2)]);
        assert_eq!(fs::read(t.mods().join("b.hmod")).unwrap(), b);
        assert!(!t.0.join("x.hmod").exists());
        assert_eq!(fs::read_dir(t.mods()).unwrap().count(), 2);
    }
    #[test]
    fn higher_version_wins_in_either_order_and_bad_manifests_are_skipped() {
        for reverse in [false, true] {
            let t = Scratch::new();
            let low = package("same", "1.9.0");
            let high = package("same", "1.10.0");
            let files = if reverse {
                [("high.hmod", &high[..]), ("low.hmod", &low[..])]
            } else {
                [("low.hmod", &low[..]), ("high.hmod", &high[..])]
            };
            let zip = t.pack(&[files[0], files[1], ("bad.hmod", b"not a zip")]);
            let r = import(&zip, &t.mods(), |_, _| {}).unwrap();
            assert_eq!(r.added, [("same".into(), "same".into(), "1.10.0".into())]);
            assert_eq!(r.skipped.len(), 2);
            assert!(
                r.skipped
                    .iter()
                    .any(|(n, r)| n == "low.hmod" && r.contains("Duplicate id"))
            );
            assert_eq!(fs::read(t.mods().join("same.hmod")).unwrap(), high);
        }
        assert_eq!(version_cmp("2", "1.9"), Ordering::Greater);
        assert_eq!(version_cmp("1.0.0", "1.0.0-rc.2"), Ordering::Greater);
    }
    #[test]
    fn invalid_manifest_is_skipped() {
        let t = Scratch::new();
        let mut inner = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        inner
            .start_file(MANIFEST, zip::write::SimpleFileOptions::default())
            .unwrap();
        inner.write_all(b"id = 'invalid/id'").unwrap();
        let inner = inner.finish().unwrap().into_inner();
        let zip = t.pack(&[("invalid.hmod", &inner)]);
        let r = import(&zip, &t.mods(), |_, _| {}).unwrap();
        assert!(r.added.is_empty());
        assert!(r.skipped[0].1.contains(MANIFEST));
    }
    #[test]
    fn java_and_empty_archives_have_clear_errors() {
        let t = Scratch::new();
        for name in ["mods/a.jar", "modrinth.index.json", "manifest.json"] {
            let zip = t.pack(&[(name, b"{}")]);
            assert_eq!(import(&zip, &t.mods(), |_, _| {}).unwrap_err(), JAVA);
        }
        for files in [&[][..], &[("readme.txt", &b"hi"[..])][..]] {
            let zip = t.pack(files);
            assert_eq!(import(&zip, &t.mods(), |_, _| {}).unwrap_err(), EMPTY);
        }
    }
    #[test]
    fn limits_are_checked_before_installation_and_staging_is_cleaned() {
        let t = Scratch::new();
        let a = package("a", "1");
        let zip = t.pack(&[("a.hmod", &a), ("b.hmod", &a)]);
        let limit = |bytes, entries, ratio| {
            import_limited(&zip, &t.mods(), |_, _| {}, bytes, entries, ratio).unwrap_err()
        };
        assert!(limit(1, 512, 1000).contains("byte cap"));
        assert!(limit(MAX_BYTES, 1, 1000).contains("entry cap"));
        let bomb = vec![0; 128 << 10];
        let zip = t.pack(&[("bomb.hmod", &bomb)]);
        assert!(
            import_limited(&zip, &t.mods(), |_, _| {}, MAX_BYTES, MAX_ENTRIES, 10)
                .unwrap_err()
                .contains("ratio cap")
        );
        assert!(!t.mods().exists());
    }
    #[test]
    fn replaces_by_manifest_id_and_keeps_enabled_settings() {
        let t = Scratch::new();
        fs::create_dir_all(t.mods()).unwrap();
        let settings = "mod.a:true\nmod.b:false\nkey.a.hold:key.keyboard.v\n";
        fs::write(t.0.join("horizon-mods.txt"), settings).unwrap();
        fs::write(t.mods().join("old-name.hmod"), package("a", "1")).unwrap();
        fs::write(t.mods().join("b.hmod"), package("b", "1")).unwrap();
        let (a, b) = (package("a", "2"), package("b", "2"));
        let zip = t.pack(&[("a.hmod", &a), ("b.hmod", &b)]);
        let r = import(&zip, &t.mods(), |_, _| {}).unwrap();
        assert_eq!(r.replaced.len(), 2);
        assert!(r.added.is_empty());
        assert!(!t.mods().join("old-name.hmod").exists());
        assert_eq!(fs::read(t.mods().join("a.hmod")).unwrap(), a);
        assert_eq!(
            fs::read_to_string(t.0.join("horizon-mods.txt")).unwrap(),
            settings
        );
    }
}
