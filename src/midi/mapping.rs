//! Maps MIDI control-change messages onto param values, plus the "learn" step that creates a
//! binding from the next knob the user touches.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::shader::params::{ParamSpec, ParamType, ParamValue};

/// One controller knob: a CC number on a MIDI channel (1..16).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct CcKey {
    pub channel: u8,
    pub cc: u8,
}

impl CcKey {
    pub fn label(&self) -> String {
        format!("CC {} ch {}", self.cc, self.channel)
    }
}

/// A received control-change message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CcMessage {
    pub channel: u8,
    pub cc: u8,
    /// 0..=127
    pub value: u8,
}

impl CcMessage {
    pub fn key(&self) -> CcKey {
        CcKey { channel: self.channel, cc: self.cc }
    }
}

/// Decodes a raw MIDI message if it is a control change; everything else is ignored.
pub fn parse_message(bytes: &[u8]) -> Option<CcMessage> {
    match bytes {
        [status, cc, value, ..] if status & 0xF0 == 0xB0 => {
            Some(CcMessage { channel: (status & 0x0F) + 1, cc: cc & 0x7F, value: value & 0x7F })
        }
        _ => None,
    }
}

/// Spreads a 7-bit controller value across the param's range (0..1 without one). Only scalar
/// numeric params are mappable.
pub fn cc_to_value(spec: &ParamSpec, value: u8) -> Option<ParamValue> {
    let t = f64::from(value.min(127)) / 127.0;
    let (lo, hi) = spec.range.unwrap_or((0.0, 1.0));
    let x = lo + t * (hi - lo);
    match spec.ty {
        ParamType::F32 => Some(ParamValue::F32(x as f32)),
        ParamType::I32 => Some(ParamValue::I32(x.round() as i32)),
        _ => None,
    }
}

/// Param name to knob, plus the transient "waiting for a knob" state of MIDI learn.
#[derive(Serialize, Deserialize, Clone, Default, PartialEq, Debug)]
pub struct MidiMap {
    pub bindings: BTreeMap<String, CcKey>,
    #[serde(skip)]
    learning: Option<String>,
}

impl MidiMap {
    pub fn from_bindings(bindings: BTreeMap<String, CcKey>) -> Self {
        MidiMap { bindings, learning: None }
    }

    /// The next CC message binds its knob to `param`.
    pub fn learn(&mut self, param: &str) {
        self.learning = Some(param.to_string());
    }

    pub fn cancel_learn(&mut self) {
        self.learning = None;
    }

    pub fn learning(&self) -> Option<&str> {
        self.learning.as_deref()
    }

    /// Completes a pending learn with this message. Returns the param that was bound, if any.
    /// A knob drives one param, so any older binding of the same knob is dropped.
    pub fn on_message(&mut self, msg: CcMessage) -> Option<String> {
        let param = self.learning.take()?;
        let key = msg.key();
        self.bindings.retain(|_, k| *k != key);
        self.bindings.insert(param.clone(), key);
        Some(param)
    }
}

