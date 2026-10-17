BEGIN;
-- Convenience: admin user id for recorded_by/created_by.
-- (inlined as a subquery below to keep this a plain SQL script)

-- 10) Timetable: class_subjects + a full weekly slot grid for current classes
INSERT INTO class_subjects (class_id, subject_id, teacher_id)
SELECT c.id, su.id, c.teacher_id
FROM classes c CROSS JOIN subjects su
WHERE c.school_year_id = (SELECT id FROM school_years WHERE name='2025-26');

WITH cur AS (
  SELECT id, teacher_id, room FROM classes
  WHERE school_year_id = (SELECT id FROM school_years WHERE name='2025-26')),
np AS (SELECT id, (row_number() OVER (ORDER BY id)) - 1 AS rn FROM periods),
ns AS (SELECT id, (row_number() OVER (ORDER BY id)) - 1 AS sn FROM subjects)
INSERT INTO slots (class_id, subject_id, period_id, teacher_id, room)
SELECT c.id, ns.id, np.id, c.teacher_id, c.room
FROM cur c
CROSS JOIN np
JOIN ns ON ns.sn = (np.rn % (SELECT COUNT(*) FROM subjects));

-- 11) Assessments (3 per current class) + grades for every enrolled student
INSERT INTO assessments (class_id, name, kind, max_score, weight, date)
SELECT c.id, a.nm, a.kind, a.mx, a.w, a.dt
FROM classes c
CROSS JOIN (VALUES
  ('Quiz 1','quiz',20,1,CURRENT_DATE-40),
  ('Midterm Exam','exam',100,2,CURRENT_DATE-20),
  ('Final Project','project',100,3,CURRENT_DATE-5)) a(nm,kind,mx,w,dt)
WHERE c.school_year_id = (SELECT id FROM school_years WHERE name='2025-26');

INSERT INTO grades (assessment_id, student_id, score)
SELECT a.id, e.student_id, round((a.max_score * (0.55 + random()*0.4))::numeric, 1)
FROM enrollments e
JOIN assessments a ON a.class_id = e.class_id
WHERE random() < 0.95;  -- a few left ungraded, like real life

-- 12) Attendance: last ~3 weeks of weekdays for every enrollment
INSERT INTO attendance (student_id, class_id, date, status, recorded_by)
SELECT e.student_id, e.class_id, d::date,
  (ARRAY['present','present','present','present','present','present','present','late','absent','excused'])[1 + floor(random()*10)::int],
  (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM enrollments e
CROSS JOIN generate_series(CURRENT_DATE - 20, CURRENT_DATE, interval '1 day') d
WHERE EXTRACT(DOW FROM d) BETWEEN 1 AND 5;

-- 13) Fees: categories, invoices, payments, receipts, fines, discounts
INSERT INTO fee_categories (name, description) VALUES
  ('Tuition','Termly tuition fee'),
  ('Transport','Bus service'),
  ('Hostel','Boarding'),
  ('Exam','Examination fees'),
  ('Library','Library membership');

