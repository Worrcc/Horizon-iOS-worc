//! Content declarations append climate entries to imported preset tables once.
use super::{BiomeSourceCatalog, BiomeSourceDefinition};
use crate::worldgen::density::identifier;
use std::collections::BTreeMap;

impl BiomeSourceCatalog {
    /// Apply `biome_source_addition` resources in resource-name order. Empty
    /// additions return the original tables byte-for-byte, without building trees.
    /// This Rust loader API adds no WASM imports or runtime registry mutation.
    pub fn with_preset_additions(
        &self,
        mut presets: BTreeMap<String, String>,
        additions: &BTreeMap<String, String>,
    ) -> Result<BTreeMap<String, String>, String> {
        for (name, text) in additions {
            let apply = || -> Result<(String, String), Box<dyn std::error::Error>> {
                let mut value: serde_json::Value = serde_json::from_str(text)?;
                let preset = identifier(
                    value["horizon:preset"]
                        .as_str()
                        .ok_or("missing horizon:preset")?,
                )?;
                let original = presets.get(&preset).ok_or("unknown target preset")?;
                // The vanilla codec may fall back from a bad inline list to a
                // preset. An addition must validate its list, never fall back.
                value
                    .as_object_mut()
                    .ok_or("expected source object")?
                    .remove("preset");
                let BiomeSourceDefinition::MultiNoise(entries) = self.decode(&value.to_string())?
                else {
                    return Err("expected inline minecraft:multi_noise source".into());
                };
                let mut source: serde_json::Value = serde_json::from_str(original)?;
                let list = source
                    .as_array_mut()
                    .ok_or("expected quantized preset table")?;
                if list.len().saturating_add(entries.len()) > 65536 {
                    return Err("combined climate entry limit is 65536".into());
                }
                for (point, biome) in entries {
                    list.push(serde_json::json!({
                        "coordinates": point.coordinates.map(|p| [p.min, p.max]),
                        "offset": point.offset,
                        "biome": biome,
                    }));
                }
                // The generator builds the search tree once when resolving its
                // source; avoid a second, discarded tree on the loading path.
                Ok((preset, source.to_string()))
            };
            let (preset, text) =
                apply().map_err(|e| format!("biome source addition {name}: {e}"))?;
            presets.insert(preset, text);
        }
        Ok(presets)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::worldgen::{biome::MultiNoiseBiomeSource, climate::TargetPoint};
    use serde_json::json;

    #[test]
    fn content_climate_point_and_vanilla_elsewhere() {
        let catalog = BiomeSourceCatalog::new(
            &[
                "minecraft:plains".into(),
                "minecraft:desert".into(),
                "test:frost".into(),
            ]
            .into(),
            &BTreeMap::new(),
            &["minecraft:overworld".into()].into(),
        )
        .unwrap();
        let presets: BTreeMap<_, _> = [(
            "minecraft:overworld".into(),
            json!([
                {"coordinates":vec![[-10000,-10000];6],"offset":0,"biome":"minecraft:plains"},
                {"coordinates":vec![[10000,10000];6],"offset":0,"biome":"minecraft:desert"}
            ])
            .to_string(),
        )]
        .into();
        assert_eq!(
            catalog
                .with_preset_additions(presets.clone(), &BTreeMap::new())
                .unwrap(),
            presets
        );
        let addition = json!({"type":"minecraft:multi_noise","horizon:preset":"minecraft:overworld",
        "biomes":[{"biome":"test:frost","parameters":{
            "temperature":0,"humidity":0,"continentalness":0,"erosion":0,"depth":0,"weirdness":0,"offset":0
        }}]});
        let combined = catalog
            .with_preset_additions(
                presets.clone(),
                &[("test:frost".into(), addition.to_string())].into(),
            )
            .unwrap();
        let original =
            MultiNoiseBiomeSource::from_quantized_json(&presets["minecraft:overworld"]).unwrap();
        let source =
            MultiNoiseBiomeSource::from_quantized_json(&combined["minecraft:overworld"]).unwrap();
        assert_eq!(
            &source.parameters().entries()[..2],
            original.parameters().entries()
        );
        let mut search = source.parameters().searcher();
        assert_eq!(
            search.find(TargetPoint::from_values([0.; 6])).unwrap(),
            "test:frost"
        );
        for v in [-1.2, -1., -0.8, 0.8, 1., 1.2] {
            let target = TargetPoint::from_values([v; 6]);
            assert_eq!(
                search.find(target).unwrap(),
                original.parameters().searcher().find(target).unwrap()
            );
        }
        for bad in [
            json!(null),
            json!({"biomes":[]}),
            json!({"type":"minecraft:fixed","biome":"test:frost","horizon:preset":"minecraft:overworld"}),
            {
                let mut v = addition.clone();
                v["horizon:preset"] = json!("test:missing");
                v
            },
            {
                let mut v = addition.clone();
                v["biomes"][0]["biome"] = json!("test:missing");
                v
            },
            {
                let mut v = addition.clone();
                v["biomes"][0]["parameters"]["temperature"] = json!([1, -1]);
                v
            },
            // A preset must not hide invalid inline entries.
            json!({"type":"minecraft:multi_noise","horizon:preset":"minecraft:overworld","preset":"minecraft:overworld","biomes":[]}),
        ] {
            let error = catalog
                .with_preset_additions(
                    presets.clone(),
                    &[("test:bad".into(), bad.to_string())].into(),
                )
                .unwrap_err();
            assert!(error.contains("test:bad"), "{error}");
        }
        // Additions accumulate even when two resources target the same preset;
        // the input map's key ordering makes the resulting list deterministic.
        let combined = catalog
            .with_preset_additions(
                presets.clone(),
                &[
                    ("test:b".into(), addition.to_string()),
                    ("test:a".into(), addition.to_string()),
                ]
                .into(),
            )
            .unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&combined["minecraft:overworld"])
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            4
        );
        let entry = json!({"coordinates":vec![[0,0];6],"offset":0,"biome":"minecraft:plains"});
        let full = [(
            "minecraft:overworld".into(),
            serde_json::to_string(&vec![entry; 65536]).unwrap(),
        )]
        .into();
        assert!(
            catalog
                .with_preset_additions(full, &[("test:full".into(), addition.to_string())].into())
                .unwrap_err()
                .contains("combined climate entry limit")
        );
    }
}
