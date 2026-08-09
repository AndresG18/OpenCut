use gpui::{Context, Window, div, prelude::*};

use crate::theme::ActiveTheme;

pub(crate) struct Preview;

impl Render for Preview {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let colors = window.theme().colors;

        div()
            .flex()
            .size_full()
            .items_center()
            .justify_center()
            .bg(colors.background)
            .child("Preview")
    }
}
