-- The node-media GC / boot reconciliation refcounts an asset's content by
-- `SELECT EXISTS(SELECT 1 FROM assets WHERE content_hash = ?)`. Without an
-- index this is a full table scan on every media remove / replace / boot sweep.
CREATE INDEX IF NOT EXISTS idx_assets__content_hash ON assets (content_hash);
