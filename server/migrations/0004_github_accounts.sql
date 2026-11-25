-- Accounts now hold either an OAuth refresh token (Google) or an access token
-- (GitHub), so the column gets a neutral name.
ALTER TABLE accounts RENAME COLUMN refresh_token TO secret;

-- The API an account belongs to, for providers with several hosts (GitHub
-- Enterprise). NULL means the provider's public service.
ALTER TABLE accounts ADD COLUMN api_url TEXT;
