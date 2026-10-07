//! Weight and italic instances supported by a variable font. Other axes keep their defaults.

use rustybuzz::ttf_parser::Tag;

use crate::config::theme::{Face, FontStyle, Weight};

pub(crate) fn faces(font: &rustybuzz::Face<'_>, base: Face) -> Vec<Face> {
    let axis = |tag| {
        font.variation_axes()
            .into_iter()
            .find(|axis| axis.tag == Tag::from_bytes(tag))
    };
    let weight = axis(b"wght");
    let italic = axis(b"ital");
    let weights: Vec<_> = match weight {
        Some(axis) => (100..=900)
            .step_by(100)
            .filter(|&value| (axis.min_value..=axis.max_value).contains(&(value as f32)))
            .map(Weight::of_font)
            .collect(),
        None => vec![base.weight],
    };
    weights
        .into_iter()
        .flat_map(|weight| {
            [FontStyle::Normal, FontStyle::Italic]
                .into_iter()
                .filter_map(move |style| {
                    let supported = italic.map_or(style == base.style, |axis| {
                        let value = if style == FontStyle::Italic { 1.0 } else { 0.0 };
                        (axis.min_value..=axis.max_value).contains(&value)
                    });
                    supported.then_some(Face { weight, style })
                })
        })
        .collect()
}

pub(super) fn coordinates(font: &rustybuzz::Face<'_>, face: Face) -> Result<Vec<rustybuzz::Variation>, String> {
    let mut coordinates = Vec::new();
    for axis in font.variation_axes() {
        let value = if axis.tag == Tag::from_bytes(b"wght") {
            f32::from(face.weight.get())
        } else if axis.tag == Tag::from_bytes(b"ital") {
            if face.style == FontStyle::Italic { 1.0 } else { 0.0 }
        } else {
            continue;
        };
        if !(axis.min_value..=axis.max_value).contains(&value) {
            return Err(format!(
                "axis {} value {value} is outside {} to {}",
                axis.tag, axis.min_value, axis.max_value
            ));
        }
        coordinates.push(rustybuzz::Variation { tag: axis.tag, value });
    }
    Ok(coordinates)
}
