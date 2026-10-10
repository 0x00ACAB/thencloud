# Linked storage (Google Drive)

People can link their own Google Drive to their thencloud account, either as a **mirror** (a second copy of everything kept on the server) or as **extra space** (new files can be kept there instead of on the server, on top of their quota). Each person chooses whether new files go to the server first or to their Drive first, and the storage meter shows both, the server's share in the accent colour and Google Drive in amber.

Only ciphertext goes to Google: the same encrypted chunks the server would keep, under random names in a "thencloud" folder. Google sees how much is stored and when it changes, never names or contents. The server makes the transfers (the web client talks only to the server), so it does connect to Google while this is on.

## Turning it on

1. In the [Google Cloud console](https://console.cloud.google.com/), create a project and enable the **Google Drive API**.
2. Under **Google Auth Platform**, set up the consent screen and add the scope `https://www.googleapis.com/auth/drive.file`. It only reaches files thencloud made, never anything else in someone's Drive. While the app is in "Testing", only the test users you add can link a Drive.
3. Create an **OAuth client ID** of type **Web application** with this authorised redirect URI (no JavaScript origins are needed):

   ```
   https://<your server>/api/storage/google/callback
   ```

4. Start the server with:

   ```sh
   THENCLOUD_GOOGLE_CLIENT_ID=...apps.googleusercontent.com
   THENCLOUD_GOOGLE_CLIENT_SECRET=...
   THENCLOUD_PUBLIC_ORIGIN=https://<your server>
   ```

   `--public-origin` makes the redirect URI the server sends match the one you registered, whatever host name your proxy passes on.

People then find it in Settings, under Linked storage. Linking opens Google's consent page in a pop-up.

## What the server keeps

- Each linked account's refresh token and address, sealed under `<data dir>/storage-token-key`, a key file outside the database. A copy of the database alone gives no access to anyone's Drive. Back that file up with the data directory; without it, people have to link their Drive again.
- Where each chunk kept in a Drive is (`remote_chunks`), and how much thencloud keeps in each account.

## Things to know

- **Backups** (`backup DEST`, S3 snapshots) hold the database and the server's own blobs. Files kept only in someone's Drive (extra space) stay there; a restored server reads them from the Drive again, as long as it has `storage-token-key`.
- `check` checks the server's own blobs; files kept only in a Drive aren't expected there.
- A mirror is filled in the background, a batch at a time, including files from before it was linked. If Google is down, uploads still go through and the mirror catches up later.
- Unlinking a mirror deletes its copies. An account used as extra space can't be unlinked while files are kept only there.
- If someone revokes access in their Google account, the server marks the link as broken and Settings asks them to link it again.
