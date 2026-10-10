# Video downloader

The video downloader lets people save a video from a link (YouTube and other sites yt-dlp supports) into their files. **It's the one feature where the server sees content**, so it's off until an admin turns it on in the Admin view, and the UI says so where it's used.

When used, the server runs yt-dlp (and ffmpeg to merge separate video and audio, which most YouTube videos need) for the link and streams the video to the browser, which encrypts and uploads it like any other file. The server sees the link and the video while it passes through.

To keep that as small as possible:

- Nothing is written to disk: only pipes, in a scratch directory that's deleted.
- Nothing is logged or cached.
- Only yt-dlp's site extractors run (no generic extractor).
- Links to private or local addresses are refused.
- Videos are capped at `--downloader-max-bytes` (2 GiB by default).

Install `yt-dlp` and `ffmpeg` on the server, or point `--yt-dlp` and `--ffmpeg` at them. The admin can also limit it to admins only.
