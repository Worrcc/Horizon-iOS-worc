//! API 42: cross-mod messages between running server mods.
use super::*;
use server_bindings::horizon::mods::net;

const MAX_NAMES: usize = 16;
const MAX_DATA: usize = 65536;

fn name_ok(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_.-".contains(&b))
}

impl net::Host for State {
    fn register(&mut self, name: String) -> Result<(), String> {
        if !name_ok(&name) {
            return Err("net payload name must be [a-z0-9_.-]{1,64}".into());
        }
        if !self.net_names.contains(&name) {
            if self.net_names.len() >= MAX_NAMES {
                return Err("at most 16 net payload names per mod".into());
            }
            self.net_names.push(name);
        }
        Ok(())
    }

    fn send_to_mod(&mut self, to: String, name: String, data: Vec<u8>) -> Result<(), String> {
        self.send(Some(to), name, data)
    }

    fn send_to_all(&mut self, name: String, data: Vec<u8>) -> Result<(), String> {
        self.send(None, name, data)
    }
}

impl State {
    fn send(&mut self, to: Option<String>, name: String, data: Vec<u8>) -> Result<(), String> {
        if !self.server {
            return Err("server only".into());
        }
        if !self.net_names.contains(&name) {
            return Err("payload name is not registered by this mod".into());
        }
        if data.len() > MAX_DATA {
            return Err("net payload exceeds 65536 bytes".into());
        }
        if !channel_host::allowance(&mut self.net_budget, self.snapshot.tick, 64) {
            return Err("at most 64 net messages per mod per tick".into());
        }
        self.out.push(Output::NetMessage(NetMessage {
            from: self.id.clone(),
            to,
            name,
            data,
        }));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn net_names_are_kept_short_and_deduplicated() {
        assert!(name_ok("chest.energy"));
        assert!(!name_ok("Bad Name"));
        assert!(!name_ok(""));
    }
}
