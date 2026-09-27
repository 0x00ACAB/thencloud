-- Verified contacts: the public keys a user has checked by fingerprint,
-- encrypted under their master key. Opaque to the server.
ALTER TABLE users ADD COLUMN enc_contacts BLOB;
ALTER TABLE users ADD COLUMN contacts_revision INTEGER NOT NULL DEFAULT 0;