/// Writes the message's value into every param bound to its knob; returns the names changed.
pub fn apply_cc(
    specs: &[ParamSpec],
    values: &mut BTreeMap<String, ParamValue>,
    map: &MidiMap,
    msg: CcMessage,
) -> Vec<String> {
    let key = msg.key();
    let mut changed = Vec::new();
    for (name, bound) in &map.bindings {
        if *bound != key {
            continue;
        }
        if let Some(spec) = specs.iter().find(|s| &s.name == name)
            && let Some(v) = cc_to_value(spec, msg.value)
        {
            values.insert(name.clone(), v);
            changed.push(name.clone());
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shader::params::{ParamSpec, ParamType, ParamValue};
    use std::collections::BTreeMap;

    fn f32_spec(name: &str, range: Option<(f64, f64)>) -> ParamSpec {
        ParamSpec { name: name.into(), ty: ParamType::F32, default: ParamValue::F32(0.0), range, color: false }
    }

    fn i32_spec(name: &str, range: Option<(f64, f64)>) -> ParamSpec {
        ParamSpec { name: name.into(), ty: ParamType::I32, default: ParamValue::I32(0), range, color: false }
    }

    #[test]
    fn parses_control_change_messages_and_ignores_the_rest() {
        assert_eq!(parse_message(&[0xB0, 7, 100]), Some(CcMessage { channel: 1, cc: 7, value: 100 }));
        assert_eq!(parse_message(&[0xB9, 1, 0]), Some(CcMessage { channel: 10, cc: 1, value: 0 }));
        assert_eq!(parse_message(&[0x90, 60, 100]), None, "note on is not a CC");
        assert_eq!(parse_message(&[0xB0, 7]), None, "truncated");
        assert_eq!(parse_message(&[]), None);
    }

    #[test]
    fn cc_value_spans_the_params_range() {
        let spec = f32_spec("fade", Some((1.0, 200.0)));
        assert_eq!(cc_to_value(&spec, 0), Some(ParamValue::F32(1.0)));
        assert_eq!(cc_to_value(&spec, 127), Some(ParamValue::F32(200.0)));
        let ParamValue::F32(mid) = cc_to_value(&spec, 64).unwrap() else { panic!() };
        assert!((mid - 101.28).abs() < 0.1, "{mid}");
    }

    #[test]
    fn cc_value_without_a_range_uses_zero_to_one_and_ints_round() {
        assert_eq!(cc_to_value(&f32_spec("k", None), 127), Some(ParamValue::F32(1.0)));
        assert_eq!(cc_to_value(&i32_spec("n", Some((0.0, 10.0))), 127), Some(ParamValue::I32(10)));
        assert_eq!(cc_to_value(&i32_spec("n", Some((0.0, 10.0))), 64), Some(ParamValue::I32(5)));
        let v3 = ParamSpec {
            name: "c".into(),
            ty: ParamType::Vec3,
            default: ParamValue::Vec3([0.0; 3]),
            range: None,
            color: true,
        };
        assert_eq!(cc_to_value(&v3, 10), None, "vectors are not mappable");
    }

    #[test]
    fn apply_cc_updates_only_bound_params_and_reports_them() {
        let specs = vec![f32_spec("a", Some((0.0, 10.0))), f32_spec("b", Some((0.0, 10.0)))];
        let mut values: BTreeMap<String, ParamValue> =
            [("a".to_string(), ParamValue::F32(1.0)), ("b".to_string(), ParamValue::F32(1.0))].into();
        let mut map = MidiMap::default();
        map.bindings.insert("a".into(), CcKey { channel: 1, cc: 20 });
        let changed = apply_cc(&specs, &mut values, &map, CcMessage { channel: 1, cc: 20, value: 127 });
        assert_eq!(changed, vec!["a".to_string()]);
        assert_eq!(values["a"], ParamValue::F32(10.0));
        assert_eq!(values["b"], ParamValue::F32(1.0));
        // Same CC number on another channel is a different knob.
        let changed = apply_cc(&specs, &mut values, &map, CcMessage { channel: 2, cc: 20, value: 0 });
        assert!(changed.is_empty());
        assert_eq!(values["a"], ParamValue::F32(10.0));
    }

    #[test]
    fn learning_binds_the_next_cc_to_the_chosen_param_and_replaces_old_bindings() {
        let mut map = MidiMap::default();
        map.bindings.insert("other".into(), CcKey { channel: 1, cc: 20 });
        map.learn("fade");
        assert!(map.learning().is_some());
        let bound = map.on_message(CcMessage { channel: 1, cc: 20, value: 5 });
        assert_eq!(bound.as_deref(), Some("fade"));
        assert_eq!(map.learning(), None, "learn mode ends after one message");
        assert_eq!(map.bindings.get("fade"), Some(&CcKey { channel: 1, cc: 20 }));
        assert_eq!(map.bindings.get("other"), None, "one knob drives one param");
        assert_eq!(map.on_message(CcMessage { channel: 1, cc: 21, value: 5 }), None);
    }

    #[test]
    fn bindings_round_trip_through_toml() {
        let mut map = MidiMap::default();
        map.bindings.insert("fade".into(), CcKey { channel: 1, cc: 74 });
        let text = toml::to_string(&map.bindings).unwrap();
        assert!(text.contains("cc = 74"), "{text}");
        let back: BTreeMap<String, CcKey> = toml::from_str(&text).unwrap();
        assert_eq!(back, map.bindings);
        assert_eq!(CcKey { channel: 1, cc: 74 }.label(), "CC 74 ch 1");
    }
}
