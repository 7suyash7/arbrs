-- A table to cache ERC20 token metadata.
CREATE TABLE tokens (
    address TEXT PRIMARY KEY NOT NULL,
    symbol TEXT NOT NULL,
    decimals INTEGER NOT NULL
);

-- A table for the bot's operational state.
CREATE TABLE bot_state (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);

-- Insert the initial starting block for scanning.
INSERT INTO bot_state (key, value) VALUES ('last_seen_block', '15000000');

-- The new, flexible pools table.
CREATE TABLE pools (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL UNIQUE,
    chain_id INTEGER NOT NULL,
    dex TEXT NOT NULL,
    -- V3-specific data, nullable for other types
    fee INTEGER,
    tick_spacing INTEGER
);

-- The junction table to link pools and tokens (many-to-many).
CREATE TABLE pool_tokens (
    pool_id INTEGER NOT NULL,
    token_address TEXT NOT NULL,
    FOREIGN KEY (pool_id) REFERENCES pools (id),
    FOREIGN KEY (token_address) REFERENCES tokens (address),
    -- Each pool can only have a specific token once.
    UNIQUE (pool_id, token_address)
);

-- Indexes to make our queries fast
CREATE INDEX idx_pools_address ON pools (address);
CREATE INDEX idx_pool_tokens_pool_id ON pool_tokens (pool_id);