BEGIN;
-- 18) HR: payroll, leave, staff attendance
INSERT INTO payroll_runs (staff_id, period_label, base_cents, allowance_cents, deduction_cents, net_cents, paid_on)
SELECT s.id, p.label, 400000, 50000, 30000, 420000, p.paid
FROM staff s CROSS JOIN (VALUES ('2025-09', CURRENT_DATE-40),('2025-10', CURRENT_DATE-10)) p(label, paid);

INSERT INTO leave_requests (staff_id, leave_type, start_date, end_date, reason, status, approved_by)
SELECT s.id, lt.t, CURRENT_DATE + lt.so, CURRENT_DATE + lt.eo, lt.r, lt.st,
       CASE WHEN lt.st = 'pending' THEN NULL ELSE (SELECT id FROM users WHERE lower(email)='admin@example.com') END
FROM staff s
JOIN (VALUES
  (1,'annual',5,9,'Family trip','pending'),
  (2,'sick',-3,-1,'Flu','approved'),
  (3,'personal',10,10,'Medical appointment','rejected'),
  (4,'annual',20,27,'Summer vacation','pending'),
  (5,'unpaid',-10,-8,'Personal matter','approved')) lt(sid,t,so,eo,r,st) ON s.id = lt.sid;

INSERT INTO staff_attendance (staff_id, date, status, recorded_by)
SELECT s.id, d::date,
  (ARRAY['present','present','present','present','late','leave'])[1 + floor(random()*6)::int],
  (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM staff s CROSS JOIN generate_series(CURRENT_DATE - 6, CURRENT_DATE, interval '1 day') d
WHERE EXTRACT(DOW FROM d) BETWEEN 1 AND 5;

-- 19) Welfare: medical records + behavior incidents
INSERT INTO medical_records (student_id, record_type, title, details, recorded_on, recorded_by)
SELECT s.id, m.rt, m.ti, m.de, CURRENT_DATE - (s.id % 90)::int, (SELECT user_id FROM staff WHERE job_title='School Nurse')
FROM students s
JOIN (VALUES
  (1,'allergy','Peanut allergy','Carries an EpiPen; notify on field trips'),
  (3,'condition','Asthma','Inhaler kept with class teacher'),
  (5,'immunization','MMR booster','Completed'),
  (7,'medication','ADHD medication','Administered by nurse at lunch'),
  (9,'allergy','Lactose intolerance','Dairy-free lunch required'),
  (11,'note','Vision','Wears glasses; front-row seating'),
  (14,'condition','Epilepsy','Seizure action plan on file'),
  (18,'immunization','Flu vaccine','Annual, consent on file')) m(sid,rt,ti,de) ON s.id = m.sid;

INSERT INTO behavior_incidents (student_id, incident_date, kind, category, description, action_taken, recorded_by)
SELECT s.id, CURRENT_DATE - (s.id % 30)::int, b.k, b.cat, b.de, b.act, (SELECT user_id FROM staff WHERE job_title='Vice Principal')
FROM students s
JOIN (VALUES
  (2,'positive','Citizenship','Helped a new student settle in','Merit awarded'),
  (4,'minor','Disruption','Talking during lesson','Verbal warning'),
  (6,'major','Conflict','Playground altercation','Parents contacted, detention'),
  (8,'minor','Uniform','Incorrect uniform','Reminder issued'),
  (10,'positive','Academic','Outstanding project','Certificate awarded'),
  (13,'minor','Homework','Repeatedly missing homework','Lunchtime catch-up')) b(sid,k,cat,de,act) ON s.id = b.sid;

