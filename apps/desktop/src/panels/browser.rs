use gpui::{Context, Entity, FontWeight, Window, div, prelude::*, px};

use crate::components::{Badge, BadgeVariant, Button, ButtonSize, ButtonVariant};
use crate::review::{ClipReview, ReviewStatus, format_timecode};
use crate::theme::ActiveTheme;

pub(crate) struct Browser {
    review: Entity<ClipReview>,
}

impl Browser {
    pub(crate) fn new(review: Entity<ClipReview>, cx: &mut Context<Self>) -> Self {
        cx.observe(&review, |_, _, cx| cx.notify()).detach();
        Self { review }
    }
}

impl Render for Browser {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let colors = window.theme().colors;
        let review = self.review.read(_cx);
        let items = review.items.clone();
        let profile_label = review.profile_label.clone();
        let source_duration = format_timecode(review.source_duration);

        let cards = items
            .into_iter()
            .enumerate()
			.map(|(index, item)| {
                let candidate = &item.suggestion.candidate;
                let range = format!(
                    "{} – {}",
                    format_timecode(candidate.start),
                    format_timecode(candidate.end)
                );
                let score = format!("{}% match", (item.suggestion.overall_score * 100.0).round());
                let reason = item.suggestion.reason.clone();

				let mut actions = div().flex().items_center().gap(px(6.0));
				let review_for_preview = self.review.clone();
				actions = actions.child(
					Button::new(("preview-suggestion", index), "Preview")
						.variant(ButtonVariant::Ghost)
						.size(ButtonSize::XSmall)
						.on_click(move |_, _, cx| {
							review_for_preview.update(cx, |review, cx| {
								if review.preview(index) {
									cx.notify();
								}
							});
						}),
				);
				match item.status {
                    ReviewStatus::Pending | ReviewStatus::Skipped => {
                        let review_for_accept = self.review.clone();
                        actions = actions.child(
                            Button::new(("accept-suggestion", index), "Add to timeline")
                                .size(ButtonSize::XSmall)
                                .on_click(move |_, _, cx| {
                                    review_for_accept.update(cx, |review, cx| {
                                        if review.accept(index) {
                                            cx.notify();
                                        }
                                    });
                                }),
                        );
                        if item.status == ReviewStatus::Pending {
                            let review_for_skip = self.review.clone();
                            actions = actions.child(
                                Button::new(("skip-suggestion", index), "Skip")
                                    .variant(ButtonVariant::Ghost)
                                    .size(ButtonSize::XSmall)
                                    .on_click(move |_, _, cx| {
                                        review_for_skip.update(cx, |review, cx| {
                                            if review.skip(index) {
                                                cx.notify();
                                            }
                                        });
                                    }),
                            );
                        } else {
                            actions =
                                actions.child(Badge::new("Skipped").variant(BadgeVariant::Outline));
                        }
                    }
                    ReviewStatus::Accepted => {
                        actions = actions
                            .child(Badge::new("In timeline").variant(BadgeVariant::Secondary));
                    }
                }

                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .p(px(10.0))
                    .rounded(window.theme().radius)
                    .border_1()
                    .border_color(colors.sidebar_border)
                    .bg(colors.card)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(8.0))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(item.suggestion.title),
                            )
                            .child(Badge::new(score).variant(BadgeVariant::Outline)),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.muted_foreground)
                            .child(range),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.muted_foreground)
                            .child(reason),
                    )
                    .child(actions)
                    .into_any_element()
            })
            .collect::<Vec<_>>();

        div()
            .flex()
            .flex_col()
            .size_full()
            .border_r_1()
            .border_color(colors.sidebar_border)
            .bg(colors.sidebar)
            .text_color(colors.sidebar_foreground)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(7.0))
                    .p(px(12.0))
                    .border_b_1()
                    .border_color(colors.sidebar_border)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_between()
                            .child(
                                div()
                                    .text_sm()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child("AI Clip Finder"),
                            )
                            .child(Badge::new("Core ready").variant(BadgeVariant::Secondary)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(colors.muted_foreground)
                            .child(format!("{profile_label} · 9:16 · {source_duration} source")),
                    )
                    .child(
                        div()
                            .text_size(px(10.0))
                            .text_color(colors.muted_foreground)
                            .child("Review ranked moments before anything changes the timeline."),
                    ),
            )
            .child(
                div()
                    .id("clip-finder-suggestions")
                    .flex()
                    .flex_1()
                    .flex_col()
                    .gap(px(8.0))
                    .overflow_y_scroll()
                    .p(px(10.0))
                    .children(cards),
            )
    }
}
