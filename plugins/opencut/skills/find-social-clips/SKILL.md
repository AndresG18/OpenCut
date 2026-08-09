---
name: find-social-clips
description: Find, score, rank, and package social-ready clips from a local time-coded transcript using OpenCut MCP tools and the current agent model. Use when a user asks to turn a long recording, podcast, interview, stream, or transcript into TikTok, Reel, Shorts, or custom-duration clip suggestions for human review in OpenCut.
---

# Find Social Clips

Use the active Codex or Claude model as the scorer. Never ask for, copy, or store a Codex, ChatGPT, Claude, or Claude.ai subscription credential. The MCP server performs deterministic local work and does not call an AI API.

## Prepare the request

Obtain the transcript path, desired clip length, destination or aspect ratio, number of suggestions, and editorial goal. Make a reasonable profile from the user's words when these are clear. Useful starting profiles are:

- Quick Reel: 15,000–30,000 ms, target 20,000 ms, 9:16.
- Standard short: 30,000–60,000 ms, target 45,000 ms, 9:16.
- User-requested 61-second clip: 61,000–75,000 ms, target 65,000 ms, 9:16.

Treat platform policies and monetization rules as changeable. Use the user's requested duration without presenting it as current platform policy unless verified separately.

The transcript JSON must be an array, or an object with a `segments` array, using exact integer milliseconds:

```json
[
  {
    "start_ms": 0,
    "end_ms": 4200,
    "text": "The transcript text.",
    "speaker": "Host"
  }
]
```

Accept camel-case `startMs` and `endMs` too. Do not convert or round timestamps unless the user asks for transcript normalization.

## Run discovery

1. Call `start_clip_discovery` with the transcript, prompt, and profile. Keep the defaults of 300 candidates, 20 candidates per batch, 40,000 transcript characters per batch, 35% maximum overlap, and 8 suggestions unless the user asks otherwise.
2. Note that starting another run replaces the current in-memory session. Finish or intentionally abandon the active run before restarting.
3. Call `get_candidate_batch` for every zero-based batch index. Repeating an index is safe.
4. Judge each candidate only from the supplied transcript and request. Do not invent visual details, speaker identity, or context that is absent.
5. Score `hook`, `relevance`, `coherence`, and `standalone` independently from 0 to 1. Give each candidate a concise title, a specific reason, and up to 12 useful keywords.
6. Call `submit_candidate_evaluations` once per batch. A submission is atomic. Do not resubmit candidate IDs that were accepted.
7. Call `clip_discovery_status` after interruptions or tool errors and continue only the listed unevaluated batches.

Score every candidate before finalizing. This prevents early transcript sections from getting an unfair advantage on long recordings.

## Create the review queue

Call `finalize_clip_discovery` after status reports `ready_to_finalize: true`. Use default local ranking weights unless the user requests a different priority. Provide an absolute `.json` output path when the user wants an OpenCut review package.

Never set `overwrite: true` without explicit user approval for that exact path. Use `allow_partial: true` only when the user knowingly accepts an incomplete search.

The package contains suggestions, not accepted edits. Make clear that the user still reviews each clip in OpenCut before it enters the timeline.

To load a package in the current desktop prototype, launch from the OpenCut repository with:

```sh
OPENCUT_REVIEW_PACKAGE=/absolute/path/to/review.json cargo run -p opencut-desktop
```

Run that command only when the user asked to open the result. Otherwise return the package path, top suggestions, duration profile, and any partial-run warning.