-- 20) Notices + events
INSERT INTO notices (title, body, audience, published_at, expires_at, created_by)
VALUES
  ('Welcome back to Term 1','We look forward to a great term. Classes resume Monday.','all', now() - interval '20 days', now() + interval '30 days', (SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Parent-Teacher Evening','Bookings open next week for the parent-teacher evening.','parents', now() - interval '3 days', now() + interval '14 days', (SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Staff briefing Friday','Whole-staff briefing in the hall, 8am Friday.','staff', now() - interval '1 day', now() + interval '5 days', (SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Sports Day (past)','Thank you to everyone who attended Sports Day.','all', now() - interval '40 days', now() - interval '10 days', (SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Library week','Bring back overdue books this week, no fines.','all', now() - interval '2 days', now() + interval '7 days', (SELECT id FROM users WHERE lower(email)='admin@example.com'));

INSERT INTO events (title, description, starts_at, ends_at, location, created_by) VALUES
  ('Parent-Teacher Evening','Meet your child''s teachers', now() + interval '10 days', now() + interval '10 days 3 hours','School Hall',(SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Science Fair','Student science exhibition', now() + interval '21 days', now() + interval '21 days 5 hours','Gymnasium',(SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Term 1 Exams begin','Examination period', now() + interval '30 days', now() + interval '40 days','Exam Halls',(SELECT id FROM users WHERE lower(email)='admin@example.com')),
  ('Winter Concert','Annual winter concert', now() + interval '55 days', now() + interval '55 days 2 hours','Auditorium',(SELECT id FROM users WHERE lower(email)='admin@example.com'));

-- 21) Lessons + materials (2 per current class)
INSERT INTO lessons (class_id, subject_id, teacher_id, date, title, body, homework)
SELECT c.id,
       (SELECT id FROM subjects WHERE code = CASE WHEN n=1 THEN 'MATH' ELSE 'ENG' END),
       c.teacher_id, CURRENT_DATE - (n*3),
       CASE WHEN n=1 THEN 'Fractions: introduction' ELSE 'Creative writing: narrative' END,
       'Lesson covering the core objectives with worked examples.',
       CASE WHEN n=1 THEN 'Worksheet 3, questions 1-10' ELSE 'Write a 200-word story' END
FROM classes c CROSS JOIN generate_series(1,2) n
WHERE c.school_year_id = (SELECT id FROM school_years WHERE name='2025-26');

INSERT INTO lesson_materials (lesson_id, filename, storage_key, size_bytes)
SELECT l.id, 'handout.pdf', 'lesson/' || l.id || '/handout.pdf', 240000
FROM lessons l WHERE l.id % 2 = 1;

-- 22) Activities + members
INSERT INTO activities (name, kind, description, supervisor_id) VALUES
  ('Chess Club','club','Weekly chess practice and tournaments',(SELECT id FROM staff WHERE employee_no='EMP006')),
  ('Football Team','sport','School football squad',(SELECT id FROM staff WHERE employee_no='EMP005')),
  ('Drama Society','club','Termly stage productions',(SELECT id FROM staff WHERE employee_no='EMP004')),
  ('Science Club','club','Hands-on experiments',(SELECT id FROM staff WHERE employee_no='EMP003')),
  ('Choir','club','School choir',(SELECT id FROM staff WHERE employee_no='EMP002'));

INSERT INTO activity_members (activity_id, student_id, joined_on)
SELECT a.id, s.id, CURRENT_DATE - 30
FROM activities a
JOIN students s ON s.status='active' AND (s.id % 5) = ((a.id - 1) % 5)
WHERE s.id <= 25;

-- 23) Notifications (mix of read/unread)
INSERT INTO notifications (user_id, kind, title, body, link, read_at)
SELECT (SELECT id FROM users WHERE lower(email)='admin@example.com'), 'info', t.ti, t.bo, t.ln,
       CASE WHEN t.rd THEN now() - interval '1 hour' ELSE NULL END
FROM (VALUES
  ('New admission enquiry','Ruby Watson applied for Grade 1','/applicants', FALSE),
  ('Payment received','A tuition payment was recorded','/fees', TRUE),
  ('Leave request pending','Margaret Hale requested annual leave','/hr/leave', FALSE),
  ('Low book availability','Some titles are fully loaned out','/library', TRUE)) t(ti,bo,ln,rd);
INSERT INTO notifications (user_id, kind, title, body, link, read_at)
SELECT u.id, 'info', 'Welcome to the parent portal', 'You can view attendance, grades and fees here.', '/portal', NULL
FROM users u WHERE u.role='parent' AND u.id % 2 = 0;

-- 24) Email outbox (queued / sent / failed)
INSERT INTO email_outbox (to_addr, subject, body, status, attempts, sent_at) VALUES
  ('parent1@riverside.example','Fee reminder','Your Term 1 tuition is due.','queued',0,NULL),
  ('parent2@riverside.example','Attendance alert','Your child was marked absent today.','sent',1, now() - interval '2 hours'),
  ('parent3@riverside.example','Report card ready','Term 1 report cards are now available.','sent',1, now() - interval '1 day'),
  ('bad-address','Newsletter','Monthly newsletter','failed',3, NULL),
  ('parent4@riverside.example','Parent evening','Bookings are now open.','queued',0, NULL);

-- 25) Attachments (metadata; files not materialised on disk)
INSERT INTO attachments (entity, entity_id, filename, content_type, size_bytes, storage_key, uploaded_by)
SELECT 'student', s.id, 'birth-certificate.pdf', 'application/pdf', 180000,
       'student/' || s.id || '/' || md5(random()::text),
       (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM students s WHERE s.id <= 4;

-- 26) Audit log (recent admin actions)
INSERT INTO audit_log (user_id, action, entity, entity_id, detail)
SELECT (SELECT id FROM users WHERE lower(email)='admin@example.com'), a.act, a.ent, a.eid, a.det
FROM (VALUES
  ('create','student',1,'Enrolled new student'),
  ('update','student',3,'Updated guardian contact'),
  ('create','invoice',1,'Issued Term 1 tuition'),
  ('record','payment',1,'Recorded payment'),
  ('import','student',NULL,'CSV import: 30 inserted'),
  ('update','settings',NULL,'Set current school year')) a(act,ent,eid,det);

COMMIT;
