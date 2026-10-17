-- Concurrency backstops for check-then-insert races found in the audit.

-- A teacher can't be in two places in the same period (teacher_clash was a
-- pre-insert check with a TOCTOU window). Partial: slots may have no teacher.
CREATE UNIQUE INDEX IF NOT EXISTS slots_teacher_period_uniq
    ON slots (teacher_id, period_id)
    WHERE teacher_id IS NOT NULL;

-- Two current occupants of a room can't share a bed label.
CREATE UNIQUE INDEX IF NOT EXISTS hostel_room_bed_open_uniq
    ON hostel_allocations (room_id, bed_label)
    WHERE released_on IS NULL;
