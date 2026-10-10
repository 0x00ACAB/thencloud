# Keeping your account safe

All of these are in Settings.

## Two-step sign-in

Turn on an **authenticator app** (TOTP) or add a **passkey**. With either, a correct password only gets a short-lived ticket; your wrapped master key is handed out once the code or passkey checks out.

A passkey whose authenticator supports PRF can also sign in on its own. It then keeps its own wrapped copy of your master key, which only that passkey can unwrap. Each passkey belongs to the host name it was made on, so passkeys don't work in the desktop and Android apps.

## Recovery key

A recovery key is 256 random bits, shown once as 11 groups of 5 characters (with a checksum, so typos are caught). It's the only way back in if you forget your password. With it and your username, **Forgot your password?** on the sign-in page lets you set a new password; every session is signed out.

Anyone with your recovery key and username can take over the account, so keep it as private as your password. Making, replacing or removing it needs your current password.

## App passwords

App passwords are for the [command line](../clients/cli.md) and other devices. Each is a random secret, shown once, with its own wrapped copy of your master key. They can be **read only**, survive password changes, and skip the second step. Revoking one signs out every session it started.

## Devices

**Devices** lists your sessions; sign out any you don't recognise.

## Fingerprint

**Your key fingerprint** is in Settings. It's what other people compare before sharing with you.

## Profile

A profile picture, display name and pronouns are encrypted under a key that's shared only with people you share with (or who share with you). Since anyone can pick any display name, the username is always shown after it.
