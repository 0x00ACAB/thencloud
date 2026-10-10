# Sharing

There are three ways to share: with another account, by public link, and by file drop (an upload-only link).

## With another person

Choose **Share** from the menu of a file or folder and enter their username. Before anything is shared you're shown their key **fingerprint**. Compare it with them over something other than thencloud (in person, a call, a message app you trust). Once it matches, mark them as verified.

Why: the server hands out public keys, so a malicious server could hand you its own key instead of theirs. The fingerprint check catches that. After the first check the key is pinned in your verified contacts, and if the server ever shows a different key for that person, sharing stops until you check again.

A share can be **read only** or **can edit**, and can have an expiry. Sharing a folder shares everything inside it.

The folder's key is sealed to their public key, with X25519 and ML-KEM-768 together, so a recording of the share stays closed unless both are broken.

**Removing a share** stops the server from serving the files to them. It doesn't re-encrypt anything, so someone who kept the keys could still decrypt a copy of the ciphertext they get elsewhere.

## Public links

A public link looks like:

```
https://cloud.example.com/s/<token>#<key>
```

The part after `#` is the key. Browsers never send it to the server, so the server only knows the token. Whoever has the whole link can open what it points to.

Options:

- **Password.** The link then carries a random secret after `#` instead of the key, and the key is wrapped under the secret and the password together. The server never sees the password; visitors prove they know it with a key derived from it.
- **Expiry.** The link stops working after a date.
- **Limit on opens.** The link stops working after it's been opened that many times.

**Public links** in the sidebar lists your links and lets you remove them.

## File drops

A file drop is an upload-only link to one of your folders. Visitors can add files but can't see what's there. Each file's key is sealed to your public key, so nobody but you can read them, the server included. The next time you open that folder, your browser takes the files in under a fresh key.

Anyone with the link (and the server) can add files to that folder, so remove the link when you're done collecting.

## Activity and comments

**Activity** in a folder's menu shows who added, changed, renamed, moved, trashed or restored what, to the hour. **Comments** on a file are encrypted under its key, so only people with access can read them.
