//! Converts Blockbench GeckoLib Bedrock animations to vanilla keyframe animations.

use super::keyframes::{Animation, Channel, Keyframe, Target};
use serde_json::Value;
use std::collections::HashMap;

/// Every animation of a `*.animation.json` file by name, converted to vanilla keyframes.
pub fn parse(file: &Value) -> Result<HashMap<String, Animation>, String> {
    let animations = file["animations"]
        .as_object()
        .ok_or_else(|| "missing animations object".to_owned())?;
    let mut result = HashMap::new();
    for (name, value) in animations {
        let mut warning = false;
        let mut bones = Vec::new();
        let mut max_time: f32 = 0.0;
        if let Some(entries) = value["bones"].as_object() {
            for (bone, channels) in entries {
                let mut converted = Vec::new();
                for (key, target) in [
                    ("position", Target::Position),
                    ("rotation", Target::Rotation),
                    ("scale", Target::Scale),
                ] {
                    if let Some(channel) = channels.get(key) {
                        let mut keyframes = Vec::new();
                        if channel.is_array() || channel.is_number() || channel.is_string() {
                            keyframes.push(make_keyframe(
                                0.0,
                                channel,
                                false,
                                target,
                                &mut warning,
                            ));
                        } else if let Some(timed) = channel.as_object() {
                            let mut frames: Vec<_> = timed
                                .iter()
                                .filter_map(|(time, frame)| {
                                    time.parse::<f32>().ok().map(|t| (t, frame))
                                })
                                .collect();
                            frames.sort_by(|a, b| a.0.total_cmp(&b.0));
                            for (time, frame) in frames {
                                max_time = max_time.max(time);
                                keyframes.push(make_keyframe(
                                    time,
                                    frame,
                                    true,
                                    target,
                                    &mut warning,
                                ));
                            }
                        }
                        converted.push(Channel { target, keyframes });
                    }
                }
                bones.push((bone.clone(), converted));
            }
        }
        if warning {
            log::warn!("Horizon: animation {name}: unsupported Molang component evaluated as 0.0");
        }
        let length = value["animation_length"]
            .as_f64()
            .map(|v| v as f32)
            .unwrap_or(max_time);
        result.insert(
            name.clone(),
            Animation {
                length,
                looping: value["loop"].as_bool().unwrap_or(false),
                bones,
            },
        );
    }
    Ok(result)
}

fn make_keyframe(
    time: f32,
    value: &Value,
    is_frame: bool,
    target: Target,
    warning: &mut bool,
) -> Keyframe {
    let (pre, post, catmull) = if is_frame && value.is_object() {
        let mode = value["lerp_mode"].as_str().or(value["easing"].as_str());
        let catmull = mode == Some("catmullrom");
        if value.get("pre").is_some() || value.get("post").is_some() {
            let pre = value.get("pre").or_else(|| value.get("post")).unwrap();
            let post = value.get("post").or_else(|| value.get("pre")).unwrap();
            (vector(pre, warning), vector(post, warning), catmull)
        } else {
            let v = vector(&value["vector"], warning);
            (v, v, catmull)
        }
    } else {
        let v = vector(value, warning);
        (v, v, false)
    };
    let convert = |v: [f32; 3]| match target {
        Target::Position => [v[0], -v[1], v[2]],
        Target::Rotation => v.map(f32::to_radians),
        Target::Scale => v.map(|x| x - 1.0),
    };
    Keyframe {
        time,
        pre: convert(pre),
        post: convert(post),
        catmull,
    }
}

fn vector(value: &Value, warning: &mut bool) -> [f32; 3] {
    if let Some(items) = value.as_array() {
        return std::array::from_fn(|i| items.get(i).map(|v| component(v, warning)).unwrap_or(0.0));
    }
    let x = component(value, warning);
    [x; 3]
}

fn component(value: &Value, warning: &mut bool) -> f32 {
    if let Some(n) = value.as_f64() {
        return n as f32;
    }
    if let Some(s) = value.as_str() {
        if let Ok(n) = s.trim().parse() {
            return n;
        }
        if let Some(n) = Expr::new(s).parse() {
            return n;
        }
        *warning = true;
    }
    0.0
}

