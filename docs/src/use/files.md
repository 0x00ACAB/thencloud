# Files and folders

## Uploading

Drag files or whole folders onto the file list, or use **Upload**. Files are encrypted in your browser and sent in 4 MiB pieces; an upload that's cut off picks up where it stopped. A name already in the folder becomes a new version of that file.

Before upload, photos can have their location and camera details removed (Settings > Files, "Location and camera details in photos"); only the orientation is kept.

The server sees roughly how big each file is. Contents are padded (everything under 16 KiB to 16 KiB, larger files by at most about 12%), so sizes are rough, but not hidden.

## Versions

Every upload of an existing file keeps the old version. By default a file keeps 10 versions, thinned by age: all from the last hour, then one per hour for a day, one per day for 30 days, one per week after that. Open **Version history** on a file to download or restore one, or, for text, to see the **Changes** between a version and the one before it. The diff is worked out in your browser.

## Trash

Deleting moves things to the trash, from where they can be restored. They're deleted for good after 30 days, or when you empty the trash.

## Downloading

Single files download directly; several files or a folder download as a zip, put together in the browser as it streams. Large files and zips stream through a service worker so they don't have to fit in memory.

## Finding things

Names are encrypted, so search runs in your browser over the names it has decrypted. The search box takes filters:

- `type:image`, `type:video`, `type:pdf` and so on;
- `tag:receipts` for files you tagged.

Searches can be saved and reopened from the sidebar. Searching **Inside files** finds words in text, Markdown and PDF files; the index of words is kept encrypted with your other app data.

## Tags, favourites and recent

**Add to favourites** puts a file or folder under Favourites. Tags are free text, set with **Tags** in a file's menu, and browsed in the Tags view. All of these are node ids kept in your encrypted app data; the server doesn't see which files you tagged or how.

## Previews

Images, video, audio, PDF, text and code, Markdown, CSV, Office documents (DOCX, ODT, XLSX, PPTX), EPUB and comic books (CBZ) open in the browser. A shared file is untrusted, so previews never run its scripts or load anything it points to: Markdown images aren't fetched, Office documents are shown as plain text and tables, and PDFs are drawn to a canvas.

Text and Markdown files you can write to can be edited in place. Saving uploads a new version; if someone else changed the file meanwhile, you're asked which to keep instead of one silently overwriting the other.

## Checking your files

Settings > **Check your files** downloads and decrypts everything you have and lists anything that fails, with its path. **Export your data** downloads My files as a zip together with your decrypted app data and contacts.
