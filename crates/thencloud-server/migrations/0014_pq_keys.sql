-- The post-quantum half of each user's keypair: an ML-KEM-768 public key,
-- and its seed wrapped under the master key. Keys sealed to a user who has
-- one are sealed with X25519 and ML-KEM together. Older accounts get one
-- the next time they sign in.
ALTER TABLE users ADD COLUMN pq_public_key BLOB;
ALTER TABLE users ADD COLUMN enc_pq_private_key BLOB;
