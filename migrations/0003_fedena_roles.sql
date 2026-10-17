-- fedena parity foundation: parent/student roles + family links + record logins

ALTER TABLE users DROP CONSTRAINT IF EXISTS users_role_check;
ALTER TABLE users
    ADD CONSTRAINT users_role_check
    CHECK (role IN ('admin', 'teacher', 'staff', 'parent', 'student'));

CREATE TABLE IF NOT EXISTS parent_links (
    id             BIGSERIAL PRIMARY KEY,
    parent_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    student_id     BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    relation       TEXT NOT NULL DEFAULT 'guardian',
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (parent_user_id, student_id)
);
CREATE INDEX IF NOT EXISTS idx_parent_links_parent ON parent_links (parent_user_id);
CREATE INDEX IF NOT EXISTS idx_parent_links_student ON parent_links (student_id);

ALTER TABLE students ADD COLUMN IF NOT EXISTS user_id BIGINT REFERENCES users(id) ON DELETE SET NULL;
ALTER TABLE staff ADD COLUMN IF NOT EXISTS user_id BIGINT REFERENCES users(id) ON DELETE SET NULL;
CREATE INDEX IF NOT EXISTS idx_students_user_id ON students (user_id);
CREATE INDEX IF NOT EXISTS idx_staff_user_id ON staff (user_id);
