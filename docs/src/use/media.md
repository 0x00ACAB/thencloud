# Music, videos, photos and more

Each library starts from a folder you pick. Only that folder's id is remembered (in your browser); the library itself is built in memory each time, by walking the folder and decrypting names and tags.

## Music

Pick a music folder and it's shown as albums, grouped by folder. Tags (ID3, FLAC, MP4) and covers are read in the browser as tracks stream. There's a queue, shuffle, playlists, media keys, and edits to track and album details, all kept in your encrypted app data. Long tracks remember where you left off.

Audiobooks (M4B) show their chapters and get a speed control.

On Android the app keeps playing with the screen off and shows the track on the lock screen.

## Videos

Pick a videos folder. Series, seasons and episodes are read from names like `Show S01E02 Title`, `1x02` or `Show/Season 1/02 Title`, and from MP4 and Matroska tags. Subtitles next to a video (`.srt`, `.vtt`, named after it) can be turned on in the player. Watch progress is saved.

## Photos

Pick a photos folder for a timeline by month (from the date the photo was taken, read from EXIF at upload, or else the file's time) and albums by folder.

## Notes

Pick a notes folder and its Markdown files become notes, edited inline and saved a moment after you stop typing. Notes can be pinned and searched.

## Books

EPUB and CBZ files open in a reader that remembers your page.

## Converting and PDF tools

**Convert** turns images, video and audio into other formats in the browser (canvas encoders, and ffmpeg.wasm for video and audio). **PDF tools** merge PDFs and rotate, reorder, remove or extract pages. Either way the result is a new encrypted file or a download; nothing is sent to the server unencrypted.

## Modules

Settings > Modules turns on optional sections, such as Health (weight, measurements and moods, kept in your encrypted app data). They're off by default.
