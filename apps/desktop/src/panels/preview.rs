use gpui::{Context, Entity, FontWeight, Window, div, prelude::*, px};

use crate::components::{Badge, BadgeVariant};
use crate::review::{ClipReview, format_timecode};
use crate::theme::ActiveTheme;

pub(crate) struct Preview {
	review: Entity<ClipReview>,
}

impl Preview {
	pub(crate) fn new(review: Entity<ClipReview>, cx: &mut Context<Self>) -> Self {
		cx.observe(&review, |_, _, cx| cx.notify()).detach();
		Self { review }
	}
}

impl Render for Preview {
	fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
		let colors = window.theme().colors;
		let selected = self.review.read(cx).previewed_item().cloned();
		let has_selected = selected.is_some();

		div()
			.flex()
			.flex_col()
			.size_full()
			.bg(colors.background)
			.text_color(colors.foreground)
			.when_some(selected, |this, item| {
				let candidate = item.suggestion.candidate;
				let range = format!(
					"{} – {}",
					format_timecode(candidate.start),
					format_timecode(candidate.end)
				);

				this.child(
					div()
						.flex()
						.items_center()
						.justify_between()
						.gap(px(12.0))
						.px(px(14.0))
						.py(px(10.0))
						.border_b_1()
						.border_color(colors.border)
						.child(
							div()
								.flex()
								.flex_col()
								.gap(px(2.0))
								.child(
									div()
										.text_sm()
										.font_weight(FontWeight::SEMIBOLD)
										.child(item.suggestion.title),
								)
								.child(
									div()
										.text_xs()
										.text_color(colors.muted_foreground)
										.child(range),
								),
						)
						.child(Badge::new("Preview only").variant(BadgeVariant::Secondary)),
				)
				.child(
					div()
						.flex()
						.flex_1()
						.flex_col()
						.items_center()
						.justify_center()
						.gap(px(12.0))
						.px(px(28.0))
						.child(
							div()
								.flex()
								.w_full()
								.max_w(px(440.0))
								.h(px(220.0))
								.items_center()
								.justify_center()
								.rounded(window.theme().radius)
								.border_1()
								.border_color(colors.border)
								.bg(colors.muted)
								.text_color(colors.muted_foreground)
								.text_sm()
								.child("Source playback will appear here"),
						)
						.child(
							div()
								.max_w(px(440.0))
								.text_center()
								.text_xs()
								.text_color(colors.muted_foreground)
								.child(format!(
									"Reviewing this candidate does not add it to the timeline. {}",
									item.suggestion.reason
								)),
						),
				)
			})
			.when(!has_selected, |this| {
				this.items_center().justify_center().child(
					div()
						.text_center()
						.text_sm()
						.text_color(colors.muted_foreground)
						.child("Choose Preview on a suggested clip to inspect it before adding it."),
				)
			})
    }
}