INSERT INTO invoices (student_id, title, amount_cents, due_date, category_id, created_by)
SELECT s.id, 'Term 1 Tuition', 150000, CURRENT_DATE - 10,
       (SELECT id FROM fee_categories WHERE name='Tuition'),
       (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM students s WHERE s.status='active';
INSERT INTO invoices (student_id, title, amount_cents, due_date, category_id, created_by)
SELECT s.id, 'Examination Fee', 20000, CURRENT_DATE + 20,
       (SELECT id FROM fee_categories WHERE name='Exam'),
       (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM students s WHERE s.status='active' AND s.id % 2 = 0;

-- Payments: ~1/3 fully paid, ~1/3 partial, ~1/3 unpaid (overdue)
INSERT INTO payments (invoice_id, amount_cents, method, reference, paid_at, recorded_by)
SELECT i.id,
       CASE WHEN i.id % 3 = 0 THEN i.amount_cents ELSE (i.amount_cents/2) END,
       (ARRAY['cash','card','bank_transfer','online'])[1 + (i.id % 4)],
       'REF-' || i.id,
       now() - (i.id % 10 || ' days')::interval,
       (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM invoices i WHERE i.id % 3 <> 1;
INSERT INTO receipts (payment_id, number, issued_by)
SELECT p.id, 'R-' || p.id, (SELECT id FROM users WHERE lower(email)='admin@example.com')
FROM payments p;

-- A few late fines on unpaid tuition invoices
INSERT INTO fee_fines (invoice_id, amount_cents, reason)
SELECT i.id, 2500, 'Late payment fee'
FROM invoices i WHERE i.title='Term 1 Tuition' AND i.id % 3 = 1 LIMIT 5;

-- Discounts (one sibling discount scoped to a student, one scholarship scoped to Tuition)
INSERT INTO fee_discounts (name, percent, student_id, category_id) VALUES
  ('Sibling discount', 10, (SELECT MIN(id) FROM students WHERE status='active'), NULL),
  ('Scholarship', 25, NULL, (SELECT id FROM fee_categories WHERE name='Tuition'));

-- 14) Admissions applicants (various statuses)
INSERT INTO applicants (first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, applying_for, status, notes) VALUES
  ('Ruby','Watson', DATE '2016-04-12','female','Alice Watson','+1-555-0301','a.watson@example.com','Grade 1','new','Walk-in enquiry'),
  ('Max','Fischer', DATE '2015-07-30','male','Paul Fischer','+1-555-0302','p.fischer@example.com','Grade 2','reviewing','Awaiting transcripts'),
  ('Zoe','Kapoor', DATE '2014-02-19','female','Neha Kapoor','+1-555-0303','n.kapoor@example.com','Grade 3','accepted','Offer sent'),
  ('Dylan','Murphy', DATE '2013-11-05','male','Sean Murphy','+1-555-0304','s.murphy@example.com','Grade 4','rejected','Catchment full'),
  ('Aisha','Rahman', DATE '2016-09-21','female','Omar Rahman','+1-555-0305','o.rahman@example.com','Grade 1','accepted','Scholarship candidate'),
  ('Theo','Nowak', DATE '2015-05-14','male','Ewa Nowak','+1-555-0306','e.nowak@example.com','Grade 2','new',NULL);

-- 15) Library: books + loans
INSERT INTO books (isbn, title, author, category, copies, available) VALUES
  ('978-0001','Charlotte''s Web','E. B. White','Fiction',5,5),
  ('978-0002','Matilda','Roald Dahl','Fiction',4,4),
  ('978-0003','The Hobbit','J. R. R. Tolkien','Fantasy',3,3),
  ('978-0004','A Wrinkle in Time','Madeleine L''Engle','Sci-Fi',3,3),
  ('978-0005','Wonder','R. J. Palacio','Fiction',6,6),
  ('978-0006','Percy Jackson','Rick Riordan','Fantasy',5,5),
  ('978-0007','The Giver','Lois Lowry','Dystopia',4,4),
  ('978-0008','Holes','Louis Sachar','Fiction',4,4),
  ('978-0009','Bridge to Terabithia','Katherine Paterson','Fiction',3,3),
  ('978-0010','National Geographic Kids','Various','Reference',8,8),
  ('978-0011','Introduction to Algebra','S. Lang','Textbook',10,10),
  ('978-0012','World Atlas','DK','Reference',6,6),
  ('978-0013','The Magic School Bus','Joanna Cole','Science',7,7),
  ('978-0014','Diary of a Wimpy Kid','Jeff Kinney','Humor',6,6),
  ('978-0015','Oxford Dictionary','Oxford','Reference',5,5);

INSERT INTO book_loans (book_id, student_id, borrowed_on, due_on, returned_on)
SELECT b.id, s.id, CURRENT_DATE - 14, CURRENT_DATE - 0,
       CASE WHEN b.id % 2 = 0 THEN CURRENT_DATE - 2 ELSE NULL END
FROM books b
JOIN students s ON s.id = b.id  -- loan book N to student N (first 12)
WHERE b.id <= 12 AND s.status='active';
-- Reconcile availability with open loans
UPDATE books SET available = copies - (
  SELECT COUNT(*) FROM book_loans l WHERE l.book_id = books.id AND l.returned_on IS NULL);

-- 16) Transport: vehicles, routes, stops, riders
INSERT INTO vehicles (reg_no, kind, capacity, driver_name, driver_phone, active) VALUES
  ('BUS-01','bus',40,'Frank Miller','+1-555-0401',TRUE),
  ('BUS-02','bus',40,'Grace Lee','+1-555-0402',TRUE),
  ('VAN-01','van',14,'Hassan Ali','+1-555-0403',TRUE);
INSERT INTO transport_routes (name, vehicle_id, fee_cents) VALUES
  ('North Route', (SELECT id FROM vehicles WHERE reg_no='BUS-01'), 500000),
  ('South Route', (SELECT id FROM vehicles WHERE reg_no='BUS-02'), 500000),
  ('East Route',  (SELECT id FROM vehicles WHERE reg_no='VAN-01'), 350000);
INSERT INTO transport_stops (route_id, name, sequence)
SELECT r.id, r.name || ' Stop ' || g, g
FROM transport_routes r CROSS JOIN generate_series(1,3) g;
INSERT INTO transport_riders (student_id, route_id, stop_id)
SELECT s.id, r.id, (SELECT st.id FROM transport_stops st WHERE st.route_id=r.id ORDER BY st.sequence LIMIT 1)
FROM students s
JOIN transport_routes r ON r.id = (SELECT id FROM transport_routes ORDER BY id LIMIT 1 OFFSET (s.id % 3))
WHERE s.status='active' AND s.id % 2 = 1;

-- 17) Hostel: rooms + allocations
INSERT INTO hostel_rooms (name, block, capacity) VALUES
  ('Room A1','Block A',4),('Room A2','Block A',4),('Room B1','Block B',4),
  ('Room B2','Block B',4),('Room C1','Block C',2),('Room C2','Block C',2);
INSERT INTO hostel_allocations (student_id, room_id, bed_label, allocated_on, released_on)
SELECT s.id,
       (SELECT id FROM hostel_rooms ORDER BY id LIMIT 1 OFFSET (s.id % 6)),
       'Bed ' || s.id,
       CURRENT_DATE - 60,
       CASE WHEN s.id % 7 = 0 THEN CURRENT_DATE - 5 ELSE NULL END
FROM students s WHERE s.status='active' AND s.id % 3 = 0;

COMMIT;
