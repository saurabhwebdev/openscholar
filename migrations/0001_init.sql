-- open school: initial schema

CREATE TABLE IF NOT EXISTS users (
    id            BIGSERIAL PRIMARY KEY,
    email         TEXT NOT NULL UNIQUE,
    password_hash TEXT NOT NULL,
    role          TEXT NOT NULL CHECK (role IN ('admin', 'teacher', 'staff')),
    full_name     TEXT NOT NULL,
    active        BOOLEAN NOT NULL DEFAULT TRUE,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS sessions (
    id         UUID PRIMARY KEY,
    user_id    BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_sessions_user_id ON sessions (user_id);

CREATE TABLE IF NOT EXISTS students (
    id             BIGSERIAL PRIMARY KEY,
    admission_no   TEXT NOT NULL UNIQUE,
    first_name     TEXT NOT NULL,
    last_name      TEXT NOT NULL,
    date_of_birth  DATE,
    gender         TEXT CHECK (gender IN ('male', 'female', 'other')),
    guardian_name  TEXT,
    guardian_phone TEXT,
    guardian_email TEXT,
    address        TEXT,
    status         TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive', 'graduated')),
    enrolled_on    DATE NOT NULL DEFAULT CURRENT_DATE,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_students_name ON students (last_name, first_name);

CREATE TABLE IF NOT EXISTS staff (
    id          BIGSERIAL PRIMARY KEY,
    employee_no TEXT NOT NULL UNIQUE,
    first_name  TEXT NOT NULL,
    last_name   TEXT NOT NULL,
    email       TEXT,
    phone       TEXT,
    job_title   TEXT,
    department  TEXT,
    hired_on    DATE,
    status      TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active', 'inactive')),
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_staff_name ON staff (last_name, first_name);

CREATE TABLE IF NOT EXISTS school_years (
    id         BIGSERIAL PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    start_date DATE NOT NULL,
    end_date   DATE NOT NULL,
    is_current BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE TABLE IF NOT EXISTS classes (
    id             BIGSERIAL PRIMARY KEY,
    school_year_id BIGINT NOT NULL REFERENCES school_years(id) ON DELETE CASCADE,
    name           TEXT NOT NULL,
    grade_level    TEXT,
    room           TEXT,
    teacher_id     BIGINT REFERENCES staff(id) ON DELETE SET NULL,
    capacity       INT,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (school_year_id, name)
);
CREATE INDEX IF NOT EXISTS idx_classes_teacher ON classes (teacher_id);

CREATE TABLE IF NOT EXISTS enrollments (
    id          BIGSERIAL PRIMARY KEY,
    student_id  BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    class_id    BIGINT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    enrolled_on DATE NOT NULL DEFAULT CURRENT_DATE,
    UNIQUE (student_id, class_id)
);
CREATE INDEX IF NOT EXISTS idx_enrollments_class ON enrollments (class_id);

CREATE TABLE IF NOT EXISTS attendance (
    id          BIGSERIAL PRIMARY KEY,
    student_id  BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    class_id    BIGINT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    date        DATE NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('present', 'absent', 'late', 'excused')),
    note        TEXT,
    recorded_by BIGINT REFERENCES users(id) ON DELETE SET NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (student_id, class_id, date)
);
CREATE INDEX IF NOT EXISTS idx_attendance_class_date ON attendance (class_id, date);

CREATE TABLE IF NOT EXISTS assessments (
    id         BIGSERIAL PRIMARY KEY,
    class_id   BIGINT NOT NULL REFERENCES classes(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    kind       TEXT NOT NULL DEFAULT 'exam' CHECK (kind IN ('exam', 'quiz', 'assignment', 'project')),
    max_score  NUMERIC(6,2) NOT NULL DEFAULT 100,
    weight     NUMERIC(5,2) NOT NULL DEFAULT 1,
    date       DATE NOT NULL DEFAULT CURRENT_DATE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX IF NOT EXISTS idx_assessments_class ON assessments (class_id);

CREATE TABLE IF NOT EXISTS grades (
    id            BIGSERIAL PRIMARY KEY,
    assessment_id BIGINT NOT NULL REFERENCES assessments(id) ON DELETE CASCADE,
    student_id    BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    score         NUMERIC(6,2),
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (assessment_id, student_id)
);
CREATE INDEX IF NOT EXISTS idx_grades_student ON grades (student_id);

CREATE TABLE IF NOT EXISTS invoices (
    id           BIGSERIAL PRIMARY KEY,
    student_id   BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    amount_cents BIGINT NOT NULL CHECK (amount_cents >= 0),
    due_date     DATE NOT NULL,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    created_by   BIGINT REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_invoices_student ON invoices (student_id);

CREATE TABLE IF NOT EXISTS payments (
    id           BIGSERIAL PRIMARY KEY,
    invoice_id   BIGINT NOT NULL REFERENCES invoices(id) ON DELETE CASCADE,
    amount_cents BIGINT NOT NULL CHECK (amount_cents > 0),
    method       TEXT NOT NULL DEFAULT 'cash' CHECK (method IN ('cash', 'card', 'bank_transfer', 'online', 'other')),
    reference    TEXT,
    paid_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    recorded_by  BIGINT REFERENCES users(id) ON DELETE SET NULL
);
CREATE INDEX IF NOT EXISTS idx_payments_invoice ON payments (invoice_id);
