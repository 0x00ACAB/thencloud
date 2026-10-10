# Upgrading

1. [Back up](backups.md).
2. Replace the binary and web client, or pull the new image.
3. Restart. Database migrations run on start.

From 1.0 on, a newer server opens everything an older one wrote: the database, files, public links people already sent, and older desktop, Android and CLI clients. Migrations only add, and ciphertext formats never change in place; a new format gets a new version byte and readers keep accepting the old ones.

Don't go back to an older version after a newer one has started on the same data: an older server refuses a database with migrations it doesn't know.

Read the [changelog](../reference/changelog.md) before upgrading. Each release's notes are the same text.

After upgrading, people's browsers notice the new web client: the service worker shows a notice with the old and new version before opening the changed page. That's expected after an upgrade, and the same notice is how users would notice an unexpected change. See [Checking a release](../security/verify.md).
