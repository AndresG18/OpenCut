# OpenCut Agent Workflow

## Studio first

- When the user asks to use OpenCut or edit a video, open the Studio/editor and leave the relevant project visible for review.
- Import the source media and approved modular clips before asking the user to assemble a timeline manually.
- Keep previewing separate from editing: choosing **Preview** must never add a clip to a timeline. Only **Add to timeline** may change the edit.
- Preserve alternate edits as separate scenes, timelines, or projects so one version never overwrites another.

## Andres' default editing workflow

- Follow [`ANDRES_VIDEO_EDITING_WORKFLOW.md`](./ANDRES_VIDEO_EDITING_WORKFLOW.md) for approved source ranges, narrative permutations, and export rules.
- Prefer confident, complete deliveries and remove stutters, filler, false starts, repeated lines, and mid-thought endings.
- Do not repeat the same proof point in one version.
- Default social exports to 9:16 for Reels, TikTok, and YouTube Shorts. Add readable captions after the spoken sequence is locked.
- Keep sound design restrained and preserve the original media unchanged.

## Rewrite architecture

- Keep editor-domain logic in the Rust crates and platform interaction in the app shell.
- The desktop Preview state is timeline-safe, but source-video playback still requires the rewrite's decode/render path to be connected. Do not describe placeholder preview UI as working video playback.
