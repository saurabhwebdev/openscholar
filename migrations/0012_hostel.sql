CREATE TABLE IF NOT EXISTS hostel_rooms (id BIGSERIAL PRIMARY KEY, name TEXT NOT NULL, block TEXT NULL, capacity INT NOT NULL DEFAULT 1 CHECK (capacity>0), created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE TABLE IF NOT EXISTS hostel_allocations (id BIGSERIAL PRIMARY KEY, student_id BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE, room_id BIGINT NOT NULL REFERENCES hostel_rooms(id) ON DELETE CASCADE, bed_label TEXT NULL, allocated_on DATE NOT NULL DEFAULT CURRENT_DATE, released_on DATE NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX IF NOT EXISTS idx_alloc_student ON hostel_allocations (student_id);
CREATE INDEX IF NOT EXISTS idx_alloc_room ON hostel_allocations (room_id);
