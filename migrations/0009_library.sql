CREATE TABLE IF NOT EXISTS books (id BIGSERIAL PRIMARY KEY, isbn TEXT, title TEXT NOT NULL, author TEXT, category TEXT, copies INT NOT NULL DEFAULT 1, available INT NOT NULL DEFAULT 1, created_at TIMESTAMPTZ NOT NULL DEFAULT now(), CHECK (available >= 0 AND available <= copies));
CREATE TABLE IF NOT EXISTS book_loans (id BIGSERIAL PRIMARY KEY, book_id BIGINT NOT NULL REFERENCES books(id) ON DELETE CASCADE, student_id BIGINT NOT NULL REFERENCES students(id) ON DELETE CASCADE, borrowed_on DATE NOT NULL DEFAULT CURRENT_DATE, due_on DATE NOT NULL, returned_on DATE NULL, created_at TIMESTAMPTZ NOT NULL DEFAULT now());
CREATE INDEX IF NOT EXISTS idx_loans_student ON book_loans (student_id);
CREATE INDEX IF NOT EXISTS idx_loans_book ON book_loans (book_id);
CREATE INDEX IF NOT EXISTS idx_loans_open ON book_loans (book_id) WHERE returned_on IS NULL;
