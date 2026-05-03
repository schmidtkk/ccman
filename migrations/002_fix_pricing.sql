-- Fix duplicate pricing data caused by datetime('now') in seed inserts

-- Remove all duplicate pricing entries, keeping only the oldest for each provider+model
DELETE FROM pricing
WHERE id NOT IN (
    SELECT MIN(id)
    FROM pricing
    GROUP BY provider_id, model
);

-- Update remaining entries to have a fixed effective_date and is_current=1
UPDATE pricing
SET effective_date = '2026-01-01 00:00:00',
    is_current = 1;
