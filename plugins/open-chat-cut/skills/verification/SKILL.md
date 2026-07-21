---
name: verification
description: "Verify OpenChatCut project integrity and visual results. Use after meaningful edits or before delivery to check project revisions, missing media, transcript anchors, A/V sync, captions, motion graphics, representative frames, export compatibility, or persistent job outcomes."
---

# Verify an OpenChatCut Project

1. Call `read_project` and pin the revision under review.
2. Call `validate_project`; separate blockers, warnings, and informational findings.
3. Choose bounded frame times covering the opening, edits/cuts, dense captions, graphics transitions, generated media, and ending.
4. Call `render_preview_frames` and inspect every returned frame rather than assuming successful rendering looks correct.
5. For speech edits, include both sides of representative cut boundaries and verify linked audio/video/caption anchors remain within one frame.
6. Track any validation or preview jobs to a terminal state.
7. Re-read the project. If its revision changed during verification, label results stale and repeat against the new revision.

Never suppress a daemon warning or infer support from a configured provider name. If validation or preview is unavailable, report the missing evidence and do not claim the project passed.
