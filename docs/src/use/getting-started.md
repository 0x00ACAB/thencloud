# Getting started

## Creating an account

Open your server's address and choose **Create account**. Depending on how the admin set it up, registration is open to anyone, needs an invite link, or is closed.

Your password is turned into keys in your browser (Argon2id, which takes a second or two on purpose). What the server gets is a key derived from it, which it hashes again, never the password itself.

> **There is no password reset.** The server can't decrypt your files, so it can't give them back to you if you forget the password. Make a [recovery key](account.md#recovery-key) right after signing up and keep it somewhere safe.

## Signing in

Sign in with your username and password, and a code or passkey if you've set up [two-step sign-in](account.md#two-step-sign-in). A passkey that supports PRF can also sign you in on its own, without the password.

Your keys live in the page's memory, so a reload asks for your password again. If that's too often on a computer only you use, tick **Keep me signed in on this browser**: the session and master key are then kept in the browser's storage, encrypted with a key no script can export. Anyone who can use that computer account can then open your files. Signing out deletes them.

## The first account on a new server

The first account becomes the admin. So that nobody else claims a fresh server first, creating it needs the **setup code** the server prints in its log on first start. See [Installing](../host/install.md#the-first-account).

## Finding your way around

- **My files** is your encrypted tree. Drop files or folders onto it to upload.
- **Shared with me** and **Shared by me** list shares in each direction; **Public links** lists your links.
- **Trash** keeps deleted things for 30 days (or what your admin set).
- **Settings** has your password, second step, recovery key, app passwords, devices, appearance, language and modules.

Most things have keyboard shortcuts; press `?` in the file list to see them.
