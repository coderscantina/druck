//! A probe's choice of type: the book's theme, or one other theme, setting the passage in that theme's body face.

use gpui::prelude::*;
use gpui::{AnyElement, Context, Pixels, SharedString, div};

use super::model::{Item, Kind};
use super::panel::MaterialPanel;
use crate::tokens::Tokens;

#[derive(Default)]
pub struct ProbeChoice {}

impl ProbeChoice {
    pub fn item_opened(&mut self, _id: &str, _cx: &mut Context<MaterialPanel>) {}

    pub fn choice(&self, _tokens: &Tokens, _cx: &mut Context<MaterialPanel>) -> AnyElement {
        div().into_any_element()
    }

    /// The family, size, and line height a probe is set in while another theme is chosen.
    pub fn type_for(&self, item: &Item) -> Option<(SharedString, Pixels, Pixels)> {
        let _ = item.kind == Kind::Probes;
        None
    }

    pub fn note(&self, _item: &Item) -> Option<String> {
        None
    }

    #[cfg(any(debug_assertions, feature = "measure"))]
    pub fn choose_named(&mut self, _name: &str, _cx: &mut Context<MaterialPanel>) {}
}
