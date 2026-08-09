use gpui::{Context, Entity, FontWeight, Window, div, prelude::*, px};

use crate::components::{Badge, BadgeVariant};
use crate::review::{ClipReview, ReviewStatus, format_timecode};
use crate::theme::ActiveTheme;

pub(crate) struct Timeline {
    review: Entity<ClipReview>,
}

impl Timeline {
    pub(crate) fn new(review: Entity<ClipReview>, cx: &mut Context<Self>) -> Self {
        cx.observe(&review, |_, _, cx| cx.notify()).detach();
        Self { review }
    }
}

impl Render for Timeline {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let colors = window.theme().colors;
        let review = self.review.read(cx);
        let accepted = review
            .items
            .iter()
            .filter(|item| item.status == ReviewStatus::Accepted)
            .cloned()
            .collect::<Vec<_>>();
        let accepted_count = review.accepted_count();
        let timeline_duration = format_timecode(review.timeline.duration());
        let clip_label = if accepted_count == 1 { "clip" } else { "clips" };

        let clips = accepted
            .into_iter()
            .map(|item| {
                div()
                    .flex()
                    .min_w(px(150.0))
                    .flex_1()
                    .flex_col()
                    .justify_center()
                    .gap(px(3.0))
                    .h(px(48.0))
                    .px(px(9.0))
                    .rounded(px(5.0))
                    .bg(colors.chart_2.opacity(0.35))
                    .border_1()
                    .border_color(colors.chart_2.opacity(0.65))
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::MEDIUM)
                            .child(item.suggestion.title),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.muted_foreground)
                            .child(format!(
                                "Source {} · {}",
                                format_timecode(item.suggestion.candidate.start),
                                format_timecode(
                                    item.suggestion.candidate.duration().unwrap_or_else(|_| {
                                        time::RationalTime::new(0, 1).unwrap()
                                    })
                                )
                            )),
                    )
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(colors.card)
            .text_color(colors.card_foreground)
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .h(px(34.0))
                    .px(px(12.0))
                    .border_b_1()
                    .border_color(colors.border)
                    .child(
                        div()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Review timeline"),
                    )
                    .child(
                        Badge::new(format!(
                            "{accepted_count} {clip_label} · {timeline_duration}"
                        ))
                        .variant(BadgeVariant::Outline),
                    ),
            )
            .child(
                div()
                    .id("review-timeline-clips")
                    .flex()
                    .flex_1()
                    .items_center()
                    .gap(px(8.0))
                    .overflow_x_scroll()
                    .p(px(12.0))
                    .when(accepted_count == 0, |this| {
                        this.justify_center().child(
                            div()
                                .text_xs()
                                .text_color(colors.muted_foreground)
                                .child("Accept a suggestion to assemble a review cut."),
                        )
                    })
                    .children(clips),
            )
    }
}
