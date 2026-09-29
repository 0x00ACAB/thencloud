-- Pronouns and grammatical gender (see `encrypt_person_details`), encrypted
-- under the same key as the profile picture and display name.
ALTER TABLE users ADD COLUMN enc_person_details BLOB;
