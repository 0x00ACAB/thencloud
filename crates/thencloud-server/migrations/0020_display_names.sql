-- A display name (see `encrypt_display_name`), encrypted under the same key
-- as the profile picture and handed out with it.
ALTER TABLE users ADD COLUMN enc_display_name BLOB;