struct Expr<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl<'a> Expr<'a> {
    fn new(s: &'a str) -> Self {
        Self {
            bytes: s.as_bytes(),
            at: 0,
        }
    }
    fn ws(&mut self) {
        while self.bytes.get(self.at).is_some_and(u8::is_ascii_whitespace) {
            self.at += 1;
        }
    }
    fn parse(mut self) -> Option<f32> {
        let v = self.sum()?;
        self.ws();
        (self.at == self.bytes.len()).then_some(v)
    }
    fn sum(&mut self) -> Option<f32> {
        let mut v = self.product()?;
        loop {
            self.ws();
            match self.bytes.get(self.at) {
                Some(b'+') => {
                    self.at += 1;
                    v += self.product()?;
                }
                Some(b'-') => {
                    self.at += 1;
                    v -= self.product()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn product(&mut self) -> Option<f32> {
        let mut v = self.atom()?;
        loop {
            self.ws();
            match self.bytes.get(self.at) {
                Some(b'*') => {
                    self.at += 1;
                    v *= self.atom()?;
                }
                Some(b'/') => {
                    self.at += 1;
                    v /= self.atom()?;
                }
                _ => return Some(v),
            }
        }
    }
    fn atom(&mut self) -> Option<f32> {
        self.ws();
        match self.bytes.get(self.at).copied()? {
            b'-' => {
                self.at += 1;
                Some(-self.atom()?)
            }
            b'(' => {
                self.at += 1;
                let v = self.sum()?;
                self.ws();
                (self.bytes.get(self.at) == Some(&b')')).then(|| {
                    self.at += 1;
                    v
                })
            }
            b'0'..=b'9' | b'.' => {
                let start = self.at;
                while self
                    .bytes
                    .get(self.at)
                    .is_some_and(|b| b.is_ascii_digit() || *b == b'.')
                {
                    self.at += 1;
                }
                std::str::from_utf8(&self.bytes[start..self.at])
                    .ok()?
                    .parse()
                    .ok()
            }
            _ if self.bytes[self.at..].starts_with(b"math.pi") => {
                self.at += 7;
                Some(std::f32::consts::PI)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bedrock_constant_channels() {
        let a = parse(&json!({"animations":{"a":{"bones":{"b":{"position":[1,2,3],"scale":2}}}}}))
            .unwrap();
        let c = &a["a"].bones[0].1;
        assert_eq!(c[0].keyframes[0].post, [1.0, -2.0, 3.0]);
        assert_eq!(c[1].keyframes[0].post, [1.0; 3]);
    }

    #[test]
    fn bedrock_timed_keyframes_convert() {
        let a = parse(&json!({"animations":{"a":{"bones":{"b":{"rotation":{"1":[0,90,0],"0.0":[0,0,0]},"position":{"0":[1,2,3]},"scale":{"0":2}}}}}})).unwrap();
        let c = &a["a"].bones[0].1;
        assert_eq!(c[0].keyframes[0].post, [1.0, -2.0, 3.0]);
        assert_eq!(c[1].keyframes[1].post[1], std::f32::consts::FRAC_PI_2);
        assert_eq!(c[1].keyframes[0].time, 0.0);
        assert_eq!(c[2].keyframes[0].post, [1.0; 3]);
    }

    #[test]
    fn bedrock_pre_post_catmull() {
        let a = parse(&json!({"animations":{"a":{"bones":{"b":{"position":{"0":{"pre":[1,2,3],"post":[4,5,6],"lerp_mode":"catmullrom"},"1":{"post":[7,8,9]}}}}}}})).unwrap();
        let k = &a["a"].bones[0].1[0].keyframes;
        assert_eq!(k[0].pre, [1.0, -2.0, 3.0]);
        assert_eq!(k[0].post, [4.0, -5.0, 6.0]);
        assert!(k[0].catmull);
        assert_eq!(k[1].pre, k[1].post);
    }

    #[test]
    fn bedrock_string_and_molang() {
        let a = parse(&json!({"animations":{"a":{"bones":{"b":{"position":["1.5","-(2*3)+1","math.sin(query.anim_time*90)"]}}}}})).unwrap();
        assert_eq!(a["a"].bones[0].1[0].keyframes[0].post, [1.5, 5.0, 0.0]);
    }

    #[test]
    fn bedrock_length_and_loop() {
        let a = parse(&json!({"animations":{"a":{"loop":true,"bones":{"b":{"position":{"2.5":[0,0,0]}}}},"b":{"animation_length":7,"loop":"hold_on_last_frame"},"c":{"loop":false}}})).unwrap();
        assert_eq!(a["a"].length, 2.5);
        assert!(a["a"].looping);
        assert_eq!(a["b"].length, 7.0);
        assert!(!a["b"].looping);
        assert!(!a["c"].looping);
    }

    #[test]
    fn bedrock_apply_offsets() {
        let a = parse(&json!({"animations":{"a":{"bones":{"child":{"position":[1,2,3],"rotation":[0,90,0],"scale":2}}}}})).unwrap();
        let child = crate::mobs::Part {
            name: "child".into(),
            pose: [0., 0., 0., 0., 0., 0., 1., 1., 1.],
            polygons: vec![],
            children: vec![],
        };
        let part = crate::mobs::Part {
            name: "root".into(),
            pose: [0., 0., 0., 0., 0., 0., 1., 1., 1.],
            polygons: vec![],
            children: vec![child.clone()],
        };
        let mut poses = crate::mobs::anim::Poses::new(&part);
        a["a"].apply(0, 1.0, &mut poses);
        let p = poses.pose(&child);
        assert_eq!(&p[..3], &[1.0, -2.0, 3.0]);
        assert!((p[4] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        assert_eq!(&p[6..], &[2.0, 2.0, 2.0]);
    }
}
