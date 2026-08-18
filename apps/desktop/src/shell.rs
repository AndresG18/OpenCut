use gpui::{App, Context, Entity, Window, div, prelude::*};

use crate::components::{Orientation, ResizablePanelGroup};
use crate::panels::{Browser, Inspector, Preview, Timeline};
use crate::review::ClipReview;
use crate::theme::ActiveTheme;

// The complete workspace is created once in `new`, not inline in `render`, so
// panels and divider positions keep their state across renders.
pub(crate) struct Shell {
    workspace: Entity<ResizablePanelGroup>,
}

impl Shell {
    pub(crate) fn new(cx: &mut App) -> Self {
        let review = cx.new(|_| ClipReview::from_environment_or_demo());
        let browser = cx.new(|cx| Browser::new(review.clone(), cx));
		let preview = cx.new(|cx| Preview::new(review.clone(), cx));
        let inspector = cx.new(|_| Inspector);
        let timeline = cx.new(|cx| Timeline::new(review, cx));

        // The preview consumes two thirds of the right-hand area, producing
        // the familiar 25% browser / 50% preview / 25% inspector layout.
        let preview_and_inspector = cx.new(|_| {
            ResizablePanelGroup::new(Orientation::Horizontal, preview, inspector)
                .initial_fraction(2.0 / 3.0)
                .minimum_fraction(0.15)
        });
        let upper_workspace = cx.new(|_| {
            ResizablePanelGroup::new(Orientation::Horizontal, browser, preview_and_inspector)
                .initial_fraction(0.25)
                .minimum_fraction(0.15)
        });
        let workspace = cx.new(|_| {
            ResizablePanelGroup::new(Orientation::Vertical, upper_workspace, timeline)
                .initial_fraction(2.0 / 3.0)
                .minimum_fraction(0.2)
        });

        Self { workspace }
    }
}

impl Render for Shell {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let colors = window.theme().colors;

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(colors.background)
            .text_color(colors.foreground)
            .child(self.workspace.clone())
    }
}
