# Privacy policy

Last updated: 11 October 2026

This policy covers **cloud.kkaii.xyz**, the thencloud server run by a private person in Poland (the "operator", "we"). If you use thencloud on a server someone else runs, their policy applies, not this one.

Questions, requests and complaints: **contact@kkaii.xyz**.

## The short version

Your files, their names and your folder structure are encrypted in your browser or app before they're sent. The server stores them encrypted and can't read them, and nor can we. We don't sell data, show ads or use analytics, and we never use your data to train AI models.

## What the server can't see

thencloud is end-to-end encrypted. The server never receives:

- your password (your device turns it into keys and sends only a key derived from it, which the server hashes again);
- your encryption keys;
- the contents or names of your files and folders, comments, notes, display name, profile picture, pronouns or app settings;
- the key in a public link (it sits after the `#`, which browsers never send to a server).

See [How the encryption works](../security/encryption.md) for the details.

## What the server stores

To run your account, the server keeps:

- **Account:** your username, a hash of the key derived from your password, your public keys, your encrypted private keys, your quota and how much of it you use, and whether you're an admin. Two-step sign-in adds a TOTP secret or passkey public keys.
- **Encrypted data:** file chunks, encrypted file and folder records, and encrypted versions, thumbnails, comments and app data, with their sizes.
- **Structure and times:** which encrypted item is inside which folder, who shared what with whom, public links, and times rounded to the hour (when something was created, changed or trashed).
- **Activity:** which account added, changed, moved or deleted which item (by id, not name), kept for 90 days.
- **Sessions:** a hash of each sign-in token, the device name your app sends (such as "Firefox on Linux"), and when it was created and last used.
- **Transfer counts:** how many encrypted bytes you upload and download each day, to apply limits.

The server uses your IP address in memory to limit repeated sign-in attempts and doesn't store it. It doesn't keep request logs with IP addresses.

## Google Drive (linked storage)

You can choose to link your Google Drive in Settings, under Linked storage. It's off unless you link it. If you do:

- **What we access:** the `drive.file` permission, which only reaches files thencloud itself created in your Drive (a folder named "thencloud" and the files in it). We can't see, open or change anything else in your Drive. We also read your Google account's email address, so Settings can show which account is linked, and your Drive's storage limit and usage, so we know how much space is left.
- **What we put there:** only encrypted chunks of your files, under random names. Google can see how much is stored and when it changes, never your file names or contents.
- **What we keep:** a refresh token for your Drive and the linked account's email address, both encrypted under a key that's kept apart from the database, and a record of which chunks are kept in your Drive.
- **How it's used:** only to store, read, move and delete your encrypted chunks, and to show you how much space you have. Nothing else.
- **Sharing:** Google user data is never sold, never shared with anyone else, never used for ads, and never used to train AI or machine learning models. No person reads it, except where you ask us to (for example, to help fix a problem with your account), for security reasons, or where the law requires it.
- **Removing access:** unlink the Drive in Settings (files kept only there are moved back to the server first), or revoke thencloud's access at [myaccount.google.com/permissions](https://myaccount.google.com/permissions). Unlinking deletes the token and email address from the server. Deleting your thencloud account deletes them too.

thencloud's use and transfer of information received from Google APIs adheres to the [Google API Services User Data Policy](https://developers.google.com/terms/api-services-user-data-policy), including the Limited Use requirements.

## Who else handles data

- **Cloudflare** carries traffic to the server through a Cloudflare Tunnel, so it sees your IP address and the encrypted traffic, as any network provider would. Its own [privacy policy](https://www.cloudflare.com/privacypolicy/) applies.
- **Google**, only if you link a Drive, as described above.

The server runs on hardware we operate in Poland. Nobody else gets your data.

## Why we process it (GDPR)

We process this data to provide the service you signed up for (Art. 6(1)(b) GDPR) and, for rate limiting, backups and security, because of our legitimate interest in keeping the service safe and working (Art. 6(1)(f)). Linking Google Drive is based on your choice to link it, which you can undo at any time.

## How long we keep it

- Your data stays until you delete it or delete your account.
- The trash is emptied after 30 days.
- Activity is deleted after 90 days.
- Expired sessions are deleted.
- Encrypted backups of the server are kept for up to 12 months, after which deleted data is gone from them too.

## Your rights

Under the GDPR you can ask to see, correct, delete or export your data, object to or restrict how it's processed, and withdraw consent. Most of this you can do yourself: Settings has "Export your data" and account deletion, and unlinking Google Drive. For anything else, write to contact@kkaii.xyz. You can also complain to the Polish data protection authority, the [Prezes Urzędu Ochrony Danych Osobowych](https://uodo.gov.pl).

## Cookies and local storage

thencloud uses no cookies for tracking and no analytics. Your browser keeps settings like the theme and language in local storage. If you tick "Keep me signed in on this browser", your session is kept in the browser's storage, encrypted, until you sign out.

## Children

The service isn't meant for children under 16.

## Changes

If this policy changes, the new version is published here with a new date. For significant changes we'll tell account holders before they take effect.
