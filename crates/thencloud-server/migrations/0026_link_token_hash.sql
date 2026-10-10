-- Public links are looked up by the SHA-256 of their token, and the token
-- itself is kept sealed under a key outside the database (see
-- src/link_tokens.rs), so a copy of the database doesn't hold tokens that
-- open links. `token` stays for its NOT NULL UNIQUE constraint and holds a
-- placeholder once a row is sealed.
ALTER TABLE public_links ADD COLUMN token_hash BLOB;
ALTER TABLE public_links ADD COLUMN enc_token BLOB;
CREATE UNIQUE INDEX public_links_token_hash ON public_links(token_hash);
