-- Link passwords in the key: the node key wrapped under a KEK made from the
-- password and the secret after `#` (handed out once the password is given),
-- and that secret sealed under the node key for the owner. password_hash is
-- now a hash of an auth key derived from the password, never the password.
ALTER TABLE public_links ADD COLUMN enc_link_key BLOB;
ALTER TABLE public_links ADD COLUMN enc_link_secret BLOB;
