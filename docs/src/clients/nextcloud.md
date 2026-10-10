# Moving from Nextcloud

`thencloud import-nextcloud` copies files from a Nextcloud account over its WebDAV, encrypting them on your machine as they stream through. Folders, names and modification times are kept; nothing is written to disk on the way.

1. In Nextcloud, make an app password (Settings > Security > Devices & sessions).
2. [Sign the CLI in](cli.md#signing-in) to your thencloud server.
3. Run the import:

```sh
NEXTCLOUD_PASSWORD=... thencloud import-nextcloud https://nc.example.com alice                 # everything, into My files
NEXTCLOUD_PASSWORD=... thencloud import-nextcloud https://nc.example.com alice "From Nextcloud" --from Photos
```

It's safe to stop and run again: files already copied, with the same size and time, are skipped. A file that changes while it's being copied stops the import rather than being saved cut short.

Only the current files are copied. Shares, comments, versions and the trash on Nextcloud stay there.
