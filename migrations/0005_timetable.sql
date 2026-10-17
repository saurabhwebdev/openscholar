-- Fedena parity: subjects + weekly timetable (Mon-Fri x P1-P8).

CREATE TABLE IF NOT EXISTS subjects (
    id BIGSERIAL PRIMARY KEY,
    code TEXT NOT NULL UNIQUE,
    name TEXT NOT NULL,
    department TEXT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS class_subjects (
    id BIGSERIAL PRIMARY KEY,
    class_id BIGINT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    subject_id BIGINT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    teacher_id BIGINT NULL REFERENCES staff(id) ON DELETE SET NULL,
    UNIQUE (class_id, subject_id)
);
CREATE INDEX IF NOT EXISTS idx_class_subjects_class ON class_subjects (class_id);

CREATE TABLE IF NOT EXISTS periods (
    id BIGSERIAL PRIMARY KEY,
    weekday SMALLINT NOT NULL CHECK (weekday BETWEEN 0 AND 4),
    start_time TIME NOT NULL,
    end_time TIME NOT NULL,
    label TEXT NOT NULL,
    UNIQUE (weekday, start_time)
);

CREATE TABLE IF NOT EXISTS slots (
    id BIGSERIAL PRIMARY KEY,
    class_id BIGINT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    subject_id BIGINT NOT NULL REFERENCES subjects(id) ON DELETE CASCADE,
    period_id BIGINT NOT NULL REFERENCES periods(id) ON DELETE CASCADE,
    teacher_id BIGINT NULL REFERENCES staff(id) ON DELETE SET NULL,
    room TEXT NULL,
    UNIQUE (class_id, period_id)
);
CREATE INDEX IF NOT EXISTS idx_slots_teacher_period ON slots (teacher_id, period_id);
CREATE INDEX IF NOT EXISTS idx_slots_class ON slots (class_id);

INSERT INTO periods (weekday, start_time, end_time, label) VALUES
-- Monday
(0,'08:00','08:45','P1'),(0,'08:45','09:30','P2'),(0,'09:45','10:30','P3'),(0,'10:30','11:15','P4'),(0,'11:30','12:15','P5'),(0,'12:15','13:00','P6'),(0,'13:30','14:15','P7'),(0,'14:15','15:00','P8'),
-- Tuesday
(1,'08:00','08:45','P1'),(1,'08:45','09:30','P2'),(1,'09:45','10:30','P3'),(1,'10:30','11:15','P4'),(1,'11:30','12:15','P5'),(1,'12:15','13:00','P6'),(1,'13:30','14:15','P7'),(1,'14:15','15:00','P8'),
-- Wednesday
(2,'08:00','08:45','P1'),(2,'08:45','09:30','P2'),(2,'09:45','10:30','P3'),(2,'10:30','11:15','P4'),(2,'11:30','12:15','P5'),(2,'12:15','13:00','P6'),(2,'13:30','14:15','P7'),(2,'14:15','15:00','P8'),
-- Thursday
(3,'08:00','08:45','P1'),(3,'08:45','09:30','P2'),(3,'09:45','10:30','P3'),(3,'10:30','11:15','P4'),(3,'11:30','12:15','P5'),(3,'12:15','13:00','P6'),(3,'13:30','14:15','P7'),(3,'14:15','15:00','P8'),
-- Friday
(4,'08:00','08:45','P1'),(4,'08:45','09:30','P2'),(4,'09:45','10:30','P3'),(4,'10:30','11:15','P4'),(4,'11:30','12:15','P5'),(4,'12:15','13:00','P6'),(4,'13:30','14:15','P7'),(4,'14:15','15:00','P8')
ON CONFLICT DO NOTHING;
