-- How a session was signed in, so the owner can be told about new sign-ins
-- ("with a passkey", "with the recovery key"). NULL for sessions from
-- before this column.
ALTER TABLE sessions ADD COLUMN method TEXT;
