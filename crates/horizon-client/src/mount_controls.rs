//! Pack-provided touch labels, from assets/<namespace>/horizon/mount_controls.json.
use serde::Deserialize;
use std::{collections::BTreeMap, path::Path};

#[derive(Clone, Debug, Default)]
pub struct MountControls(pub BTreeMap<String, (String, String)>);

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    mounts: Vec<Mount>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Mount {
    entities: Vec<String>,
    sprint: String,
    attack: String,
}
impl MountControls {
    pub fn parse(text: &str) -> Result<Self, String> {
        let file: File = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let mut controls = Self::default();
        for mount in file.mounts {
            for id in mount.entities {
                if !valid_id(&id) {
                    return Err(format!("invalid mount entity id {id:?}"));
                }
                controls.0.insert(
                    id,
                    (
                        mount.sprint.chars().take(6).collect(),
                        mount.attack.chars().take(6).collect(),
                    ),
                );
            }
        }
        Ok(controls)
    }
    /// The merged resource root includes selected packs and enabled content mods.
    pub fn load(root: &Path) -> Self {
        let mut controls = Self::default();
        let Ok(namespaces) = std::fs::read_dir(root.join("assets")) else {
            return controls;
        };
        let mut paths: Vec<_> = namespaces
            .flatten()
            // Namespaces are folders; `assets/` also holds files (.mcassetsroot, pack.mcmeta).
            .filter(|ns| ns.file_type().is_ok_and(|t| t.is_dir()))
            .map(|ns| ns.path().join("horizon/mount_controls.json"))
            .collect();
        paths.sort();
        for path in paths {
            match std::fs::read_to_string(&path) {
                Ok(text) => match Self::parse(&text) {
                    Ok(labels) => controls.0.extend(labels.0),
                    Err(error) => log::warn!("Horizon: ignoring {}: {error}", path.display()),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => log::warn!("Horizon: ignoring {}: {error}", path.display()),
            }
        }
        controls
    }
}
fn valid_id(id: &str) -> bool {
    id.len() <= 256
        && id.split_once(':').is_some_and(|(ns, path)| {
            !ns.is_empty()
                && ns != "."
                && ns != ".."
                && !path.is_empty()
                && ns
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
                && path
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
                && path
                    .split('/')
                    .all(|p| !p.is_empty() && p != "." && p != "..")
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_multiple_mounts_clips_characters_and_rejects_bad_files() {
        let labels = MountControls::parse(r#"{"mounts":[
            {"entities":["iceandfire:fire_dragon","iceandfire:ice_dragon"],"sprint":"Flamé🔥long","attack":"Bite"},
            {"entities":["iceandfire:lightning_dragon"],"sprint":"Zap","attack":"Chomp"}
        ]}"#).unwrap();
        assert_eq!(
            labels.0["iceandfire:fire_dragon"],
            ("Flamé🔥".into(), "Bite".into())
        );
        assert_eq!(
            labels.0["iceandfire:ice_dragon"],
            labels.0["iceandfire:fire_dragon"]
        );
        assert_eq!(labels.0["iceandfire:lightning_dragon"].0, "Zap");
        for bad in [
            "{}",
            "not json",
            r#"{"mounts":[{"entities":["bad"],"sprint":"X","attack":"Y"}]}"#,
            r#"{"mounts":[],"unknown":true}"#,
        ] {
            assert!(MountControls::parse(bad).is_err(), "{bad}");
        }
    }
}
