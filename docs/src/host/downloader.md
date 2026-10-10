# Video downloader

The video downloader lets people save a video from a link (YouTube and other sites yt-dlp supports) into their files. **It's the one feature where the server sees content**, so it's off until an admin turns it on in the Admin view, and the UI says so where it's used.

When used, the server runs yt-dlp (and ffmpeg to merge separate video and audio, which most YouTube videos need) for the link and streams the video to the browser, which encrypts and uploads it like any other file. The server sees the link and the video while it passes through.

To keep that as small as possible:

- Nothing is written to disk: only pipes, in a scratch directory that's deleted.
- Nothing is logged or cached.
- Only yt-dlp's site extractors run (no generic extractor).
- Links to private or local addresses are refused, and so is every connection yt-dlp makes after that: it goes through a proxy inside the server that resolves each host itself and only connects to public addresses, so a redirect or a name that answers differently the second time can't reach the server's own network.
- Videos are capped at `--downloader-max-bytes` (2 GiB by default).

Install `yt-dlp` and `ffmpeg` on the server, or point `--yt-dlp` and `--ffmpeg` at them. YouTube also needs a JavaScript runtime for yt-dlp, such as `deno`. The admin can also limit it to admins only.

With Docker, build the `with-downloader` target, which adds yt-dlp, ffmpeg and deno (pinned by version and checksum):

```sh
docker build --target with-downloader -t thencloud .
# or, with deploy/compose.yaml:
THENCLOUD_TARGET=with-downloader THENCLOUD_DOMAIN=cloud.example.com docker compose -f deploy/compose.yaml up -d --build
```

Sites change often and yt-dlp follows them. To update it, rebuild with a newer `--build-arg YT_DLP_VERSION=...` and its `--build-arg YT_DLP_SHA256=...` (the `yt-dlp` line of that release's `SHA2-256SUMS`).
