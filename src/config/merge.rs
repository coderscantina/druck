//! Layered JSON merging with per-value provenance.
//!
//! Objects merge recursively by field. Arrays, scalars, and `null` replace the earlier value
//! as a whole. Provenance records which layer supplied each value so resource paths keep
//! their origin and resolved-configuration errors can name the responsible layer.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

/// Index of a layer in resolution order.
pub type LayerIndex = usize;

#[derive(Debug, Default)]
pub struct Merged {
    pub value: Value,
    provenance: BTreeMap<String, LayerIndex>,
}

impl Merged {
    pub fn new(base: Value, layer: LayerIndex) -> Self {
        let mut merged = Self {
            value: Value::Null,
            provenance: BTreeMap::new(),
        };
        merged.apply(base, layer);
        merged
    }

    pub fn apply(&mut self, overlay: Value, layer: LayerIndex) {
        record_leaves(&overlay, &mut String::new(), layer, &mut self.provenance);
        merge(&mut self.value, overlay);
    }

    /// The layer that supplied the value at a JSON pointer, using the nearest recorded ancestor.
    pub fn source_of(&self, pointer: &str) -> Option<LayerIndex> {
        let mut path = pointer;
        loop {
            if let Some(&layer) = self.provenance.get(path) {
                return Some(layer);
            }
            path = &path[..path.rfind('/')?];
        }
    }
}

pub fn merge(base: &mut Value, overlay: Value) {
    match (base, overlay) {
        (Value::Object(base), Value::Object(overlay)) => merge_objects(base, overlay),
        (base, overlay) => *base = overlay,
    }
}

fn merge_objects(base: &mut Map<String, Value>, overlay: Map<String, Value>) {
    for (key, value) in overlay {
        match base.get_mut(&key) {
            Some(existing) => merge(existing, value),
            None => {
                base.insert(key, value);
            }
        }
    }
}

/// Records every non-object value, including whole arrays, under its JSON pointer.
fn record_leaves(value: &Value, pointer: &mut String, layer: LayerIndex, out: &mut BTreeMap<String, LayerIndex>) {
    let Value::Object(map) = value else {
        out.insert(pointer.clone(), layer);
        return;
    };
    for (key, child) in map {
        let len = pointer.len();
        pointer.push('/');
        pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
        record_leaves(child, pointer, layer, out);
        pointer.truncate(len);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn objects_merge_by_field_and_arrays_replace_whole() {
        let mut merged = Merged::new(json!({"a": {"x": 1, "y": 2}, "list": [1, 2, 3], "s": "base"}), 0);
        merged.apply(json!({"a": {"y": 20, "z": 30}, "list": [9]}), 1);
        assert_eq!(
            merged.value,
            json!({"a": {"x": 1, "y": 20, "z": 30}, "list": [9], "s": "base"})
        );
    }

    #[test]
    fn null_replaces_the_earlier_value_for_typed_validation_to_judge() {
        let mut merged = Merged::new(json!({"a": {"x": 1}}), 0);
        merged.apply(json!({"a": null}), 1);
        assert_eq!(merged.value, json!({"a": null}));
    }

    #[test]
    fn provenance_names_the_last_layer_supplying_a_value() {
        let mut merged = Merged::new(json!({"fonts": {"serif": {"regular": "a.otf"}}, "list": [1]}), 0);
        merged.apply(json!({"fonts": {"mine": {"regular": "b.otf"}}, "list": [2, 3]}), 1);
        merged.apply(json!({"fonts": {"serif": {"regular": "c.otf"}}}), 2);
        assert_eq!(merged.source_of("/fonts/mine/regular"), Some(1));
        assert_eq!(merged.source_of("/fonts/serif/regular"), Some(2));
        assert_eq!(merged.source_of("/list/1"), Some(1));
        assert_eq!(merged.source_of("/missing"), None);
    }
}
