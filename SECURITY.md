# Security policy

thencloud's promise is that the server never sees your keys, passwords, file names or file contents. If you find a way to break that, or any other vulnerability, please tell us privately.

## Reporting

Use [GitHub's private vulnerability reporting](https://github.com/0x00ACAB/thencloud/security/advisories/new). Please include:
- what an attacker can do, and from where (a malicious server, another user, someone with a public link, a web page);
- steps or a proof of concept;
- the commit you tested.

Please don't open a public issue, and give us a reasonable time to fix it before you publish. We'll reply within a week.

## Scope

In scope, for example:
- anything that lets the server, or anyone else, learn plaintext names, contents, keys or passwords;
- a malicious server swapping, reordering or truncating ciphertext without the client noticing;
- a shared file or public link that runs script in the app's origin;
- authorisation bypasses (reaching nodes you don't own and weren't shared).

Known limitations are listed under "Threat model" in the [README](README.md#threat-model) and aren't vulnerabilities in themselves, for example that the web client is served by the server, or that the opt-in video downloader sees the videos it fetches.

## Supported versions

thencloud is early and has no stable releases yet; fixes go to `main`.
