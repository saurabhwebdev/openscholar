BEGIN;
-- 8) Students (30)
INSERT INTO students (admission_no, first_name, last_name, date_of_birth, gender, guardian_name, guardian_phone, guardian_email, address, status, enrolled_on)
SELECT
  'S' || lpad(g::text, 4, '0'),
  (ARRAY['Olivia','Noah','Emma','Liam','Ava','Oliver','Sophia','Elijah','Isabella','Lucas','Mia','Mason','Amelia','Ethan','Harper','James','Evelyn','Benjamin','Abigail','Henry','Ella','Alexander','Scarlett','Sebastian','Grace','Jack','Chloe','Owen','Lily','Leo'])[g],
  (ARRAY['Anderson','Brown','Clark','Davis','Evans','Foster','Garcia','Harris','Ingram','Jones','Khan','Lewis','Morgan','Nguyen','Owens','Patel','Quinn','Reed','Singh','Turner'])[1 + (g % 20)],
  DATE '2012-01-01' + ((g * 53) % 1400),
  (ARRAY['female','male'])[1 + (g % 2)],
  'Guardian of #' || g,
  '+1-555-' || lpad((1000 + g)::text, 4, '0'),
  'parent' || g || '@riverside.example',
  (10 + g) || ' Maple Street, Springfield',
  CASE WHEN g = 30 THEN 'graduated' WHEN g = 29 THEN 'inactive' ELSE 'active' END,
  CURRENT_DATE - ((g * 11) % 300)
FROM generate_series(1, 30) g;

-- Parent user logins (one per student) + links
INSERT INTO users (email, password_hash, role, full_name, active)
SELECT s.guardian_email, (SELECT password_hash FROM users WHERE id=1), 'parent',
       s.guardian_name, TRUE
FROM students s
WHERE s.guardian_email IS NOT NULL AND s.guardian_email <> ''
ON CONFLICT DO NOTHING;
INSERT INTO parent_links (parent_user_id, student_id, relation)
SELECT u.id, s.id, (ARRAY['mother','father','guardian'])[1 + (s.id % 3)]
FROM students s JOIN users u ON u.email = s.guardian_email;

-- Student logins for the first 12 students
INSERT INTO users (email, password_hash, role, full_name, active)
SELECT 'student' || s.id || '@riverside.example', (SELECT password_hash FROM users WHERE id=1),
       'student', s.first_name || ' ' || s.last_name, TRUE
FROM students s WHERE s.id <= 12;
UPDATE students s SET user_id = u.id
FROM users u WHERE u.email = 'student' || s.id || '@riverside.example';

-- 9) Enrollments: distribute 30 students across the 5 current-year classes
INSERT INTO enrollments (student_id, class_id, enrolled_on)
SELECT s.id, c.id, s.enrolled_on
FROM students s
JOIN classes c ON c.name = 'Grade ' || (1 + ((s.id - 1) % 5)) || 'A'
  AND c.school_year_id = (SELECT id FROM school_years WHERE name='2025-26')
WHERE s.status = 'active';
COMMIT;
