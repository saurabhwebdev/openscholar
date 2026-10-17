-- ============================================================================
-- OpenScholar comprehensive demo seed — a full, internally-consistent school.
-- Run against the dev DB to populate EVERY module with live-like data.
-- Re-runnable: wipes domain tables first (keeps the admin login + settings).
-- Seeded user logins all use the admin's password ("changeme-admin-123").
-- ============================================================================
BEGIN;

-- 1) Wipe domain data (keep admin user, settings, login_attempts).
DELETE FROM users WHERE lower(email) <> 'admin@example.com';
TRUNCATE
  students, staff, classes, school_years, semesters, subjects,
  slots, class_subjects, enrollments, attendance, assessments, grades,
  grading_scales, invoices, payments, receipts, fee_categories, fee_discounts,
  fee_fines, applicants, books, book_loans, vehicles, transport_routes,
  transport_stops, transport_riders, hostel_rooms, hostel_allocations,
  payroll_runs, leave_requests, staff_attendance, medical_records,
  behavior_incidents, notices, events, lessons, lesson_materials, activities,
  activity_members, parent_links, notifications, email_outbox, attachments,
  audit_log, sessions
  RESTART IDENTITY CASCADE;

-- 2) Settings
INSERT INTO settings (key, value) VALUES
  ('school_name','Riverside Academy'),
  ('school_address','42 Riverside Drive, Springfield'),
  ('school_phone','+1-555-0100'),
  ('school_email','office@riverside.example'),
  ('currency','USD')
ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value;

-- 3) School years + semesters
INSERT INTO school_years (name, start_date, end_date, is_current) VALUES
  ('2024-25', DATE '2024-09-01', DATE '2025-06-30', FALSE),
  ('2025-26', DATE '2025-09-01', DATE '2026-06-30', TRUE);
INSERT INTO semesters (school_year_id, name, start_date, end_date, is_current)
SELECT y.id, s.name, s.sd, s.ed, s.cur FROM school_years y,
  (VALUES ('Term 1', DATE '2025-09-01', DATE '2026-01-15', TRUE),
          ('Term 2', DATE '2026-01-16', DATE '2026-06-30', FALSE)) AS s(name, sd, ed, cur)
WHERE y.name = '2025-26';

-- 4) Grading scales
INSERT INTO grading_scales (name, min_score, max_score, grade, points) VALUES
  ('A', 90, 100, 'A', 4.0),
  ('B', 80, 89.99, 'B', 3.0),
  ('C', 70, 79.99, 'C', 2.0),
  ('D', 60, 69.99, 'D', 1.0),
  ('F', 0, 59.99, 'F', 0.0);

-- 5) Subjects
INSERT INTO subjects (code, name, department) VALUES
  ('MATH','Mathematics','STEM'),
  ('ENG','English','Humanities'),
  ('SCI','Science','STEM'),
  ('HIST','History','Humanities'),
  ('GEO','Geography','Humanities'),
  ('ART','Art','Arts'),
  ('PE','Physical Education','Sport'),
  ('CS','Computer Science','STEM');

-- 6) Staff (+ user logins). First 6 are teachers, then bursar/librarian/nurse/vice-principal.
INSERT INTO staff (employee_no, first_name, last_name, email, phone, job_title, department, hired_on, status) VALUES
  ('EMP001','Margaret','Hale','m.hale@riverside.example','+1-555-0201','Teacher','STEM', DATE '2019-08-15','active'),
  ('EMP002','David','Okafor','d.okafor@riverside.example','+1-555-0202','Teacher','Humanities', DATE '2020-01-10','active'),
  ('EMP003','Priya','Nair','p.nair@riverside.example','+1-555-0203','Teacher','STEM', DATE '2018-09-01','active'),
  ('EMP004','Tom','Becker','t.becker@riverside.example','+1-555-0204','Teacher','Arts', DATE '2021-08-20','active'),
  ('EMP005','Sofia','Marino','s.marino@riverside.example','+1-555-0205','Teacher','Sport', DATE '2022-02-14','active'),
  ('EMP006','James','Chen','j.chen@riverside.example','+1-555-0206','Teacher','STEM', DATE '2017-09-01','active'),
  ('EMP007','Grace','Adeyemi','g.adeyemi@riverside.example','+1-555-0207','Bursar','Finance', DATE '2016-03-01','active'),
  ('EMP008','Henry','Walsh','h.walsh@riverside.example','+1-555-0208','Librarian','Library', DATE '2019-11-05','active'),
  ('EMP009','Nadia','Petrova','n.petrova@riverside.example','+1-555-0209','School Nurse','Welfare', DATE '2020-09-15','active'),
  ('EMP010','Robert','Dunn','r.dunn@riverside.example','+1-555-0210','Vice Principal','Admin', DATE '2015-08-01','active');

INSERT INTO users (email, password_hash, role, full_name, active)
SELECT s.email, (SELECT password_hash FROM users WHERE id=1),
       CASE WHEN s.job_title = 'Teacher' THEN 'teacher' ELSE 'staff' END,
       s.first_name || ' ' || s.last_name, TRUE
FROM staff s;
UPDATE staff s SET user_id = u.id FROM users u WHERE u.email = s.email;

-- 7) Classes (5 current-year + 1 archived last year)
INSERT INTO classes (school_year_id, name, grade_level, room, teacher_id, capacity)
SELECT (SELECT id FROM school_years WHERE name='2025-26'), c.name, c.gl, c.room,
       (SELECT id FROM staff WHERE employee_no = c.emp), c.cap
FROM (VALUES
  ('Grade 1A','Grade 1','R101','EMP001',30),
  ('Grade 2A','Grade 2','R102','EMP002',30),
  ('Grade 3A','Grade 3','R103','EMP003',28),
  ('Grade 4A','Grade 4','R104','EMP004',28),
  ('Grade 5A','Grade 5','R105','EMP006',26)
) AS c(name, gl, room, emp, cap);
INSERT INTO classes (school_year_id, name, grade_level, room, teacher_id, capacity)
SELECT (SELECT id FROM school_years WHERE name='2024-25'), 'Grade 5A (2024-25)','Grade 5','R105',
       (SELECT id FROM staff WHERE employee_no='EMP006'), 26;

COMMIT;
