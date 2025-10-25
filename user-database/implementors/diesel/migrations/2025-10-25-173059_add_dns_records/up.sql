CREATE TABLE dns_records (
    id BINARY(16) PRIMARY KEY NOT NULL,
    user_id BINARY(16) NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider_key TEXT NOT NULL,  -- Provider-specific key as string
    name TEXT NOT NULL,
    record_type INTEGER NOT NULL,
    content TEXT NOT NULL,
    ttl_seconds INTEGER NOT NULL,
    priority INTEGER NULL,
    created_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Indexes for performance
CREATE INDEX idx_dns_records_user_id ON dns_records(user_id);
CREATE INDEX idx_users_id ON users(id);
