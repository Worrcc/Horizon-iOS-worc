//! World data packs a mod ships whole under `datapacks/<name>/` (manifest
//! field `datapacks`, docs/mods.md "Data packs in a mod"). They are data
//! only: the game offers them in Create World > Data Packs and copies the
//! one the player picks into that world.
use crate::{
    content::MAX_FILE,
    manifest::{MANIFEST, Manifest},
};
use std::{io::Read, path::Path};

/// Files and bytes across all of a mod's packs (per file: `content::MAX_FILE`).
pub const MAX_FILES: usize = 65_536;
pub const MAX_TOTAL: u64 = 128 << 20;

/// A pack a mod lists: its folder name and `pack.mcmeta`'s description.
#[derive(Clone, Debug, PartialEq)]
pub struct ModPack {
    pub name: String,
    pub description: String,
}

fn part_ok(part: &str) -> bool {
    !part.is_empty()
        && part != "."
        && part != ".."
        && part
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
}

/// Whether an archive entry belongs to the `datapacks/` tree, however it is spelled.
fn in_tree(name: &str) -> bool {
    name.trim_start_matches(['/', '\\'])
        .replace('\\', "/")
        .starts_with("datapacks/")
}

/// Checks one `datapacks/...` path against the listed packs.
pub fn check_name(name: &str, listed: &[String]) -> Result<(), String> {
    let bad = |why: &str| {
        format!("{name:?}: {why} (data packs hold data/ json, mcfunction and nbt files only)")
    };
    let parts: Vec<&str> = name.split('/').collect();
    if parts.first() != Some(&"datapacks") || parts.len() < 3 || !parts.iter().all(|p| part_ok(p)) {
        return Err(bad("not a plain datapacks/<name>/... path"));
    }
    if !listed.iter().any(|l| l == parts[1]) {
        return Err(format!(
            "{name:?}: pack {} is not listed in datapacks in {MANIFEST}",
            parts[1]
        ));
    }
    let rest = &parts[2..];
    match rest {
        ["pack.mcmeta"] | ["pack.png"] => Ok(()),
        ["data", _ns, tail @ ..] if !tail.is_empty() => {
            let ext = name.rsplit_once('.').map(|(_, e)| e);
            if matches!(ext, Some("json" | "mcfunction" | "nbt")) {
                Ok(())
            } else {
                Err(bad("file type not allowed"))
            }
        }
        _ => Err(bad("not pack.mcmeta, pack.png or data/<namespace>/...")),
    }
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, u64)>) -> Result<(), String> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in read.flatten() {
        let path = entry.path();
        let meta = std::fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            return Err(format!("{} is a symbolic link", path.display()));
        }
        if meta.is_dir() {
            walk(root, &path, out)?;
        } else if meta.is_file() {
            let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
            let parts: Option<Vec<&str>> = rel.iter().map(|p| p.to_str()).collect();
            let parts = parts.ok_or_else(|| format!("{} is not UTF-8", rel.display()))?;
            out.push((parts.join("/"), meta.len()));
        }
        if out.len() > MAX_FILES {
            return Err(format!("more than {MAX_FILES} data pack files"));
        }
    }
    Ok(())
}

