//! API 41: host-managed persistent values attached to game objects.
use super::*;
use server_bindings::horizon::mods::attachments::{self, AttachmentTarget};
use std::io::Read;

type Key = (u8, String, i32, i32, i32, String);

fn key(target: &AttachmentTarget, name: &str) -> Key {
    match target {
        AttachmentTarget::Entity(id) => (0, String::new(), *id as i32, 0, 0, name.to_owned()),
        AttachmentTarget::Chunk(c) => (1, c.dimension.clone(), c.x, c.z, 0, name.to_owned()),
        AttachmentTarget::World(d) => (2, d.clone(), 0, 0, 0, name.to_owned()),
        AttachmentTarget::Block(b) => (
            3,
            b.dimension.clone(),
            b.x,
            b.y,
            b.z,
            name.to_owned(),
        ),
    }
}

#[derive(Default)]
pub(super) struct AttachmentStore {
    pub(super) values: HashMap<Key, Vec<u8>>,
    bytes: u64,
}

const MAX_KEYS: usize = 4096;
const MAX_BYTES: u64 = 4 << 20;
const MAX_VALUE: usize = 4096;

impl AttachmentStore {
    pub(super) fn load(path: &Path) -> Self {
        let mut store = Self::default();
        let Ok(mut file) = std::fs::File::open(path) else {
            return store;
        };
        let mut raw = Vec::new();
        if file.read_to_end(&mut raw).is_err() {
            return store;
        }
        let mut at = 0;
        let read_u8 = |at: &mut usize| -> Option<u8> {
            let v = *raw.get(*at)?;
            *at += 1;
            Some(v)
        };
        let read_u32 = |at: &mut usize| -> Option<u32> {
            let bytes = raw.get(*at..*at + 4)?;
            *at += 4;
            Some(u32::from_be_bytes(bytes.try_into().ok()?))
        };
        let read_i32 = |at: &mut usize| -> Option<i32> { read_u32(at).map(|v| v as i32) };
        let read_string = |at: &mut usize| -> Option<String> {
            let len = read_u32(at)? as usize;
            let bytes = raw.get(*at..*at + len)?;
            *at += len;
            String::from_utf8(bytes.to_vec()).ok()
        };
        while at < raw.len() {
            let Some(kind) = read_u8(&mut at) else { break };
            let Some(dimension) = read_string(&mut at) else { break };
            let Some(a) = read_i32(&mut at) else { break };
            let Some(b) = read_i32(&mut at) else { break };
            let Some(c) = read_i32(&mut at) else { break };
            let Some(name) = read_string(&mut at) else { break };
            let Some(len) = read_u32(&mut at) else { break };
            let Some(value) = raw.get(at..at + len as usize).map(|v| v.to_vec()) else {
                break;
            };
            at += len as usize;
            if len as usize > MAX_VALUE || store.bytes.saturating_add(len as u64) > MAX_BYTES {
                continue;
            }
            store.bytes += len as u64;
            store.values.insert((kind, dimension, a, b, c, name), value);
            if store.values.len() >= MAX_KEYS {
                break;
            }
        }
        store
    }

    pub(super) fn save(&self, path: &Path) {
        let mut raw = Vec::new();
        for ((kind, dimension, a, b, c, name), value) in &self.values {
            raw.push(*kind);
            let string = |out: &mut Vec<u8>, s: &str| {
                out.extend_from_slice(&(s.len() as u32).to_be_bytes());
                out.extend_from_slice(s.as_bytes());
            };
            string(&mut raw, dimension);
            for n in [a, b, c] {
                raw.extend_from_slice(&(*n as u32).to_be_bytes());
            }
            string(&mut raw, name);
            raw.extend_from_slice(&(value.len() as u32).to_be_bytes());
            raw.extend_from_slice(value);
        }
        let Some(parent) = path.parent() else { return };
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
        let pending = path.with_extension("pending");
        if std::fs::write(&pending, raw).is_ok() {
            let _ = std::fs::rename(pending, path);
        }
    }

    pub(super) fn path(data_dir: &Path) -> PathBuf {
        data_dir.with_extension("attachments")
    }
}

impl attachments::Host for State {
    fn set_attachment(
        &mut self,
        target: AttachmentTarget,
        name: String,
        value: Option<Vec<u8>>,
    ) -> Result<(), String> {
        if name.is_empty() || name.len() > 64 {
            return Err("attachment key must be 1..64 bytes".into());
        }
        let k = key(&target, &name);
        match value {
            Some(bytes) => {
                if bytes.len() > MAX_VALUE {
                    return Err("attachment value exceeds 4096 bytes".into());
                }
                let old = self.attachments.values.get(&k).map_or(0, Vec::len) as u64;
                let next = self.attachments.bytes - old + bytes.len() as u64;
                if next > MAX_BYTES {
                    return Err("attachment quota of 4 MiB exceeded".into());
                }
                if !self.attachments.values.contains_key(&k)
                    && self.attachments.values.len() >= MAX_KEYS
                {
                    return Err("at most 4096 attachment keys".into());
                }
                self.attachments.bytes = next;
                self.attachments.values.insert(k, bytes);
            }
            None => {
                if let Some(old) = self.attachments.values.remove(&k) {
                    self.attachments.bytes -= old.len() as u64;
                }
            }
        }
        Ok(())
    }

    fn attachment(&mut self, target: AttachmentTarget, name: String) -> Option<Vec<u8>> {
        self.attachments.values.get(&key(&target, &name)).cloned()
    }
}
