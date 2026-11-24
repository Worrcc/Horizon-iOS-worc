//! API 39: a shared directory of which mod provides an API for a game object.
use super::*;
use server_bindings::horizon::mods::lookup::{self, TargetKind};

/// (kind, object id, api id) -> (owning mod id, provider label).
pub(super) type LookupDirectory = Arc<Mutex<HashMap<(String, String, String), (String, String)>>>;

fn kind_name(kind: TargetKind) -> &'static str {
    match kind {
        TargetKind::Block => "block",
        TargetKind::Item => "item",
        TargetKind::Entity => "entity",
    }
}

impl lookup::Host for State {
    fn register(
        &mut self,
        kind: TargetKind,
        id: String,
        api: String,
        provider: String,
    ) -> Result<(), String> {
        if !resource_id(&id) || !resource_id(&api) {
            return Err("lookup ids must be resource ids".into());
        }
        if provider.is_empty() || provider.len() > 64 {
            return Err("provider label must be 1..64 bytes".into());
        }
        let key = (kind_name(kind).to_owned(), id, api);
        let mut directory = self.lookup.lock().map_err(|_| "lookup unavailable")?;
        if directory.get(&key).map_or(true, |(owner, _)| owner != &self.id)
            && directory.values().filter(|(owner, _)| owner == &self.id).count() >= 64
        {
            return Err("at most 64 lookup registrations per mod".into());
        }
        directory.insert(key, (self.id.clone(), provider));
        Ok(())
    }

    fn unregister(&mut self, kind: TargetKind, id: String, api: String) -> Result<(), String> {
        let key = (kind_name(kind).to_owned(), id, api);
        let mut directory = self.lookup.lock().map_err(|_| "lookup unavailable")?;
        if directory.get(&key).is_some_and(|(owner, _)| owner == &self.id) {
            directory.remove(&key);
        }
        Ok(())
    }

    fn find(&mut self, kind: TargetKind, id: String, api: String) -> Option<(String, String)> {
        self.lookup
            .lock()
            .ok()?
            .get(&(kind_name(kind).to_owned(), id, api))
            .cloned()
    }
}
