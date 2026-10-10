# Reporting a vulnerability

Found a way for the server, or anyone else, to learn something it shouldn't? Please report it privately through [GitHub's private vulnerability reporting](https://github.com/0x00ACAB/thencloud/security/advisories/new), not a public issue.

Include:

- what an attacker can do, and from where (a malicious server, another user, someone with a public link, a web page);
- steps or a proof of concept;
- the commit or release you tested.

We'll reply within a week. Please give us a reasonable time to fix it before publishing.

In scope, for example:

- anything that lets the server or anyone else learn plaintext names, contents, keys or passwords;
- a malicious server swapping, reordering or truncating ciphertext without the client noticing;
- a shared file or public link that runs script in the app's origin;
- reaching nodes you don't own and weren't shared.

The [known limitations](threat-model.md#known-limitations) aren't vulnerabilities in themselves.
