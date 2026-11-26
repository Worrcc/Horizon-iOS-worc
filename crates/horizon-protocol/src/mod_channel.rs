//! Sandboxed mod channel negotiation and bounded fragmentation (API 28).
//! Separate from native mod_net, whose wire contract stays unchanged.
use crate::{
    Error, Result,
    buf::{Reader, Writer},
};

pub const CHANNEL: &str = "horizon:wasm_channel";
pub const MAX_DATA: usize = 65536;
pub const MAX_MODS: usize = 128;
const PART: usize = 24000;

#[derive(Debug, PartialEq)]
pub enum Packet {
    Offer(Vec<String>),
    Part {
        id: String,
        name: String,
        serial: u32,
        offset: u32,
        total: u32,
        data: Vec<u8>,
    },
}
fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}
fn name_ok(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'.' | b'/' | b'-')
        })
}
pub fn offer(ids: &[String]) -> Vec<u8> {
    let mut w = Writer::new();
    w.u8(0);
    let ids: Vec<_> = ids.iter().filter(|id| id_ok(id)).take(MAX_MODS).collect();
    w.varint(ids.len() as i32);
    for id in ids {
        w.string(id);
    }
    w.data
}
pub fn message(id: &str, name: &str, serial: u32, data: &[u8]) -> Result<Vec<Vec<u8>>> {
    if !id_ok(id) || !name_ok(name) || data.len() > MAX_DATA {
        return Err(Error::new("invalid mod channel message"));
    }
    let mut out = Vec::new();
    for offset in (0..data.len().max(1)).step_by(PART) {
        let mut w = Writer::new();
        w.u8(1);
        w.string(id);
        w.string(name);
        w.u32(serial);
        w.u32(offset as u32);
        w.u32(data.len() as u32);
        w.byte_array(&data[offset..(offset + PART).min(data.len())]);
        out.push(w.data);
    }
    Ok(out)
}
pub fn decode(bytes: &[u8]) -> Result<Packet> {
    if bytes.len() > 32767 {
        return Err(Error::new("mod channel packet too large"));
    }
    let mut r = Reader::new(bytes);
    let p = match r.u8()? {
        0 => {
            let n = r.varint()?;
            if !(0..=MAX_MODS as i32).contains(&n) {
                return Err(Error::new("too many mod channels"));
            }
            let mut ids = Vec::new();
            for _ in 0..n {
                let id = r.string(64)?;
                if !id_ok(&id) {
                    return Err(Error::new("invalid mod id"));
                }
                ids.push(id);
            }
            ids.sort();
            ids.dedup();
            Packet::Offer(ids)
        }
        1 => {
            let id = r.string(64)?;
            let name = r.string(64)?;
            let serial = r.u32()?;
            let offset = r.u32()?;
            let total = r.u32()?;
            let data = r.byte_array(PART)?;
            let offset_usize = offset as usize;
            let total_usize = total as usize;
            if !id_ok(&id)
                || !name_ok(&name)
                || total_usize > MAX_DATA
                || offset_usize > total_usize
                || !offset_usize.is_multiple_of(PART)
                || data.len() != PART.min(total_usize - offset_usize)
                || (offset == total && total != 0)
            {
                return Err(Error::new("invalid channel fragment"));
            }
            Packet::Part {
                id,
                name,
                serial,
                offset,
                total,
                data,
            }
        }
        _ => return Err(Error::new("unknown mod channel packet")),
    };
    if r.remaining() != 0 {
        return Err(Error::new("trailing channel data"));
    }
    Ok(p)
}
#[derive(Default)]
pub struct Assembly {
    pending: Option<(String, String, u32, u32, Vec<u8>)>,
}
impl Assembly {
    /// Ordered transport permits one in-flight message per connection. A new
    /// first part replaces an abandoned message, bounding partial storage to 64 KiB.
    pub fn accept(&mut self, p: Packet) -> Option<(String, String, Vec<u8>)> {
        let Packet::Part {
            id,
            name,
            serial,
            offset,
            total,
            data,
        } = p
        else {
            return None;
        };
        if offset == 0 {
            self.pending = Some((id.clone(), name.clone(), serial, total, Vec::new()));
        }
        let (pid, pname, seq, size, bytes) = self.pending.as_mut()?;
        if *pid != id
            || *pname != name
            || *seq != serial
            || *size != total
            || bytes.len() != offset as usize
        {
            self.pending = None;
            return None;
        }
        bytes.extend(data);
        if bytes.len() == total as usize {
            let (id, name, _, _, data) = self.pending.take()?;
            Some((id, name, data))
        } else {
            None
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_sized_messages_round_trip_below_vanilla_packet_limit() {
        for size in [0, 1, 24000, 24001, MAX_DATA] {
            let data = vec![177; size];
            let mut assembly = Assembly::default();
            let mut result = None;
            for part in message("storage", "terminal/update", 42, &data).unwrap() {
                assert!(part.len() <= 32767);
                result = assembly.accept(decode(&part).unwrap());
            }
            assert_eq!(
                result,
                Some(("storage".into(), "terminal/update".into(), data))
            );
        }
        assert!(message("storage", "bad:scope", 0, &[]).is_err());
        assert!(message("storage", "state", 0, &vec![0; MAX_DATA + 1]).is_err());
    }
    #[test]
    fn missing_and_reordered_parts_never_deliver_partial_messages() {
        let parts = message("storage", "state", 1, &vec![0; MAX_DATA]).unwrap();
        let mut a = Assembly::default();
        assert!(a.accept(decode(&parts[1]).unwrap()).is_none());
        assert!(a.accept(decode(&parts[0]).unwrap()).is_none());
        assert!(a.accept(decode(&parts[2]).unwrap()).is_none());
        assert_eq!(
            decode(&offer(&["storage".into()])),
            Ok(Packet::Offer(vec!["storage".into()]))
        );
        let mut invalid = parts[0].clone();
        invalid.push(0);
        assert!(decode(&invalid).is_err());
    }
}