/// Checks the data packs of a `.hmod` or mod folder against its manifest and
/// returns them in manifest order.
pub fn check(path: &Path, manifest: &Manifest) -> Result<Vec<ModPack>, String> {
    let dir = path.is_dir();
    let mut zip = None;
    let mut entries: Vec<(String, u64)> = Vec::new();
    if dir {
        walk(path, &path.join("datapacks"), &mut entries)?;
    } else {
        let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
        let mut z = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
        for i in 0..z.len() {
            let entry = z.by_index_raw(i).map_err(|e| e.to_string())?;
            let name = entry.name().map_err(|e| e.to_string())?.into_owned();
            if in_tree(&name) && !entry.is_dir() {
                entries.push((name, entry.size()));
            }
        }
        zip = Some(z);
    }
    if !entries.is_empty() && manifest.datapacks.is_empty() {
        return Err(format!("has datapacks/ but lists none in {MANIFEST}"));
    }
    if entries.len() > MAX_FILES {
        return Err(format!("more than {MAX_FILES} data pack files"));
    }
    let mut total = 0u64;
    for (name, size) in &entries {
        check_name(name, &manifest.datapacks)?;
        if *size > MAX_FILE {
            return Err(format!("{name} is over {} MiB", MAX_FILE >> 20));
        }
        total += size;
    }
    if total > MAX_TOTAL {
        return Err(format!("data packs are over {} MiB", MAX_TOTAL >> 20));
    }
    let mut out = Vec::new();
    for pack in &manifest.datapacks {
        let name = format!("datapacks/{pack}/pack.mcmeta");
        let mut bytes = Vec::new();
        match &mut zip {
            None => bytes = std::fs::read(path.join(&name)).map_err(|e| format!("{name}: {e}"))?,
            Some(z) => {
                let f = z.by_name(&name).map_err(|e| format!("{name}: {e}"))?;
                f.take(MAX_FILE + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|e| format!("{name}: {e}"))?;
            }
        }
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| format!("{name}: {e}"))?;
        let Some(meta) = json.get("pack").filter(|p| p.is_object()) else {
            return Err(format!("{name}: no \"pack\" object"));
        };
        let description = meta
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("")
            .to_owned();
        out.push(ModPack {
            name: pack.clone(),
            description,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    const MCMETA: &str = r#"{"pack":{"description":"D","min_format":121,"max_format":121}}"#;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("hm-dp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn manifest(packs: &[&str]) -> Manifest {
        let list: Vec<String> = packs.iter().map(|p| format!("\"{p}\"")).collect();
        let list = if packs.is_empty() {
            String::new()
        } else {
            format!("datapacks = [{}]", list.join(","))
        };
        Manifest::parse(&format!(
            "id = \"m\"\nname = \"M\"\nversion = \"1\"\napi = \"0.2\"\n{list}\n"
        ))
        .unwrap()
    }

    fn hmod(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        let path = scratch(tag).join("m.hmod");
        let mut z = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        for (name, body) in files {
            z.start_file(*name, options).unwrap();
            z.write_all(body.as_bytes()).unwrap();
        }
        z.finish().unwrap();
        path
    }

    #[test]
    fn finds_listed_packs_in_an_hmod() {
        let m = manifest(&["b", "a"]);
        let p = hmod(
            "find",
            &[
                ("horizon-mod.toml", "x"),
                (
                    "datapacks/a/pack.mcmeta",
                    r#"{"pack":{"description":"Alpha"}}"#,
                ),
                (
                    "datapacks/b/pack.mcmeta",
                    r#"{"pack":{"description":{"text":"x"}}}"#,
                ),
                ("datapacks/a/data/minecraft/worldgen/noise/n.json", "{}"),
            ],
        );
        let found = check(&p, &m).unwrap();
        assert_eq!(
            found,
            vec![
                ModPack {
                    name: "b".into(),
                    description: String::new()
                },
                ModPack {
                    name: "a".into(),
                    description: "Alpha".into()
                },
            ]
        );
    }

    #[test]
    fn finds_packs_in_a_mod_folder() {
        let dir = scratch("folder");
        let d = dir.join("datapacks/a/data/x");
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(dir.join("datapacks/a/pack.mcmeta"), MCMETA).unwrap();
        std::fs::write(d.join("y.json"), "{}").unwrap();
        let found = check(&dir, &manifest(&["a"])).unwrap();
        assert_eq!(found[0].description, "D");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/hostname", d.join("link.json")).unwrap();
            assert!(
                check(&dir, &manifest(&["a"]))
                    .unwrap_err()
                    .contains("symbolic")
            );
        }
    }

    #[test]
    fn rejects_traversal_and_odd_names() {
        for bad in [
            "datapacks/a/../x.json",
            "datapacks/a/data/x/../../../../e.json",
            "datapacks/a\\x.json",
            "/datapacks/a/data/x/y.json",
            "datapacks/A/data/x/y.json",
        ] {
            let p = hmod("odd", &[("datapacks/a/pack.mcmeta", MCMETA), (bad, "{}")]);
            assert!(check(&p, &manifest(&["a"])).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_native_and_unknown_files() {
        for bad in [
            "datapacks/a/data/x/y.so",
            "datapacks/a/data/x/y.wasm",
            "datapacks/a/assets/x/y.json",
            "datapacks/a/data/x.json",
        ] {
            let p = hmod("native", &[("datapacks/a/pack.mcmeta", MCMETA), (bad, "x")]);
            assert!(check(&p, &manifest(&["a"])).is_err(), "{bad}");
        }
    }

    #[test]
    fn rejects_unlisted_or_missing_mcmeta() {
        let p = hmod(
            "unl",
            &[
                ("datapacks/a/pack.mcmeta", MCMETA),
                ("datapacks/z/pack.mcmeta", MCMETA),
            ],
        );
        assert!(check(&p, &manifest(&["a"])).is_err());
        let p = hmod("nometa", &[("datapacks/a/data/x/y.json", "{}")]);
        assert!(check(&p, &manifest(&["a"])).is_err());
        let p = hmod("badmeta", &[("datapacks/a/pack.mcmeta", "{}")]);
        assert!(check(&p, &manifest(&["a"])).is_err());
    }

    #[test]
    fn rejects_datapacks_without_manifest_field() {
        let p = hmod("nofield", &[("datapacks/a/pack.mcmeta", MCMETA)]);
        assert!(
            check(&p, &manifest(&[]))
                .unwrap_err()
                .contains("lists none")
        );
    }

    #[cfg(feature = "index")]
    #[test]
    fn a_codeless_hmod_passes_package_read_and_inspect() {
        let toml =
            "id = \"m\"\nname = \"M\"\nversion = \"1\"\napi = \"0.2\"\ndatapacks = [\"a\"]\n";
        let p = hmod(
            "inspect",
            &[
                ("horizon-mod.toml", toml),
                ("datapacks/a/pack.mcmeta", MCMETA),
            ],
        );
        let pkg = crate::Package::read(&p).unwrap();
        assert!(pkg.wasm.is_none());
        crate::index::inspect(&crate::index::Kind::Mod, &p).unwrap();
        let bad = hmod(
            "inspectbad",
            &[
                ("horizon-mod.toml", toml),
                ("datapacks/a/pack.mcmeta", MCMETA),
                ("datapacks/a/data/x/y.so", "x"),
            ],
        );
        assert!(crate::index::inspect(&crate::index::Kind::Mod, &bad).is_err());
    }
}
